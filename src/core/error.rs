use std::{fmt, path::PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerConfigError {
    MissingListenAddr,
    MissingCorsOrigins,
    MissingCorsMethods,
    InvalidCorsOrigin(String),
    InvalidCorsMethod(String),
    MissingTlsCertPath,
    MissingTlsKeyPath,
}

impl fmt::Display for ServerConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingListenAddr => write!(f, "listen_addr must not be empty"),
            Self::MissingCorsOrigins => {
                write!(
                    f,
                    "restricted CORS configuration requires at least one origin"
                )
            }
            Self::MissingCorsMethods => {
                write!(
                    f,
                    "restricted CORS configuration requires at least one method"
                )
            }
            Self::InvalidCorsOrigin(origin) => {
                write!(f, "restricted CORS origin is invalid: {origin}")
            }
            Self::InvalidCorsMethod(method) => {
                write!(f, "restricted CORS method is invalid: {method}")
            }
            Self::MissingTlsCertPath => write!(f, "TLS configuration requires cert_path"),
            Self::MissingTlsKeyPath => write!(f, "TLS configuration requires cert_key_path"),
        }
    }
}

impl std::error::Error for ServerConfigError {}

#[derive(Debug)]
pub enum TlsConfigLoadError {
    OpenCertificate {
        path: PathBuf,
        source: std::io::Error,
    },
    ReadCertificates {
        path: PathBuf,
        source: std::io::Error,
    },
    OpenPrivateKey {
        path: PathBuf,
        source: std::io::Error,
    },
    ReadPrivateKey {
        path: PathBuf,
        source: std::io::Error,
    },
    MissingPrivateKey {
        path: PathBuf,
    },
    InvalidConfig {
        source: rustls::Error,
    },
}

impl fmt::Display for TlsConfigLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenCertificate { path, source } => {
                write!(
                    f,
                    "failed to open TLS certificate at {}: {source}",
                    path.display()
                )
            }
            Self::ReadCertificates { path, source } => {
                write!(
                    f,
                    "failed to read TLS certificates from {}: {source}",
                    path.display()
                )
            }
            Self::OpenPrivateKey { path, source } => {
                write!(
                    f,
                    "failed to open TLS private key at {}: {source}",
                    path.display()
                )
            }
            Self::ReadPrivateKey { path, source } => {
                write!(
                    f,
                    "failed to read TLS private key from {}: {source}",
                    path.display()
                )
            }
            Self::MissingPrivateKey { path } => {
                write!(f, "no TLS private key found in {}", path.display())
            }
            Self::InvalidConfig { source } => {
                write!(f, "failed to build TLS server configuration: {source}")
            }
        }
    }
}

impl std::error::Error for TlsConfigLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OpenCertificate { source, .. }
            | Self::ReadCertificates { source, .. }
            | Self::OpenPrivateKey { source, .. }
            | Self::ReadPrivateKey { source, .. } => Some(source),
            Self::InvalidConfig { source } => Some(source),
            Self::MissingPrivateKey { .. } => None,
        }
    }
}

#[derive(Debug)]
pub enum ServerError {
    Config(ServerConfigError),
    Io(std::io::Error),
    Tls(TlsConfigLoadError),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => err.fmt(f),
            Self::Io(err) => err.fmt(f),
            Self::Tls(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for ServerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(err) => Some(err),
            Self::Io(err) => Some(err),
            Self::Tls(err) => Some(err),
        }
    }
}

impl From<ServerConfigError> for ServerError {
    fn from(value: ServerConfigError) -> Self {
        Self::Config(value)
    }
}

impl From<std::io::Error> for ServerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
