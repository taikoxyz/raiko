use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::Json;
use axum::{extract::State, routing::post, Router};
use raiko_ballot::Ballot;
use raiko_lib::proof_type::ProofType;
use std::collections::BTreeMap;

use crate::interfaces::HostResult;
use raiko_reqactor::Actor;

pub fn create_router() -> Router<Actor> {
    Router::new()
        .route("/pause", post(pause))
        .route("/set_ballot", post(set_ballot))
        .route("/get_ballot", get(get_ballot))
}

async fn pause(State(actor): State<Actor>) -> HostResult<&'static str> {
    actor.pause().await.map_err(|e| anyhow::anyhow!(e))?;
    Ok("System paused successfully")
}

async fn set_ballot(
    State(actor): State<Actor>,
    Json(probs): Json<BTreeMap<ProofType, (f64, u64)>>,
) -> HostResult<&'static str> {
    let ballot = Ballot::new(probs).map_err(|e| anyhow::anyhow!(e))?;
    actor.set_ballot(ballot).await;
    Ok("Ballot set successfully")
}

async fn get_ballot(State(actor): State<Actor>) -> Response {
    let ballot = actor.get_ballot().await.probabilities().to_owned();
    Json(ballot).into_response()
}
