use crate::interfaces::HostResult;
use axum::{extract::State, routing::post, Router};
use raiko_reqactor::Actor;
use utoipa::OpenApi;

#[utoipa::path(post, path = "/proof/aggregate/prune",
    tag = "Proving",
    responses (
        (status = 200, description = "Successfully pruned all aggregation tasks", body = PruneStatus)
    )
)]
/// Prune all aggregation tasks.
async fn prune_handler(State(actor): State<Actor>) -> HostResult<()> {
    let statuses = actor
        .pool_list_status()
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    for (key, status) in statuses {
        tracing::info!("Pruning task: {key} with status: {status}");
        let _ = actor
            .pool_remove_request(&key)
            .await
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
