use ::axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use log::info;
use std::{
    future::Future,
    net::{SocketAddr, TcpListener},
    sync::Arc,
    time::Duration,
};
use tower_http::cors::{Any, CorsLayer};

use crate::{CorsConfig, ServerError, ValidatedServerConfig, load_tls_config};

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

    pub async fn run(self) -> Result<(), ServerError> {
        if let Some(tls_config) = self.tls_config {
            axum_server::from_tcp_rustls(self.listener, tls_config)
                .serve(self.app.into_make_service())
                .await
                .map_err(ServerError::from)
        } else {
            self.listener
                .set_nonblocking(true)
                .map_err(ServerError::from)?;

            let listener =
                tokio::net::TcpListener::from_std(self.listener).map_err(ServerError::from)?;

            ::axum::serve(listener, self.app)
                .await
                .map_err(ServerError::from)
        }
    }

    pub async fn run_with_graceful_shutdown<F>(self, signal: F) -> Result<(), ServerError>
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
                .handle(handle)
                .serve(self.app.into_make_service())
                .await
                .map_err(ServerError::from)
        } else {
            self.listener
                .set_nonblocking(true)
                .map_err(ServerError::from)?;

            let listener =
                tokio::net::TcpListener::from_std(self.listener).map_err(ServerError::from)?;

            ::axum::serve(listener, self.app)
                .with_graceful_shutdown(signal)
                .await
                .map_err(ServerError::from)
        }
    }
}

pub struct Server<U = ()> {
    config: ValidatedServerConfig<U>,
    app: Router,
}

impl<U> Server<U> {
    #[cfg(test)]
    pub(crate) fn into_router(self) -> Router {
        self.app
    }
}

impl Server<()> {
    pub fn new(config: ValidatedServerConfig<()>, app: Router) -> Self {
        let app = app.layer(build_cors_layer(config.cors()));
        Self { config, app }
    }
}

impl<U> Server<U>
where
    U: Clone + Send + Sync + 'static,
{
    pub fn new_with_state(config: ValidatedServerConfig<U>, app: Router<U>) -> Self {
        let app = app.with_state(
            config
                .app_data()
                .cloned()
                .expect("validated stateful config must contain app_data"),
        );
        let app = app.layer(build_cors_layer(config.cors()));

        Self { config, app }
    }

    pub fn bind(self) -> Result<BoundServer, ServerError> {
        let listener = TcpListener::bind(self.config.listen_addr()).map_err(ServerError::from)?;
        let addrs = vec![listener.local_addr().map_err(ServerError::from)?];
        let uses_tls = self.config.tls_enabled();

        let tls_config = match load_tls_config(&self.config)? {
            Some(config) => Some(RustlsConfig::from_config(Arc::new(config))),
            None => None,
        };

        let scheme = if uses_tls { "https" } else { "http" };
        info!("starting {scheme} server on {:?}", addrs);

        Ok(BoundServer {
            addrs,
            listener,
            app: self.app,
            tls_config,
        })
    }

    pub async fn start(self) -> Result<(), ServerError> {
        self.bind()?.run().await
    }
}

fn build_cors_layer(config: &CorsConfig) -> CorsLayer {
    if config.is_permissive() {
        return CorsLayer::permissive();
    }

    let methods = config
        .allowed_methods()
        .iter()
        .map(|method| method.parse().expect("validated CORS method"))
        .collect::<Vec<::axum::http::Method>>();
    let origins = config
        .allowed_origins()
        .iter()
        .map(|origin| origin.parse().expect("validated CORS origin"))
        .collect::<Vec<::axum::http::HeaderValue>>();

    let mut layer = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(methods)
        .allow_headers(Any);

    if let Some(max_age) = config.max_age() {
        layer = layer.max_age(Duration::from_secs(max_age as u64));
    }

    layer
}
