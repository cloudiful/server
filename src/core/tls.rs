use std::{fs::File, io::BufReader, sync::Arc};

use crate::core::{
    ValidatedServerConfig,
    error::{ServerError, TlsConfigLoadError},
};

pub fn load_tls_config<U>(
    config: &ValidatedServerConfig<U>,
) -> Result<Option<rustls::ServerConfig>, ServerError> {
    let Some(tls) = config.tls.as_ref() else {
        return Ok(None);
    };

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let mut cert_file = BufReader::new(File::open(&tls.cert_path).map_err(|source| {
        ServerError::Tls(TlsConfigLoadError::OpenCertificate {
            path: tls.cert_path.clone(),
            source,
        })
    })?);

    let mut key_file = BufReader::new(File::open(&tls.cert_key_path).map_err(|source| {
        ServerError::Tls(TlsConfigLoadError::OpenPrivateKey {
            path: tls.cert_key_path.clone(),
            source,
        })
    })?);

    let tls_certs = rustls_pemfile::certs(&mut cert_file)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| {
            ServerError::Tls(TlsConfigLoadError::ReadCertificates {
                path: tls.cert_path.clone(),
                source,
            })
        })?;

    let tls_key = rustls_pemfile::private_key(&mut key_file)
        .map_err(|source| {
            ServerError::Tls(TlsConfigLoadError::ReadPrivateKey {
                path: tls.cert_key_path.clone(),
                source,
            })
        })?
        .ok_or_else(|| {
            ServerError::Tls(TlsConfigLoadError::MissingPrivateKey {
                path: tls.cert_key_path.clone(),
            })
        })?;

    let builder = rustls::ServerConfig::builder();
    let builder = match tls.client_ca.as_ref() {
        Some(client_ca_path) => {
            let mut client_ca_file =
                BufReader::new(File::open(client_ca_path).map_err(|source| {
                    ServerError::Tls(TlsConfigLoadError::OpenClientCa {
                        path: client_ca_path.clone(),
                        source,
                    })
                })?);

            let client_ca_certs = rustls_pemfile::certs(&mut client_ca_file)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|source| {
                    ServerError::Tls(TlsConfigLoadError::ReadClientCa {
                        path: client_ca_path.clone(),
                        source,
                    })
                })?;

            let mut client_roots = rustls::RootCertStore::empty();
            let (added, _ignored) = client_roots.add_parsable_certificates(client_ca_certs);
            if added == 0 {
                return Err(ServerError::Tls(TlsConfigLoadError::InvalidClientCa {
                    path: client_ca_path.clone(),
                    source: rustls::Error::General("no valid client CA certificates found".into()),
                }));
            }

            let client_verifier =
                rustls::server::WebPkiClientVerifier::builder(Arc::new(client_roots))
                    .build()
                    .map_err(|source| {
                        ServerError::Tls(TlsConfigLoadError::InvalidClientCa {
                            path: client_ca_path.clone(),
                            source: rustls::Error::General(source.to_string().into()),
                        })
                    })?;

            builder.with_client_cert_verifier(client_verifier)
        }
        None => builder.with_no_client_auth(),
    };

    builder
        .with_single_cert(tls_certs, tls_key)
        .map(Some)
        .map_err(|source| ServerError::Tls(TlsConfigLoadError::InvalidConfig { source }))
}
