use axum::{
    body::HttpBody,
    extract::Request,
    http::{header, HeaderName, Method, StatusCode, Uri},
    middleware::{self, Next},
    response::Response,
    Router,
};
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::{self, CorsLayer},
    trace::TraceLayer,
    validate_request::ValidateRequestHeaderLayer,
};

use crate::ProverState;

mod v1;

pub fn create_router(concurrency_limit: usize, jwt_secret: Option<&str>) -> Router<ProverState> {
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

    let router = Router::new()
        .nest("/v1", v1_api.clone())
        .merge(v1_api)
        .layer(middleware)
        .layer(middleware::from_fn(check_max_body_size))
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
    v1::create_docs()
}

async fn check_max_body_size(req: Request, next: Next) -> Response {
    const MAX_BODY_SIZE: u64 = 1 << 20;
    let response_content_length = match req.body().size_hint().upper() {
        Some(v) => v,
        None => MAX_BODY_SIZE + 1,
    };

    if response_content_length > MAX_BODY_SIZE {
        let mut resp = Response::new(axum::body::Body::from("request too large"));
        *resp.status_mut() = StatusCode::BAD_REQUEST;
        return resp;
    }

    next.run(req).await
}
