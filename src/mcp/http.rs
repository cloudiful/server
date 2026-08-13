use std::{
    future::Future,
    net::{SocketAddr, TcpListener},
};

use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use log::info;
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};

use crate::{ServerError, ValidatedServerConfig, load_tls_config};

use super::{McpServerError, ServerConfig, ServerHandler};

pub fn service<F, S>(
    runtime_config: ServerConfig,
    service_factory: F,
) -> Result<StreamableHttpService<S, LocalSessionManager>, McpServerError>
where
    F: Fn() -> S + Send + Sync + 'static,
    S: ServerHandler + 'static,
{
    runtime_config.service_path()?;
    Ok(build_service(runtime_config, service_factory))
}

pub fn router<F, S>(
    runtime_config: ServerConfig,
    service_factory: F,
) -> Result<Router, McpServerError>
where
    F: Fn() -> S + Send + Sync + 'static,
    S: ServerHandler + 'static,
{
    let service_path = runtime_config.service_path()?;
    let service = build_service(runtime_config, service_factory);
    if service_path == "/" {
        Ok(Router::new().fallback_service(service))
    } else {
        Ok(Router::new().nest_service(&service_path, service))
    }
}

fn build_service<F, S>(
    runtime_config: ServerConfig,
    service_factory: F,
) -> StreamableHttpService<S, LocalSessionManager>
where
    F: Fn() -> S + Send + Sync + 'static,
    S: ServerHandler + 'static,
{
    let session_manager = match runtime_config.event_store() {
        Some(event_store) => LocalSessionManager::default().with_event_store(event_store),
        None => LocalSessionManager::default(),
    };

    StreamableHttpService::new(
        move || Ok::<_, std::io::Error>(service_factory()),
        session_manager.into(),
        runtime_config.to_rmcp_config(),
    )
}

pub struct Server<F, S>
where
    F: Fn() -> S + Send + Sync + 'static,
    S: ServerHandler + 'static,
{
    config: ValidatedServerConfig<()>,
    runtime_config: ServerConfig,
    service_factory: F,
}

impl<F, S> Server<F, S>
where
    F: Fn() -> S + Send + Sync + 'static,
    S: ServerHandler + 'static,
{
    pub fn new(config: ValidatedServerConfig<()>, service_factory: F) -> Self {
        Self {
            config,
            runtime_config: ServerConfig::default(),
            service_factory,
        }
    }

    pub fn with_server_config(mut self, runtime_config: ServerConfig) -> Self {
        self.runtime_config = runtime_config;
        self
    }

    pub fn bind(self) -> Result<BoundServer, McpServerError> {
        let Server {
            config,
            runtime_config,
            service_factory,
        } = self;

        let listener = TcpListener::bind(config.listen_addr()).map_err(ServerError::from)?;
        let addrs = vec![listener.local_addr().map_err(ServerError::from)?];
        let uses_tls = config.tls_enabled();
        let app = router(runtime_config, service_factory)?;

        let tls_config = match load_tls_config(&config)? {
            Some(config) => Some(RustlsConfig::from_config(std::sync::Arc::new(config))),
            None => None,
        };

        let scheme = if uses_tls { "https" } else { "http" };
        info!("starting {scheme} MCP server on {:?}", addrs);

        Ok(BoundServer {
            addrs,
            listener,
            app,
            tls_config,
        })
    }

    pub async fn start(self) -> Result<(), McpServerError> {
        self.bind()?.run().await
    }
}

#[derive(Debug)]
pub struct BoundServer {
    addrs: Vec<SocketAddr>,
    listener: TcpListener,
    app: Router,
    tls_config: Option<RustlsConfig>,
}

impl BoundServer {
    pub fn addrs(&self) -> &[SocketAddr] {
        self.addrs.as_slice()
    }

    pub async fn run(self) -> Result<(), McpServerError> {
        if let Some(tls_config) = self.tls_config {
            axum_server::from_tcp_rustls(self.listener, tls_config)
                .map_err(ServerError::from)?
                .serve(self.app.into_make_service())
                .await
                .map_err(ServerError::from)?;
            Ok(())
        } else {
            self.listener
                .set_nonblocking(true)
                .map_err(ServerError::from)?;

            let listener =
                tokio::net::TcpListener::from_std(self.listener).map_err(ServerError::from)?;

            axum::serve(listener, self.app)
                .await
                .map_err(ServerError::from)?;
            Ok(())
        }
    }

    pub async fn run_with_graceful_shutdown<F>(self, signal: F) -> Result<(), McpServerError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if let Some(tls_config) = self.tls_config {
            let handle = axum_server::Handle::new();
            let shutdown_handle = handle.clone();

            tokio::spawn(async move {
                signal.await;
                shutdown_handle.graceful_shutdown(None);
            });

            axum_server::from_tcp_rustls(self.listener, tls_config)
                .map_err(ServerError::from)?
                .handle(handle)
                .serve(self.app.into_make_service())
                .await
                .map_err(ServerError::from)?;
            Ok(())
        } else {
            self.listener
                .set_nonblocking(true)
                .map_err(ServerError::from)?;

            let listener =
                tokio::net::TcpListener::from_std(self.listener).map_err(ServerError::from)?;

            axum::serve(listener, self.app)
                .with_graceful_shutdown(signal)
                .await
                .map_err(ServerError::from)?;
            Ok(())
        }
    }
}
