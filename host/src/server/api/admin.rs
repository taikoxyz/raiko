use axum::{extract::State, routing::post, Router};

use crate::interfaces::HostResult;
use raiko_reqactor::Gateway;

pub fn create_router<P: raiko_reqpool::Pool + 'static>() -> Router<Gateway<P>> {
    Router::new().route("/admin/pause", post(pause))
}

async fn pause<P: raiko_reqpool::Pool>(
    State(gateway): State<Gateway<P>>,
) -> HostResult<&'static str> {
    gateway.pause().await.map_err(|e| anyhow::anyhow!(e))?;
    Ok("System paused successfully")
}
