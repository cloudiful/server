# cloudiful-server

Small Rust server bootstrap crate for Cloud1ful services. It centralizes shared
listen address, CORS, TLS, Axum, Actix, and MCP transport setup without owning
application routing.

## Features

- Default: none
- `actix`: Actix `App` startup with TLS and CORS helpers
- `axum`: Axum `Router` startup with optional shared state
- `mcp`: rmcp stdio helpers plus Streamable HTTP service, router, and server helpers
- `tls`: omit `tls` for plain HTTP, set `cert_path` + `cert_key_path` for server TLS, and add `client_ca` for mTLS

## Core Config

```rust
use cloudiful_server::{CorsConfig, ServerConfig};

let config = ServerConfig::new()
    .with_listen_addr("127.0.0.1:3000")
    .with_cors(CorsConfig::restricted(["https://intranet.example.com"]))
    .build()?;
```

Listen addresses like `:8080` are normalized to `0.0.0.0:8080`.

TLS is opt-in:

```rust
use cloudiful_server::{ServerConfig, TlsConfig};

let config = ServerConfig::new()
    .with_tls(
        TlsConfig::new()
            .with_cert_path("cert.pem")
            .with_cert_key_path("key.pem"),
    )
    .build()?;
```

mTLS uses the same `TlsConfig` with an extra client CA bundle:

```rust
use cloudiful_server::{ServerConfig, TlsConfig};

let config = ServerConfig::new()
    .with_tls(
        TlsConfig::new()
            .with_cert_path("/etc/cloudiful/tls/server.crt")
            .with_cert_key_path("/etc/cloudiful/tls/server.key")
            .with_client_ca("/etc/cloudiful/tls/client-ca.crt"),
    )
    .build()?;
```

When `client_ca` is not configured, startup keeps the existing server-only TLS behavior.
When `client_ca` is configured, the server requests and verifies client certificates
against that CA bundle.

## Actix

Enable with `features = ["actix"]`.

```rust
use actix_web::{HttpResponse, web};
use cloudiful_server::{Server, ServerConfig};

let config = ServerConfig::new().build()?;

Server::new(config, |cfg| {
    cfg.route("/healthz", web::get().to(|| async { HttpResponse::Ok().body("ok") }));
})
.start()
.await?;
```

## Axum

Enable with `features = ["axum"]`.

```rust
use axum::{Router, extract::State, routing::get};
use cloudiful_server::ServerConfig;

#[derive(Clone)]
struct AppState {
    service_name: String,
}

let config = ServerConfig::new()
    .with_app_data(AppState {
        service_name: "orders".to_string(),
    })
    .build()?;

let app = Router::new().route(
    "/healthz",
    get(|State(state): State<AppState>| async move { format!("{} ok", state.service_name) }),
);

cloudiful_server::axum::Server::new_with_state(config, app)
    .start()
    .await?;
```

## MCP

Enable with `features = ["mcp"]`.

```rust
use cloudiful_server::mcp::{self, tool, tool_router};

#[derive(Clone)]
struct Calculator;

#[tool_router(server_handler)]
impl Calculator {
    #[tool(description = "Add two numbers")]
    fn add(&self, a: i32, b: i32) -> String {
        (a + b).to_string()
    }
}
```

Run over stdio:

```rust
let server = mcp::serve_stdio(Calculator).await?;
server.waiting().await?;
```

Run as a standalone Streamable HTTP server:

```rust
use cloudiful_server::ServerConfig;

let http = ServerConfig::new()
    .with_listen_addr("127.0.0.1:8000")
    .build()?;

mcp::Server::new(http, || Calculator)
    .with_server_config(mcp::ServerConfig::new().with_service_path("/mcp"))
    .start()
    .await?;
```

Embed into an existing Axum router:

```rust
use axum::{Router, routing::get};

let mcp_service = mcp::service(mcp::ServerConfig::new(), || Calculator)?;

let app = Router::new()
    .route("/healthz", get(|| async { "ok" }))
    .nest_service("/mcp", mcp_service);
```

Build an MCP-only router:

```rust
let app = mcp::router(
    mcp::ServerConfig::new().with_service_path("/mcp"),
    || Calculator,
)?;
```

`mcp::service` is the shared construction path. `mcp::router` and
`mcp::Server` build on top of it.

### Host validation and protocol version

By default the Streamable HTTP service only accepts loopback `Host` headers
(`localhost`, `127.0.0.1`, `::1`) to prevent DNS rebinding attacks. Public
deployments must allowlist their own hostnames:

```rust
let mcp_config = mcp::ServerConfig::new().with_allowed_hosts(["mcp.example.com"]);
```

`2026-07-28` protocol requests are always served statelessly.
`with_legacy_session_mode` only controls whether clients negotiating an older
protocol version get MCP sessions:

```rust
let mcp_config = mcp::ServerConfig::new().with_legacy_session_mode(false);
```

Additional Streamable HTTP options: `with_json_response`,
`with_max_request_body_bytes`, and `with_stateless_protocol_metadata_required`.

### Stateless SSE event recovery

For cross-instance recovery of stateless SSE streams, attach a shared event
store so clients can resume with `Last-Event-ID`:

```rust
use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;
use cloudiful_server::mcp::{
    self, EventStore, EventStoreError, EventId, EventStream, ServerSseMessage,
};

#[derive(Default)]
struct InMemoryEventStore(Arc<RwLock<HashMap<String, Vec<(EventId, ServerSseMessage)>>>>);

#[async_trait::async_trait]
impl EventStore for InMemoryEventStore {
    async fn store_event(
        &self,
        stream_id: &str,
        event: &ServerSseMessage,
    ) -> Result<EventId, EventStoreError> {
        let mut streams = self.0.write().await;
        let events = streams.entry(stream_id.to_string()).or_default();
        // The ID must be globally unique and identify its stream, because
        // replay_events_after only receives the ID back.
        let id = format!("{stream_id}:{}", events.len());
        events.push((id.clone(), event.clone()));
        Ok(id)
    }

    async fn replay_events_after(
        &self,
        last_event_id: &str,
    ) -> Result<EventStream, EventStoreError> {
        let streams = self.0.read().await;
        let (stream_id, index) = last_event_id.rsplit_once(':').ok_or("invalid event id")?;
        let index: usize = index.parse().map_err(|_| "invalid event id")?;
        let events = streams.get(stream_id).cloned().unwrap_or_default();
        let replayed = events.into_iter().skip(index + 1).map(|(_, event)| event);
        Ok(Box::pin(futures::stream::iter(replayed)))
    }
}

let mcp_config = mcp::ServerConfig::new().with_event_store(Arc::new(InMemoryEventStore::default()));
```

### Legacy session recovery

To support cross-instance session recovery for legacy clients, attach an
external session store:

```rust
use std::sync::Arc;

use cloudiful_server::mcp::{self, SessionStore};

struct MyStore;

#[async_trait::async_trait]
impl SessionStore for MyStore {
    async fn load(
        &self,
        session_id: &str,
    ) -> Result<Option<mcp::SessionState>, mcp::SessionStoreError> {
        let _ = session_id;
        Ok(None)
    }

    async fn store(
        &self,
        session_id: &str,
        state: &mcp::SessionState,
    ) -> Result<(), mcp::SessionStoreError> {
        let _ = (session_id, state);
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<(), mcp::SessionStoreError> {
        let _ = session_id;
        Ok(())
    }
}

let mcp_config = mcp::ServerConfig::new().with_session_store(Arc::new(MyStore));
```

## Testing

```bash
cargo test
cargo test --features actix
cargo test --features mcp
cargo test --no-default-features --features axum
cargo test --all-features
```
