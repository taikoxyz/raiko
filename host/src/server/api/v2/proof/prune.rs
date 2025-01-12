use axum::{extract::State, routing::post, Router};
use utoipa::OpenApi;

use crate::{interfaces::HostResult, server::api::v2::PruneStatus};
use raiko_reqactor::Gateway;

#[utoipa::path(post, path = "/proof/prune",
    tag = "Proving",
    responses (
        (status = 200, description = "Successfully pruned tasks", body = PruneStatus)
    )
)]
// #[debug_handler(state = Gateway<P>)]
/// Prune all tasks.
async fn prune_handler<P: raiko_reqpool::Pool + 'static>(
    State(_gateway): State<Gateway<P>>,
) -> HostResult<PruneStatus> {
    todo!()
}

#[derive(OpenApi)]
#[openapi(paths(prune_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router<P: raiko_reqpool::Pool + 'static>() -> Router<Gateway<P>> {
    Router::new().route("/", post(prune_handler::<P>))
}
