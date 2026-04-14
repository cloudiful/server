mod core;

#[cfg(feature = "actix")]
pub mod actix;
#[cfg(feature = "axum")]
pub mod axum;

pub use core::{
    CorsConfig, DEFAULT_LISTEN_ADDR, ServerConfig, ServerConfigError, ServerError, TlsConfig,
    TlsConfigLoadError, ValidatedServerConfig, load_tls_config,
};

#[cfg(feature = "actix")]
pub use actix::{BoundServer, Server};
