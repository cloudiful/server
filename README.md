# cloudiful-server

Single crate server bootstrap library with shared config/TLS handling and
feature-gated Actix/Axum/MCP adapters.

Published to:

- crates.io as `cloudiful-server`
- Kellnr as `cloudiful-server`

## Features

- `actix` is enabled by default and keeps the existing `Server::new(config, |cfg| ...)` API
- `axum` adds `server::axum::Server` with native `Router` and `with_state` support
- `mcp` adds stdio MCP helpers plus a streamable HTTP MCP server wrapper

## Actix example

```rust
use actix_web::{HttpResponse, web};
use cloudiful_server::{CorsConfig, Server, ServerConfig};

#[derive(Debug)]
struct AppState {
    service_name: String,
}

#[actix_web::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::new()
        .with_listen_addr("127.0.0.1:3000")
        .with_app_data(web::Data::new(AppState {
            service_name: "orders".to_string(),
        }))
        .with_cors(CorsConfig::restricted(["https://intranet.example.com"]))
        .build()?;

    Server::new(config, |cfg| {
        cfg.route(
            "/health",
            web::get().to(|state: web::Data<AppState>| async move {
                HttpResponse::Ok().body(format!("{} ok", state.service_name))
            }),
        );
    })
    .start()
    .await?;

    Ok(())
}
```

## Axum example

```rust
use axum::{Router, extract::State, routing::get};
use cloudiful_server::{CorsConfig, ServerConfig};

#[derive(Clone)]
struct AppState {
    service_name: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::new()
        .with_listen_addr("127.0.0.1:3000")
        .with_app_data(AppState {
            service_name: "orders".to_string(),
        })
        .with_cors(CorsConfig::restricted(["https://intranet.example.com"]))
        .build()?;

    let app = Router::new().route(
        "/health",
        get(|State(state): State<AppState>| async move { format!("{} ok", state.service_name) }),
    );

    cloudiful_server::axum::Server::new_with_state(config, app)
        .start()
        .await?;

    Ok(())
}
```

## MCP stdio example

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = mcp::serve_stdio(Calculator).await?;
    server.waiting().await?;
    Ok(())
}
```

## MCP streamable HTTP example

```rust
use cloudiful_server::{
    ServerConfig,
    mcp::{self, tool, tool_router},
};

#[derive(Clone)]
struct Calculator;

#[tool_router(server_handler)]
impl Calculator {
    #[tool(description = "Add two numbers")]
    fn add(&self, a: i32, b: i32) -> String {
        (a + b).to_string()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::new()
        .with_listen_addr("127.0.0.1:8000")
        .build()?;

    mcp::Server::new(config, || Calculator)
        .with_server_config(mcp::ServerConfig::new().with_service_path("/mcp"))
        .start()
        .await?;

    Ok(())
}
```

## Testing

```bash
cargo test
cargo test --features mcp
cargo test --no-default-features --features axum
cargo test --all-features
```
