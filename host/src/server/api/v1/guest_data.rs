use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use utoipa::OpenApi;

use raiko_reqactor::Actor;

#[utoipa::path(
    get,
    path = "/guest_data",
    tag = "GuestData",
    responses (
        (status = 200, description = "Returns the guest data of provers, e.g. SGX quote bytes, sp1 program hashes"),
        (status = 500, description = "Failed to read guest data"),
    )
)]
/// Returns the contents of host/config/guest_data.json as JSON.
async fn guest_data() -> impl IntoResponse {
    match raiko_core::interfaces::get_guest_data().await {
        Ok(json) => (StatusCode::OK, Json(json)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{:?}", e)).into_response(),
    }
}

#[derive(OpenApi)]
#[openapi(paths(guest_data))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<Actor> {
    Router::new().route("/", get(guest_data))
}
