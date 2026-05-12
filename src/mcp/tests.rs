use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use rmcp::ServiceExt;
use tower::ServiceExt as TowerServiceExt;

use crate::{
    ServerConfig as HttpServerConfig,
    mcp::{Server, serve, tool, tool_router},
};

#[derive(Clone)]
struct PingServer;

#[tool_router(server_handler)]
impl PingServer {
    #[tool(description = "Ping the server")]
    fn ping(&self) -> String {
        "pong".to_string()
    }
}

#[tokio::test]
async fn serve_over_async_rw_transport_exposes_tools() {
    let (server_io, client_io) = tokio::io::duplex(1024);
    let (server_read, server_write) = tokio::io::split(server_io);
    let (client_read, client_write) = tokio::io::split(client_io);

    let server_task =
        tokio::spawn(async move { serve(PingServer, (server_read, server_write)).await });

    let client = ().serve((client_read, client_write)).await.unwrap();
    let tools = client.peer().list_tools(Default::default()).await.unwrap();

    assert_eq!(tools.tools.len(), 1);
    assert_eq!(tools.tools[0].name, "ping");

    client.cancel().await.unwrap();
    server_task.await.unwrap().unwrap().cancel().await.unwrap();
}

#[tokio::test]
async fn streamable_http_server_binds_on_loopback() {
    let config = HttpServerConfig::new()
        .with_listen_addr("127.0.0.1:0")
        .build()
        .unwrap();

    let bound = Server::new(config, || PingServer)
        .with_server_config(crate::mcp::ServerConfig::new().with_service_path("/mcp"))
        .bind()
        .unwrap();

    assert!(bound.addrs().iter().all(|addr| addr.ip().is_loopback()));
}

#[tokio::test]
async fn service_path_is_normalized_for_http_router() {
    let response = crate::mcp::router(
        crate::mcp::ServerConfig::new().with_service_path("mcp/"),
        || PingServer,
    )
    .unwrap()
    .oneshot(
        Request::builder()
            .uri("/mcp")
            .header("host", "not-loopback.example")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();

    assert_ne!(response.status(), StatusCode::NOT_FOUND);
    assert_ne!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn root_service_path_is_accepted() {
    let response = crate::mcp::router(
        crate::mcp::ServerConfig::new().with_service_path("/"),
        || PingServer,
    )
    .unwrap()
    .oneshot(
        Request::builder()
            .uri("/")
            .header("host", "not-loopback.example")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();

    assert_ne!(response.status(), StatusCode::NOT_FOUND);
    assert_ne!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn service_can_be_nested_in_existing_router() {
    let service = crate::mcp::service(crate::mcp::ServerConfig::new(), || PingServer).unwrap();
    let app = Router::new().nest_service("/mcp", service);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/mcp")
                .header("host", "not-loopback.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_ne!(response.status(), StatusCode::NOT_FOUND);
    assert_ne!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn empty_service_path_is_rejected() {
    let config = HttpServerConfig::new()
        .with_listen_addr("127.0.0.1:0")
        .build()
        .unwrap();

    let err = Server::new(config, || PingServer)
        .with_server_config(crate::mcp::ServerConfig::new().with_service_path(" "))
        .bind()
        .unwrap_err();

    assert!(matches!(
        err,
        crate::mcp::McpServerError::InvalidServicePath(_)
    ));
}
