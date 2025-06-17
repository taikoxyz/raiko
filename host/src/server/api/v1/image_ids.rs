use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use std::fs;
use std::path::Path;
use utoipa::OpenApi;

use raiko_reqactor::Actor;

#[utoipa::path(
    get,
    path = "/image_ids",
    tag = "ImageIds",
    responses (
        (status = 200, description = "Returns the image_ids.json contents"),
        (status = 500, description = "Failed to read image_ids.json"),
    )
)]
/// Returns the contents of host/config/image_ids.json as JSON.
async fn image_ids_handler() -> impl IntoResponse {
    let path = Path::new("host/config/image_ids.json");
    match fs::read_to_string(path) {
        Ok(contents) => match serde_json::from_str::<serde_json::Value>(&contents) {
            Ok(json) => (StatusCode::OK, Json(json)).into_response(),
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Invalid JSON").into_response(),
        },
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "File not found").into_response(),
    }
}

#[derive(OpenApi)]
#[openapi(paths(image_ids_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<Actor> {
    Router::new().route("/", get(image_ids_handler))
}
