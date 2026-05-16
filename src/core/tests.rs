use crate::{
    CorsConfig, ServerConfig, ServerConfigError, ServerError, TlsConfig, TlsConfigLoadError,
    load_tls_config, normalize_listen_addr,
};
use rustls::pki_types::ServerName;

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
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

    pub struct TlsArtifacts {
        pub server_cert: PathBuf,
        pub server_key: PathBuf,
        pub client_ca: PathBuf,
        pub client_key: PrivateKeyDer<'static>,
        pub client_cert: CertificateDer<'static>,
    }

    pub fn write_tls_artifacts() -> TlsArtifacts {
        let tmp = std::env::temp_dir().join(format!(
            "cloudiful-server-tls-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&tmp).unwrap();

        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.distinguished_name = dn("cloudiful test ca");
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca_key = KeyPair::generate().unwrap();
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();
        let ca_issuer = Issuer::new(ca_params.clone(), ca_key);

        let mut server_params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        server_params.distinguished_name = dn("cloudiful server");
        server_params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_key = KeyPair::generate().unwrap();
        let server_cert = server_params.signed_by(&server_key, &ca_issuer).unwrap();

        let mut client_params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        client_params.distinguished_name = dn("cloudiful client");
        client_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let client_key = KeyPair::generate().unwrap();
        let client_cert = client_params.signed_by(&client_key, &ca_issuer).unwrap();

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
            client_key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(client_key.serialize_der())),
            client_cert: CertificateDer::from(client_cert.der().to_vec()),
        }
    }

    fn dn(common_name: &str) -> DistinguishedName {
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, common_name);
        dn
    }
}

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
fn listen_addr_with_only_port_uses_wildcard_host() {
    let config = ServerConfig::new()
        .with_listen_addr(":8081")
        .build()
        .unwrap();

    assert_eq!(config.listen_addr(), "0.0.0.0:8081");
}

#[test]
fn normalize_listen_addr_preserves_explicit_host() {
    assert_eq!(normalize_listen_addr("127.0.0.1:8081"), "127.0.0.1:8081");
    assert_eq!(normalize_listen_addr(" 0.0.0.0:8081 "), "0.0.0.0:8081");
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

#[test]
fn tls_config_can_include_client_ca_path() {
    let tls = TlsConfig::new()
        .with_cert_path("cert.pem")
        .with_cert_key_path("key.pem")
        .with_client_ca("ca.pem");

    assert_eq!(tls.client_ca(), Some(std::path::Path::new("ca.pem")));
    assert_eq!(tls.client_ca_path(), Some(std::path::Path::new("ca.pem")));
}

#[test]
fn load_tls_config_supports_client_auth_verifier() {
    let artifacts = tls_test_support::write_tls_artifacts();
    let config = ServerConfig::new()
        .with_tls(
            TlsConfig::new()
                .with_cert_path(&artifacts.server_cert)
                .with_cert_key_path(&artifacts.server_key)
                .with_client_ca(&artifacts.client_ca),
        )
        .build()
        .unwrap();

    assert!(load_tls_config(&config).unwrap().is_some());
}

#[test]
fn server_only_tls_still_accepts_clients_without_certificates() {
    let artifacts = tls_test_support::write_tls_artifacts();
    let config = ServerConfig::new()
        .with_tls(
            TlsConfig::new()
                .with_cert_path(&artifacts.server_cert)
                .with_cert_key_path(&artifacts.server_key),
        )
        .build()
        .unwrap();

    let server_config = load_tls_config(&config).unwrap().unwrap();
    let mut roots = rustls::RootCertStore::empty();
    let mut ca_file = std::io::BufReader::new(std::fs::File::open(&artifacts.client_ca).unwrap());
    let ca_certs = rustls_pemfile::certs(&mut ca_file)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let (added, _) = roots.add_parsable_certificates(ca_certs);
    assert!(added > 0);

    let client_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    assert!(perform_tls_handshake(server_config, client_config).is_ok());
}

#[test]
fn mtls_rejects_clients_without_certificates() {
    let artifacts = tls_test_support::write_tls_artifacts();
    let config = ServerConfig::new()
        .with_tls(
            TlsConfig::new()
                .with_cert_path(&artifacts.server_cert)
                .with_cert_key_path(&artifacts.server_key)
                .with_client_ca(&artifacts.client_ca),
        )
        .build()
        .unwrap();

    let server_config = load_tls_config(&config).unwrap().unwrap();
    let mut roots = rustls::RootCertStore::empty();
    let mut ca_file = std::io::BufReader::new(std::fs::File::open(&artifacts.client_ca).unwrap());
    let ca_certs = rustls_pemfile::certs(&mut ca_file)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let (added, _) = roots.add_parsable_certificates(ca_certs);
    assert!(added > 0);

    let client_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    assert!(perform_tls_handshake(server_config, client_config).is_err());
}

#[test]
fn mtls_accepts_clients_with_trusted_certificates() {
    let artifacts = tls_test_support::write_tls_artifacts();
    let config = ServerConfig::new()
        .with_tls(
            TlsConfig::new()
                .with_cert_path(&artifacts.server_cert)
                .with_cert_key_path(&artifacts.server_key)
                .with_client_ca(&artifacts.client_ca),
        )
        .build()
        .unwrap();

    let server_config = load_tls_config(&config).unwrap().unwrap();
    let mut roots = rustls::RootCertStore::empty();
    let mut ca_file = std::io::BufReader::new(std::fs::File::open(&artifacts.client_ca).unwrap());
    let ca_certs = rustls_pemfile::certs(&mut ca_file)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let (added, _) = roots.add_parsable_certificates(ca_certs);
    assert!(added > 0);

    let client_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_client_auth_cert(vec![artifacts.client_cert], artifacts.client_key)
        .unwrap();

    assert!(perform_tls_handshake(server_config, client_config).is_ok());
}

fn perform_tls_handshake(
    server_config: rustls::ServerConfig,
    client_config: rustls::ClientConfig,
) -> Result<(), rustls::Error> {
    let server_name = ServerName::try_from("localhost").unwrap();
    let mut client = rustls::ClientConnection::new(std::sync::Arc::new(client_config), server_name)
        .unwrap();
    let mut server = rustls::ServerConnection::new(std::sync::Arc::new(server_config)).unwrap();
    let mut client_to_server = Vec::new();
    let mut server_to_client = Vec::new();

    for _ in 0..16 {
        while client.wants_write() {
            client.write_tls(&mut client_to_server).unwrap();
        }
        while server.wants_write() {
            server.write_tls(&mut server_to_client).unwrap();
        }

        if !client_to_server.is_empty() {
            let mut rd = std::io::Cursor::new(std::mem::take(&mut client_to_server));
            server.read_tls(&mut rd).unwrap();
            if let Err(err) = server.process_new_packets() {
                return Err(err);
            }
        }

        if !server_to_client.is_empty() {
            let mut rd = std::io::Cursor::new(std::mem::take(&mut server_to_client));
            client.read_tls(&mut rd).unwrap();
            if let Err(err) = client.process_new_packets() {
                return Err(err);
            }
        }

        if !client.is_handshaking() && !server.is_handshaking() {
            return Ok(());
        }
    }

    Err(rustls::Error::General("TLS handshake did not complete".to_string()))
}
