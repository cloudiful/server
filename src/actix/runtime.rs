use actix_web::{
    App, Error, HttpServer,
    dev::{Server as ActixServer, ServerHandle, ServiceFactory, ServiceRequest},
    web,
};
use log::info;
use std::net::SocketAddr;

use crate::{ServerError, ValidatedServerConfig, load_tls_config};

use super::cors::build_cors;

pub struct BoundServer {
    addrs: Vec<SocketAddr>,
    server: ActixServer,
}

impl BoundServer {
    pub fn addrs(&self) -> &[SocketAddr] {
        self.addrs.as_slice()
    }

    pub fn handle(&self) -> ServerHandle {
        self.server.handle()
    }

    pub async fn run(self) -> Result<(), ServerError> {
        self.server.await.map_err(ServerError::from)
    }
}

pub struct Server<F, U = ()>
where
    F: Fn(&mut web::ServiceConfig) + Clone + Send + 'static,
    U: Clone + Send + 'static,
{
    services: F,
    config: ValidatedServerConfig<U>,
}

impl<F, U> Server<F, U>
where
    F: Fn(&mut web::ServiceConfig) + Clone + Send + 'static,
    U: Clone + Send + 'static,
{
    pub fn new(config: ValidatedServerConfig<U>, services: F) -> Self {
        Self { services, config }
    }

    pub fn bind(self) -> Result<BoundServer, ServerError> {
        let Server { services, config } = self;
        let listen_addr = config.listen_addr().to_string();
        let uses_tls = config.tls_enabled();
        let cors_config = config.cors().clone();
        let app_data = config.app_data().cloned();

        let http_server = HttpServer::new(move || {
            let app = App::new().wrap(build_cors(&cors_config));
            build_app(app, &services, &app_data)
        });

        let http_server = match load_tls_config(&config)? {
            Some(tls_config) => http_server.bind_rustls_0_23(listen_addr.as_str(), tls_config)?,
            None => http_server.bind(listen_addr.as_str())?,
        };

        let addrs = http_server.addrs();
        let scheme = if uses_tls { "https" } else { "http" };
        info!("starting {scheme} server on {:?}", addrs);

        Ok(BoundServer {
            addrs,
            server: http_server.run(),
        })
    }

    pub async fn start(self) -> Result<(), ServerError> {
        self.bind()?.run().await
    }
}

fn build_app<T, F, U>(app: App<T>, services: &F, app_data: &Option<U>) -> App<T>
where
    T: ServiceFactory<ServiceRequest, Config = (), Error = Error, InitError = ()>,
    F: Fn(&mut web::ServiceConfig) + Clone + Send + 'static,
    U: Clone + Send + 'static,
{
    let app = match app_data.clone() {
        Some(app_data) => app.app_data(app_data),
        None => app,
    };

    app.configure(services.clone())
}
