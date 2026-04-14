use ::axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Method, Request, StatusCode, header},
    routing::get,
};
use tower::ServiceExt;

use crate::{CorsConfig, ServerConfig, ServerError, TlsConfig, TlsConfigLoadError, axum::Server};

#[derive(Clone)]
struct AppState {
    message: String,
}

#[tokio::test]
async fn http_server_serves_requests() {
    let config = ServerConfig::new()
        .with_listen_addr("127.0.0.1:0")
        .build()
        .unwrap();

    let bound = Server::new(
        config.clone(),
        Router::new().route("/health", get(|| async { "ok" })),
    )
    .bind()
    .unwrap();

    assert!(bound.addrs().iter().all(|addr| addr.ip().is_loopback()));

    let app = Server::new(
        config,
        Router::new().route("/health", get(|| async { "ok" })),
    )
    .into_router();
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body, "ok");
}

#[tokio::test]
async fn state_is_available_in_handlers() {
    let config = ServerConfig::new()
        .with_app_data(AppState {
            message: "from-state".to_string(),
        })
        .build()
        .unwrap();

    let app = Router::new().route(
        "/state",
        get(|State(state): State<AppState>| async move { state.message }),
    );
    let app = Server::new_with_state(config, app).into_router();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body, "from-state");
}

#[tokio::test]
async fn restricted_cors_sets_preflight_headers() {
    let config = ServerConfig::new()
        .with_cors(
            CorsConfig::restricted(["https://allowed.example"])
                .with_allowed_methods(["GET", "POST"]),
        )
        .build()
        .unwrap();

    let app = Router::new().route("/health", get(|| async { "ok" }));
    let app = Server::new(config, app).into_router();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/health")
                .header(header::ORIGIN, "https://allowed.example")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(matches!(
        response.status(),
        StatusCode::OK | StatusCode::NO_CONTENT
    ));
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "https://allowed.example"
    );
    assert!(
        response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_METHODS)
    );
}

#[tokio::test]
async fn invalid_tls_paths_return_structured_errors() {
    let config = ServerConfig::new()
        .with_listen_addr("127.0.0.1:0")
        .with_tls(
            TlsConfig::new()
                .with_cert_path("missing-cert.pem")
                .with_cert_key_path("missing-key.pem"),
        )
        .build()
        .unwrap();

    let app = Router::new().route("/health", get(|| async { "ok" }));
    let err = match Server::new(config, app).bind() {
        Ok(_) => panic!("expected TLS binding to fail for missing certificate files"),
        Err(err) => err,
    };

    match err {
        ServerError::Tls(TlsConfigLoadError::OpenCertificate { path, .. }) => {
            assert_eq!(path, std::path::PathBuf::from("missing-cert.pem"));
        }
        other => panic!("unexpected error: {other}"),
    }
}
