use axum::{extract::State, routing::post, Router};
use utoipa::OpenApi;

use crate::interfaces::HostResult;
use raiko_reqactor::Actor;

#[utoipa::path(post, path = "/proof/aggregate/prune",
    tag = "Proving",
    responses (
        (status = 200, description = "Successfully pruned all aggregation tasks", body = PruneStatus)
    )
)]
/// Prune all aggregation tasks.
async fn prune_handler(State(actor): State<Actor>) -> HostResult<()> {
    let statuses = actor.pool_list_status().map_err(|e| anyhow::anyhow!(e))?;
    for (key, status) in statuses {
        tracing::info!("Pruning task: {key} with status: {status}");
        let _ = actor
            .pool_remove_request(&key)
            .map_err(|e| anyhow::anyhow!(e))?;
    }
    Ok(())
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
