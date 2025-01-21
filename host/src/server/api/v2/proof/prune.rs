use axum::{extract::State, routing::post, Router};
use utoipa::OpenApi;

use crate::{interfaces::HostResult, server::api::v2::PruneStatus};
use raiko_reqactor::Actor;

#[utoipa::path(post, path = "/proof/prune",
    tag = "Proving",
    responses (
        (status = 200, description = "Successfully pruned tasks", body = PruneStatus)
    )
)]
/// Prune all tasks.
async fn prune_handler(State(_actor): State<Actor>) -> HostResult<PruneStatus> {
    todo!()
}

#[derive(OpenApi)]
#[openapi(paths(prune_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<Actor> {
    Router::new().route("/", post(prune_handler))
}
