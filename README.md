# cloudiful-server

Small Rust server bootstrap crate for Cloud1ful services. It centralizes shared
listen address, CORS, TLS, Axum, Actix, and MCP transport setup without owning
application routing.

## Features

- Default: `actix`
- `axum`: Axum `Router` startup with optional shared state
- `mcp`: rmcp stdio helpers plus Streamable HTTP service, router, and server helpers

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

## Actix

Enabled by default.

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

## Testing

```bash
cargo test
cargo test --features mcp
cargo test --no-default-features --features axum
cargo test --all-features
```
