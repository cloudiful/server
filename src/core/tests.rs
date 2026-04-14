use crate::{
    CorsConfig, ServerConfig, ServerConfigError, ServerError, TlsConfig, TlsConfigLoadError,
    load_tls_config,
};

#[test]
fn requires_tls_key_path_when_tls_is_enabled() {
    let err = ServerConfig::new()
        .with_tls(TlsConfig::new().with_cert_path("cert.pem"))
        .build()
        .unwrap_err();

    assert_eq!(err, ServerConfigError::MissingTlsKeyPath);
}

#[test]
fn allows_http_configuration_without_tls() {
    let config = ServerConfig::new()
        .with_listen_addr("127.0.0.1:8081")
        .build()
        .unwrap();

    assert_eq!(config.listen_addr(), "127.0.0.1:8081");
    assert!(!config.tls_enabled());
}

#[test]
fn restricted_cors_requires_valid_methods() {
    let err = ServerConfig::new()
        .with_cors(
            CorsConfig::restricted(["https://allowed.example"])
                .with_allowed_methods(["BAD METHOD"]),
        )
        .build()
        .unwrap_err();

    assert_eq!(
        err,
        ServerConfigError::InvalidCorsMethod("BAD METHOD".to_string())
    );
}

#[test]
fn invalid_tls_paths_return_errors_without_panicking() {
    let config = ServerConfig::new()
        .with_tls(
            TlsConfig::new()
                .with_cert_path("missing-cert.pem")
                .with_cert_key_path("missing-key.pem"),
        )
        .build()
        .unwrap();

    let err = load_tls_config(&config).unwrap_err();

    match err {
        ServerError::Tls(TlsConfigLoadError::OpenCertificate { path, .. }) => {
            assert_eq!(path, std::path::PathBuf::from("missing-cert.pem"));
        }
        other => panic!("unexpected error: {other}"),
    }
}
