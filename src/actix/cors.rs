use actix_cors::Cors;
use actix_web::http::Method;

use crate::CorsConfig;

pub(crate) fn build_cors(config: &CorsConfig) -> Cors {
    if config.is_permissive() {
        return Cors::permissive();
    }

    let mut cors = Cors::default();

    for origin in config.allowed_origins() {
        cors = cors.allowed_origin(origin);
    }

    cors = cors.allow_any_header();

    let methods = config
        .allowed_methods()
        .iter()
        .map(|method| Method::from_bytes(method.as_bytes()).expect("validated CORS method"))
        .collect::<Vec<_>>();

    if !methods.is_empty() {
        cors = cors.allowed_methods(methods);
    }

    if let Some(max_age) = config.max_age() {
        cors = cors.max_age(max_age);
    }

    cors
}
