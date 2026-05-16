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

#[cfg(test)]
mod tls_test_support {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use rcgen::{
        BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose,
        IsCa, Issuer, KeyPair, KeyUsagePurpose,
    };

    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

    pub struct TlsArtifacts {
        pub server_cert: PathBuf,
        pub server_key: PathBuf,
        pub client_ca: PathBuf,
    }

    pub fn write_tls_artifacts() -> TlsArtifacts {
        let tmp = std::env::temp_dir().join(format!(
            "cloudiful-server-actix-tls-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&tmp).unwrap();

        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.distinguished_name = dn("cloudiful actix ca");
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca_key = KeyPair::generate().unwrap();
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();
        let ca_issuer = Issuer::new(ca_params.clone(), ca_key);

        let mut server_params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        server_params.distinguished_name = dn("cloudiful actix server");
        server_params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_key = KeyPair::generate().unwrap();
        let server_cert = server_params.signed_by(&server_key, &ca_issuer).unwrap();

        let server_cert_path = tmp.join("server.crt");
        let server_key_path = tmp.join("server.key");
        let client_ca_path = tmp.join("client-ca.crt");
        fs::write(&server_cert_path, server_cert.pem()).unwrap();
        fs::write(&server_key_path, server_key.serialize_pem()).unwrap();
        fs::write(&client_ca_path, ca_cert.pem()).unwrap();

        TlsArtifacts {
            server_cert: server_cert_path,
            server_key: server_key_path,
            client_ca: client_ca_path,
        }
    }

    fn dn(common_name: &str) -> DistinguishedName {
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, common_name);
        dn
    }
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

#[actix_web::test]
async fn tls_client_ca_is_loaded_for_actix_binding() {
    let artifacts = tls_test_support::write_tls_artifacts();
    let config = ServerConfig::new()
        .with_listen_addr("127.0.0.1:0")
        .with_tls(
            TlsConfig::new()
                .with_cert_path(&artifacts.server_cert)
                .with_cert_key_path(&artifacts.server_key)
                .with_client_ca(&artifacts.client_ca),
        )
        .build()
        .unwrap();

    let bound = Server::new(config, |_| {}).bind().unwrap();

    assert!(bound.addrs().iter().all(|addr| addr.ip().is_loopback()));
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
