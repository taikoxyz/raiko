use crate::{
    app_args::{GlobalOpts, ServerArgs},
    one_shot::{bootstrap, one_shot},
};
use anyhow::Context;
use axum::{extract::State, Json};
use axum::{routing::post, Router};
use raiko_lib::{
    input::{GuestBatchInput, GuestInput},
    primitives::B256,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, str::FromStr};
use tokio::net::TcpListener;

pub async fn serve(server_args: ServerArgs, global_opts: GlobalOpts) {
    let state = ServerStateConfig {
        global_opts,
        server_args,
    };

    let router = Router::new()
        .route("/prove/block", post(prove_block))
        .route("/prove/batcn", post(prove_batch))
        .route("/prove/aggregate", post(prove_aggregation))
        .route("/check", post(check_server))
        .route("/bootstrap", post(bootstrap_server))
        .with_state(state.clone());

    let address = format!("{}:{}", state.server_args.address, state.server_args.port);
    let addr = SocketAddr::from_str(&address).expect("addr is correct");
    let listener = TcpListener::bind(addr).await.expect("create listener ok");

    println!(
        "Listening on: {}",
        listener.local_addr().expect("correct listener local addr")
    );

    let _ = axum::serve(listener, router)
        .await
        .context("Server couldn't serve");
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SgxResponse {
    /// proof format: 4b(id)+20b(pubkey)+65b(signature)
    pub proof: String,
    pub quote: String,
    pub input: B256,
}

#[derive(Clone)]
pub struct ServerStateConfig {
    pub global_opts: GlobalOpts,
    pub server_args: ServerArgs,
}

async fn prove_block(
    State(state): State<ServerStateConfig>,
    Json(input): Json<GuestInput>,
) -> String {
    todo!();
}

async fn prove_batch(
    State(state): State<ServerStateConfig>,
    Json(batch_input): Json<GuestBatchInput>,
) -> String {
    todo!();
}

async fn prove_aggregation(
    State(state): State<ServerStateConfig>,
    Json(proofs): Json<GuestBatchInput>,
) -> String {
    todo!();
}

async fn bootstrap_server(State(state): State<ServerStateConfig>) -> String {
    bootstrap(state.global_opts).expect("bootstrap ok");
    "".to_owned()
}

async fn check_server(
    State(state): State<ServerStateConfig>,
    Json(proofs): Json<GuestBatchInput>,
) -> String {
    todo!();
}
