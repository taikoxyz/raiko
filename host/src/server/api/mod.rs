use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderName, Method, StatusCode, Uri},
    Router,
};
use raiko_reqactor::Actor;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::{self, CorsLayer},
    trace::TraceLayer,
    validate_request::ValidateRequestHeaderLayer,
};

pub mod admin;
pub mod v1;
pub mod v2;
pub mod v3;

pub const MAX_BODY_SIZE: usize = 1 << 20;

pub fn create_router(concurrency_limit: usize, jwt_secret: Option<&str>) -> Router<Actor> {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            header::ORIGIN,
            header::ORIGIN,
            header::ACCEPT,
            HeaderName::from_static("x-requested-with"),
        ])
        .allow_origin(cors::Any);
    let compression = CompressionLayer::new();

    let middleware = ServiceBuilder::new().layer(cors).layer(compression);

    let trace = TraceLayer::new_for_http();

    let v1_api = v1::create_router(concurrency_limit);
    let v2_api = v2::create_router();
    let v3_api = v3::create_router();
    let admin_api = admin::create_router();
    let router = Router::new()
        .nest("/v1", v1_api)
        .nest("/v2", v2_api)
        .nest("/v3", v3_api.clone())
        .merge(v3_api)
        .nest("/admin", admin_api)
        .layer(middleware)
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .layer(trace)
        .fallback(|uri: Uri| async move {
            (StatusCode::NOT_FOUND, format!("No handler found for {uri}"))
        });

    if let Some(jwt_secret) = jwt_secret {
        let auth = ValidateRequestHeaderLayer::bearer(jwt_secret);
        router.layer(auth)
    } else {
        router
    }
}

pub fn create_docs() -> utoipa::openapi::OpenApi {
    v3::create_docs()
}
