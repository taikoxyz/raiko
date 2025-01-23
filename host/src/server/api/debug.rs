use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::Json;
use axum::{extract::State, routing::post, Router};
use raiko_ballot::Ballot;

use crate::interfaces::HostResult;
use raiko_reqactor::Actor;

pub fn create_router() -> Router<Actor> {
    Router::new()
        .route("/get_ballot", get(get_ballot))
        .route("/set_ballot", post(set_ballot))
}

#[axum::debug_handler]
async fn get_ballot(State(actor): State<Actor>) -> Response {
    let ballot = actor.get_ballot();
    Json(serde_json::to_value(ballot).unwrap()).into_response()
}

async fn set_ballot(
    State(actor): State<Actor>,
    Json(ballot): Json<Ballot>,
) -> HostResult<&'static str> {
    actor.set_ballot(ballot);
    Ok("Ballot set successfully")
}
