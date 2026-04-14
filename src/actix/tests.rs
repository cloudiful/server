use actix_web::{
    App, HttpResponse,
    http::{Method, StatusCode, header},
    test, web,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Duration, sleep},
};

use crate::{CorsConfig, Server, ServerConfig, ServerError, TlsConfig, TlsConfigLoadError};

use super::cors::build_cors;

#[derive(Debug)]
struct TestState {
    message: String,
}

#[actix_web::test]
async fn restricted_cors_adds_expected_headers() {
    let cors =
        CorsConfig::restricted(["https://allowed.example"]).with_allowed_methods(["GET", "POST"]);

    let app = test::init_service(App::new().wrap(build_cors(&cors)).route(
        "/health",
        web::get().to(|| async { HttpResponse::Ok().finish() }),
    ))
    .await;

    let req = test::TestRequest::default()
        .method(Method::OPTIONS)
        .uri("/health")
        .insert_header((header::ORIGIN, "https://allowed.example"))
        .insert_header((header::ACCESS_CONTROL_REQUEST_METHOD, "GET"))
        .to_request();

    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "https://allowed.example"
    );
    let allowed_methods = resp
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_METHODS)
        .unwrap()
        .to_str()
        .unwrap();

    assert!(allowed_methods.contains("GET"));
    assert!(allowed_methods.contains("POST"));
}

#[actix_web::test]
async fn http_server_serves_requests_and_applies_bind_addr() {
    let config = ServerConfig::new()
        .with_listen_addr("127.0.0.1:0")
        .with_app_data(web::Data::new(TestState {
            message: "from-state".to_string(),
        }))
        .build()
        .unwrap();

    let bound = Server::new(config, |cfg| {
        cfg.route(
            "/state",
            web::get().to(|state: web::Data<TestState>| async move {
                HttpResponse::Ok().body(state.message.clone())
            }),
        );
    })
    .bind()
    .unwrap();

    assert!(bound.addrs().iter().all(|addr| addr.ip().is_loopback()));

    let addr = bound.addrs()[0];
    let handle = bound.handle();
    let server_task = actix_web::rt::spawn(bound.run());

    let response = send_http_request(
        addr,
        "GET /state HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;

    handle.stop(true).await;
    let run_result = server_task.await.unwrap();

    assert!(matches!(run_result, Ok(())));
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("\r\n\r\nfrom-state"));
}

#[actix_web::test]
async fn invalid_tls_paths_return_errors_without_panicking() {
    let config = ServerConfig::new()
        .with_listen_addr("127.0.0.1:0")
        .with_tls(
            TlsConfig::new()
                .with_cert_path("missing-cert.pem")
                .with_cert_key_path("missing-key.pem"),
        )
        .build()
        .unwrap();

    let err = match Server::new(config, |_| {}).bind() {
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

async fn send_http_request(addr: std::net::SocketAddr, request: &str) -> String {
    let mut stream = connect_with_retry(addr).await;
    stream.write_all(request.as_bytes()).await.unwrap();
    stream.shutdown().await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();

    String::from_utf8(response).unwrap()
}

async fn connect_with_retry(addr: std::net::SocketAddr) -> TcpStream {
    let mut last_error = None;

    for _ in 0..20 {
        match TcpStream::connect(addr).await {
            Ok(stream) => return stream,
            Err(err) => {
                last_error = Some(err);
                sleep(Duration::from_millis(10)).await;
            }
        }
    }

    panic!("failed to connect to test server: {}", last_error.unwrap());
}
