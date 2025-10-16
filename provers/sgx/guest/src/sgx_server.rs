use crate::{
    app_args::{GlobalOpts, OneShotArgs, ServerArgs},
    one_shot::{aggregate, bootstrap, one_shot, one_shot_batch, shasta_aggregate},
};
use anyhow::Context;
use axum::{
    extract::{DefaultBodyLimit, State},
    routing::{get, post},
    Json, Router,
};
use raiko_lib::{
    consts::SpecId,
    input::{
        GuestBatchInput, GuestInput, RawAggregationGuestInput, ShastaRawAggregationGuestInput,
    },
    primitives::B256,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, str::FromStr};
use tokio::net::TcpListener;

#[derive(Debug, thiserror::Error)]
pub enum SgxServerError {
    // Instance Ids config error
    #[error("Instance Ids config error")]
    InvalidInstanceIds(String),
}

pub fn get_instance_id_from_params(
    input: &GuestInput,
    state: &ServerStateConfig,
) -> Result<u32, SgxServerError> {
    let spec_id = input
        .chain_spec
        .active_fork(input.block.number, input.block.timestamp)
        .map_err(|e| SgxServerError::InvalidInstanceIds(e.to_string()))?;

    state
        .server_args
        .sgx_instance_ids
        .get(&spec_id)
        .cloned()
        .ok_or_else(|| {
            SgxServerError::InvalidInstanceIds(format!(
                "No instance id found for spec id: {:?}",
                spec_id
            ))
        })
}

pub async fn serve(server_args: ServerArgs, global_opts: GlobalOpts) {
    let state = ServerStateConfig {
        global_opts,
        server_args,
    };

    let router = Router::new()
        .route("/prove/block", post(prove_block))
        .route("/prove/batch", post(prove_batch))
        .route("/prove/aggregate", post(prove_aggregation))
        .route("/prove/shasta-aggregate", post(prove_shasta_aggregation))
        .route("/check", post(check_server))
        .route("/bootstrap", post(bootstrap_server))
        .route("/health", get(health_check))
        .layer(DefaultBodyLimit::max(10000 * 1024 * 1024)) // max 10G
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
pub struct ServerResponse {
    pub status: String,
    pub message: String,
    pub proof: SgxResponse,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SgxResponse {
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
) -> Json<ServerResponse> {
    match get_instance_id_from_params(&input, &state) {
        Ok(sgx_instance_id) => {
            let args = OneShotArgs { sgx_instance_id };
            match one_shot(state.global_opts, args, input).await {
                Ok(sgx_proof) => {
                    let sgx_response: SgxResponse =
                        serde_json::from_value(sgx_proof).expect("deserialize proof to response");
                    Json(ServerResponse {
                        status: "success".to_owned(),
                        message: "".to_owned(),
                        proof: sgx_response,
                    })
                }
                Err(e) => Json(ServerResponse {
                    status: "error".to_owned(),
                    message: e.to_string(),
                    ..Default::default()
                }),
            }
        }
        Err(e) => Json(ServerResponse {
            status: "error".to_owned(),
            message: e.to_string(),
            ..Default::default()
        }),
    }
}

async fn prove_batch(
    State(state): State<ServerStateConfig>,
    Json(batch_input): Json<GuestBatchInput>,
) -> Json<ServerResponse> {
    match get_instance_id_from_params(&batch_input.inputs[0], &state) {
        Ok(sgx_instance_id) => {
            let args = OneShotArgs { sgx_instance_id };
            match one_shot_batch(state.global_opts, args, batch_input).await {
                Ok(sgx_proof) => {
                    let sgx_response: SgxResponse =
                        serde_json::from_value(sgx_proof).expect("deserialize proof to response");
                    Json(ServerResponse {
                        status: "success".to_owned(),
                        message: "".to_owned(),
                        proof: sgx_response,
                    })
                }
                Err(e) => Json(ServerResponse {
                    status: "error".to_owned(),
                    message: e.to_string(),
                    ..Default::default()
                }),
            }
        }
        Err(e) => Json(ServerResponse {
            status: "error".to_owned(),
            message: e.to_string(),
            ..Default::default()
        }),
    }
}

async fn prove_aggregation(
    State(state): State<ServerStateConfig>,
    Json(input): Json<RawAggregationGuestInput>,
) -> Json<ServerResponse> {
    let sgx_instance_id = match state
        .server_args
        .sgx_instance_ids
        .get(&SpecId::PACAYA)
        .copied()
    {
        Some(id) => id,
        None => {
            return Json(ServerResponse {
                status: "error".to_owned(),
                message: "No instance id found for PACAYA fork".to_owned(),
                ..Default::default()
            })
        }
    };

    let args = OneShotArgs { sgx_instance_id };
    match aggregate(state.global_opts, args, input).await {
        Ok(sgx_proof) => {
            let sgx_response: SgxResponse =
                serde_json::from_value(sgx_proof).expect("deserialize proof to response");
            Json(ServerResponse {
                status: "success".to_owned(),
                message: "".to_owned(),
                proof: sgx_response,
            })
        }
        Err(e) => Json(ServerResponse {
            status: "error".to_owned(),
            message: e.to_string(),
            ..Default::default()
        }),
    }
}

async fn prove_shasta_aggregation(
    State(state): State<ServerStateConfig>,
    Json(input): Json<ShastaRawAggregationGuestInput>,
) -> Json<ServerResponse> {
    let sgx_instance_id = match state
        .server_args
        .sgx_instance_ids
        .get(&SpecId::SHASTA)
        .copied()
    {
        Some(id) => id,
        None => {
            return Json(ServerResponse {
                status: "error".to_owned(),
                message: "No instance id found for SHASTA fork".to_owned(),
                ..Default::default()
            })
        }
    };

    let args = OneShotArgs { sgx_instance_id };
    match shasta_aggregate(state.global_opts, args, input).await {
        Ok(sgx_proof) => {
            let sgx_response: SgxResponse =
                serde_json::from_value(sgx_proof).expect("deserialize proof to response");
            Json(ServerResponse {
                status: "success".to_owned(),
                message: "".to_owned(),
                proof: sgx_response,
            })
        }
        Err(e) => Json(ServerResponse {
            status: "error".to_owned(),
            message: e.to_string(),
            ..Default::default()
        }),
    }
}

async fn bootstrap_server(State(state): State<ServerStateConfig>) -> String {
    bootstrap(state.global_opts).expect("bootstrap ok");
    "".to_owned()
}

async fn check_server(
    State(_state): State<ServerStateConfig>,
    Json(_proofs): Json<GuestBatchInput>,
) -> String {
    todo!();
}

async fn health_check() -> &'static str {
    "OK"
}
