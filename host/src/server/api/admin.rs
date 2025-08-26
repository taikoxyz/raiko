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
        .route("/set_ballot_zk", post(set_ballot_zk))
        .route("/get_ballot_zk", get(get_ballot_zk))
        .route("/set_ballot_sgx", post(set_ballot_sgx))
        .route("/get_ballot_sgx", get(get_ballot_sgx))
}

async fn pause(State(actor): State<Actor>) -> HostResult<&'static str> {
    actor.pause().await.map_err(|e| anyhow::anyhow!(e))?;
    Ok("System paused successfully")
}

async fn set_ballot_zk(
    State(actor): State<Actor>,
    Json(probs): Json<BTreeMap<ProofType, f64>>,
) -> HostResult<&'static str> {
    let ballot = Ballot::new(probs).map_err(|e| anyhow::anyhow!(e))?;
    actor.set_ballot_zk(ballot);
    Ok("Ballot set successfully")
}

async fn get_ballot_zk(State(actor): State<Actor>) -> Response {
    let ballot = actor.get_ballot_zk().probabilities().to_owned();
    Json(ballot).into_response()
}

async fn set_ballot_sgx(
    State(actor): State<Actor>,
    Json(probs): Json<BTreeMap<ProofType, f64>>,
) -> HostResult<&'static str> {
    let ballot = Ballot::new(probs).map_err(|e| anyhow::anyhow!(e))?;
    actor.set_ballot_sgx(ballot);
    Ok("Ballot set successfully")
}

async fn get_ballot_sgx(State(actor): State<Actor>) -> Response {
    let ballot = actor.get_ballot_sgx().probabilities().to_owned();
    Json(ballot).into_response()
}
