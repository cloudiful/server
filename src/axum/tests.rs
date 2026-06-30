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
            "cloudiful-server-axum-tls-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&tmp).unwrap();

        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.distinguished_name = dn("cloudiful axum ca");
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca_key = KeyPair::generate().unwrap();
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();
        let ca_issuer = Issuer::new(ca_params.clone(), ca_key);

        let mut server_params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        server_params.distinguished_name = dn("cloudiful axum server");
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

#[tokio::test]
async fn tls_client_ca_is_loaded_for_axum_binding() {
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

    let bound = Server::new(
        config,
        Router::new().route("/health", get(|| async { "ok" })),
    )
    .bind()
    .unwrap();

    assert!(bound.addrs().iter().all(|addr| addr.ip().is_loopback()));
}
