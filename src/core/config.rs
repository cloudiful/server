use http::{HeaderValue, Method};
use std::path::{Path, PathBuf};

use crate::core::error::ServerConfigError;

pub const DEFAULT_LISTEN_ADDR: &str = "127.0.0.1:3000";

#[derive(Clone, Debug, Eq, PartialEq)]
enum CorsMode {
    Permissive,
    Restricted { allowed_origins: Vec<String> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorsConfig {
    mode: CorsMode,
    allowed_methods: Vec<String>,
    max_age: Option<usize>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self::permissive()
    }
}

impl CorsConfig {
    pub fn permissive() -> Self {
        Self {
            mode: CorsMode::Permissive,
            allowed_methods: Vec::new(),
            max_age: None,
        }
    }

    pub fn restricted<I, S>(allowed_origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            mode: CorsMode::Restricted {
                allowed_origins: allowed_origins.into_iter().map(Into::into).collect(),
            },
            allowed_methods: vec!["GET".to_string(), "POST".to_string()],
            max_age: Some(3600),
        }
    }

    pub fn with_allowed_methods<I, S>(mut self, allowed_methods: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_methods = allowed_methods.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_max_age(mut self, max_age: usize) -> Self {
        self.max_age = Some(max_age);
        self
    }

    pub fn is_permissive(&self) -> bool {
        matches!(self.mode, CorsMode::Permissive)
    }

    pub fn allowed_origins(&self) -> &[String] {
        match &self.mode {
            CorsMode::Permissive => &[],
            CorsMode::Restricted { allowed_origins } => allowed_origins.as_slice(),
        }
    }

    pub fn allowed_methods(&self) -> &[String] {
        self.allowed_methods.as_slice()
    }

    pub fn max_age(&self) -> Option<usize> {
        self.max_age
    }

    pub(crate) fn validate(&self) -> Result<(), ServerConfigError> {
        match &self.mode {
            CorsMode::Permissive => Ok(()),
            CorsMode::Restricted { allowed_origins } => {
                if allowed_origins.is_empty() {
                    return Err(ServerConfigError::MissingCorsOrigins);
                }

                for origin in allowed_origins {
                    HeaderValue::from_str(origin)
                        .map_err(|_| ServerConfigError::InvalidCorsOrigin(origin.clone()))?;
                }

                if self.allowed_methods.is_empty() {
                    return Err(ServerConfigError::MissingCorsMethods);
                }

                for method in &self.allowed_methods {
                    Method::from_bytes(method.as_bytes())
                        .map_err(|_| ServerConfigError::InvalidCorsMethod(method.clone()))?;
                }

                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TlsConfig {
    cert_path: Option<PathBuf>,
    cert_key_path: Option<PathBuf>,
    client_ca: Option<PathBuf>,
}

impl TlsConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_cert_path<P>(mut self, cert_path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.cert_path = Some(cert_path.into());
        self
    }

    pub fn with_cert_key_path<P>(mut self, cert_key_path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.cert_key_path = Some(cert_key_path.into());
        self
    }

    pub fn with_client_ca<P>(mut self, client_ca: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.client_ca = Some(client_ca.into());
        self
    }

    pub fn with_client_ca_path<P>(self, client_ca_path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.with_client_ca(client_ca_path)
    }

    pub fn cert_path(&self) -> Option<&Path> {
        self.cert_path.as_deref()
    }

    pub fn cert_key_path(&self) -> Option<&Path> {
        self.cert_key_path.as_deref()
    }

    pub fn client_ca(&self) -> Option<&Path> {
        self.client_ca.as_deref()
    }

    pub fn client_ca_path(&self) -> Option<&Path> {
        self.client_ca()
    }

    pub(crate) fn validate(self) -> Result<ValidatedTlsConfig, ServerConfigError> {
        let cert_path = self
            .cert_path
            .ok_or(ServerConfigError::MissingTlsCertPath)?;
        let cert_key_path = self
            .cert_key_path
            .ok_or(ServerConfigError::MissingTlsKeyPath)?;

        Ok(ValidatedTlsConfig {
            cert_path,
            cert_key_path,
            client_ca: self.client_ca,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidatedTlsConfig {
    pub(crate) cert_path: PathBuf,
    pub(crate) cert_key_path: PathBuf,
    pub(crate) client_ca: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct ServerConfig<U = ()> {
    listen_addr: String,
    app_data: Option<U>,
    cors: CorsConfig,
    tls: Option<TlsConfig>,
}

impl Default for ServerConfig<()> {
    fn default() -> Self {
        Self {
            listen_addr: DEFAULT_LISTEN_ADDR.to_string(),
            app_data: None,
            cors: CorsConfig::default(),
            tls: None,
        }
    }
}

impl ServerConfig<()> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<U> ServerConfig<U> {
    pub fn with_listen_addr(mut self, listen_addr: impl Into<String>) -> Self {
        self.listen_addr = normalize_listen_addr(listen_addr);
        self
    }

    pub fn with_app_data<T>(self, app_data: T) -> ServerConfig<T> {
        ServerConfig {
            listen_addr: self.listen_addr,
            app_data: Some(app_data),
            cors: self.cors,
            tls: self.tls,
        }
    }

    pub fn with_cors(mut self, cors: CorsConfig) -> Self {
        self.cors = cors;
        self
    }

    pub fn with_tls(mut self, tls: TlsConfig) -> Self {
        self.tls = Some(tls);
        self
    }

    pub fn build(self) -> Result<ValidatedServerConfig<U>, ServerConfigError> {
        if self.listen_addr.trim().is_empty() {
            return Err(ServerConfigError::MissingListenAddr);
        }

        self.cors.validate()?;
        let tls = self.tls.map(TlsConfig::validate).transpose()?;

        Ok(ValidatedServerConfig {
            listen_addr: self.listen_addr,
            app_data: self.app_data,
            cors: self.cors,
            tls,
        })
    }
}

pub fn normalize_listen_addr(listen_addr: impl Into<String>) -> String {
    let listen_addr = listen_addr.into();
    let trimmed = listen_addr.trim();
    if trimmed.starts_with(':') {
        format!("0.0.0.0{trimmed}")
    } else {
        trimmed.to_string()
    }
}

#[derive(Clone, Debug)]
pub struct ValidatedServerConfig<U = ()> {
    pub(crate) listen_addr: String,
    pub(crate) app_data: Option<U>,
    pub(crate) cors: CorsConfig,
    pub(crate) tls: Option<ValidatedTlsConfig>,
}

impl<U> ValidatedServerConfig<U> {
    pub fn listen_addr(&self) -> &str {
        self.listen_addr.as_str()
    }

    pub fn app_data(&self) -> Option<&U> {
        self.app_data.as_ref()
    }

    pub fn cors(&self) -> &CorsConfig {
        &self.cors
    }

    pub fn tls_enabled(&self) -> bool {
        self.tls.is_some()
    }

    pub fn tls_paths(&self) -> Option<(&Path, &Path)> {
        self.tls
            .as_ref()
            .map(|tls| (tls.cert_path.as_path(), tls.cert_key_path.as_path()))
    }
}
