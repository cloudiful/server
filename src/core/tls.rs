use std::{fs::File, io::BufReader};

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

    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(tls_certs, tls_key)
        .map(Some)
        .map_err(|source| ServerError::Tls(TlsConfigLoadError::InvalidConfig { source }))
}
