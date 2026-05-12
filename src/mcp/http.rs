use std::{
    future::Future,
    net::{SocketAddr, TcpListener},
    time::Duration,
};

use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use log::info;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;

use crate::{ServerError, ValidatedServerConfig, load_tls_config};

use super::{McpServerError, ServerHandler};

#[derive(Clone, Debug)]
pub struct ServerConfig {
    service_path: String,
    stateful_mode: bool,
    json_response: bool,
    sse_keep_alive: Option<Duration>,
    sse_retry: Option<Duration>,
    allowed_hosts: Vec<String>,
    allowed_origins: Vec<String>,
    cancellation_token: Option<CancellationToken>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            service_path: "/mcp".to_string(),
            stateful_mode: true,
            json_response: false,
            sse_keep_alive: None,
            sse_retry: None,
            allowed_hosts: Vec::new(),
            allowed_origins: Vec::new(),
            cancellation_token: None,
        }
    }
}

impl ServerConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_service_path(mut self, service_path: impl Into<String>) -> Self {
        self.service_path = service_path.into();
        self
    }

    pub fn with_stateful_mode(mut self, stateful_mode: bool) -> Self {
        self.stateful_mode = stateful_mode;
        self
    }

    pub fn with_json_response(mut self, json_response: bool) -> Self {
        self.json_response = json_response;
        self
    }

    pub fn with_sse_keep_alive(mut self, sse_keep_alive: Duration) -> Self {
        self.sse_keep_alive = Some(sse_keep_alive);
        self
    }

    pub fn with_sse_retry(mut self, sse_retry: Duration) -> Self {
        self.sse_retry = Some(sse_retry);
        self
    }

    pub fn with_allowed_hosts<I, S>(mut self, allowed_hosts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_hosts = allowed_hosts.into_iter().map(Into::into).collect();
        self
    }

    pub fn disable_allowed_hosts(mut self) -> Self {
        self.allowed_hosts.clear();
        self
    }

    pub fn with_allowed_origins<I, S>(mut self, allowed_origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_origins = allowed_origins.into_iter().map(Into::into).collect();
        self
    }

    pub fn disable_allowed_origins(mut self) -> Self {
        self.allowed_origins.clear();
        self
    }

    pub fn with_cancellation_token(mut self, cancellation_token: CancellationToken) -> Self {
        self.cancellation_token = Some(cancellation_token);
        self
    }

    fn service_path(&self) -> Result<String, McpServerError> {
        let path = self.service_path.trim();
        if path.is_empty() {
            return Err(McpServerError::invalid_service_path(
                self.service_path.clone(),
            ));
        }

        let mut normalized = path.trim_end_matches('/').to_string();
        if normalized.is_empty() {
            normalized.push('/');
        }
        if !normalized.starts_with('/') {
            normalized.insert(0, '/');
        }

        Ok(normalized)
    }

    fn to_rmcp_config(&self) -> StreamableHttpServerConfig {
        let mut config = StreamableHttpServerConfig::default()
            .with_sse_keep_alive(self.sse_keep_alive)
            .with_sse_retry(self.sse_retry)
            .with_stateful_mode(self.stateful_mode)
            .with_json_response(self.json_response)
            .with_allowed_hosts(self.allowed_hosts.clone())
            .with_allowed_origins(self.allowed_origins.clone());

        if self.allowed_hosts.is_empty() {
            config = config.disable_allowed_hosts();
        }
        if self.allowed_origins.is_empty() {
            config = config.disable_allowed_origins();
        }
        if let Some(cancellation_token) = self.cancellation_token.clone() {
            config = config.with_cancellation_token(cancellation_token);
        }

        config
    }
}

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
    StreamableHttpService::new(
        move || Ok::<_, std::io::Error>(service_factory()),
        LocalSessionManager::default().into(),
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
