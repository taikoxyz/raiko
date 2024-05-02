use axum::{
    body::HttpBody,
    extract::Request,
    http::{header, HeaderName, HeaderValue, Method, StatusCode, Uri},
    middleware::{self, Next},
    response::Response,
    Router,
};
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::{self, CorsLayer},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::ProverState;

mod health;
mod metrics;
mod proof;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Raiko Proverd Server API",
        version = "1.0",
        description = "Raiko Proverd Server API",
        contact(
            name = "API Support",
            url = "https://community.taiko.xyz",
            email = "info@taiko.xyz",
        ),
        license(
            name = "MIT",
            url = "https://github.com/taikoxyz/raiko/blob/taiko/unstable/LICENSE"
        ),
    ),
    components(
        schemas(
            crate::request::ProofRequestOpt,
            crate::error::HostError,
        )
    ),
    tags(
        (name = "Prooving", description = "Routes that handle prooving requests"),
        (name = "Health", description = "Routes that report the server health status"),
        (name = "Metrics", description = "Routes that give detailed insight into the server")
    )
)]
/// The root API struct which is generated from the `OpenApi` derive macro.
pub struct Docs;

#[must_use]
pub fn create_docs() -> utoipa::openapi::OpenApi {
    [
        health::create_docs(),
        metrics::create_docs(),
        proof::create_docs(),
    ]
    .into_iter()
    .fold(Docs::openapi(), |mut doc, sub_doc| {
        doc.merge(sub_doc);
        doc
    })
}

pub fn create_router(concurrency_limit: usize) -> Router<ProverState> {
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

    let middleware = ServiceBuilder::new().layer(cors).layer(compression).layer(
        SetResponseHeaderLayer::overriding(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        ),
    );

    let trace = TraceLayer::new_for_http();

    Router::new()
        // Only add the concurrency limit to the proof route. We want to still be able to call
        // healthchecks and metrics to have insight into the system.
        .nest(
            "/proof",
            proof::create_router()
                .layer(ServiceBuilder::new().concurrency_limit(concurrency_limit)),
        )
        .nest("/health", health::create_router())
        .nest("/metrics", metrics::create_router())
        .layer(middleware)
        .layer(middleware::from_fn(check_max_body_size))
        .layer(trace)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", create_docs()))
        .fallback(|uri: Uri| async move {
            (StatusCode::NOT_FOUND, format!("No handler found for {uri}"))
        })
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
