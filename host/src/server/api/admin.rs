use axum::{extract::State, routing::post, Router};

use crate::interfaces::HostResult;
use raiko_reqactor::Actor;

pub fn create_router() -> Router<Actor> {
    Router::new().route("/admin/pause", post(pause))
}

async fn pause(State(actor): State<Actor>) -> HostResult<&'static str> {
    actor.pause().await.map_err(|e| anyhow::anyhow!(e))?;
    Ok("System paused successfully")
}
