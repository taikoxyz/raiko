use std::str::FromStr;

use anyhow::anyhow;
use axum::{debug_handler, extract::State, routing::post, Json, Router};
use raiko_core::{
    interfaces::{AggregationRequest, ProofRequest, ProofRequestOpt, ProofType},
    provider::get_task_data,
};
use raiko_lib::input::{AggregationGuestInput, AggregationGuestOutput};
use raiko_tasks::{TaskDescriptor, TaskManager, TaskStatus};
use reth_primitives::B256;
use utoipa::OpenApi;

use crate::{
    interfaces::HostResult,
    metrics::{inc_current_req, inc_guest_req_count, inc_host_req_count},
    server::api::{v2, v3::Status},
    Message, ProverState,
};

mod cancel;

#[utoipa::path(post, path = "/proof",
    tag = "Proving",
    request_body = ProofRequestOpt,
    responses (
        (status = 200, description = "Successfully submitted proof task, queried tasks in progress or retrieved proof.", body = Status)
    )
)]
#[debug_handler(state = ProverState)]
/// Submit a proof aggregation task with requested config, get task status or get proof value.
///
/// Accepts a proof request and creates a proving task with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn proof_handler(
    State(prover_state): State<ProverState>,
    Json(mut aggregation_request): Json<AggregationRequest>,
) -> HostResult<Status> {
    inc_current_req();
    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    aggregation_request.merge(&prover_state.request_config())?;

    let mut tasks = Vec::with_capacity(aggregation_request.block_numbers.len());

    let proof_request_opts: Vec<ProofRequestOpt> = aggregation_request.clone().into();

    // Construct the actual proof request from the available configs.
    for proof_request_opt in proof_request_opts {
        let proof_request = ProofRequest::try_from(proof_request_opt)?;

        inc_host_req_count(proof_request.block_number);
        inc_guest_req_count(&proof_request.proof_type, proof_request.block_number);

        let (chain_id, blockhash) = get_task_data(
            &proof_request.network,
            proof_request.block_number,
            &prover_state.chain_specs,
        )
        .await?;

        let key = TaskDescriptor::from((
            chain_id,
            blockhash,
            proof_request.proof_type,
            proof_request.prover.to_string(),
        ));

        tasks.push(key);
    }

    let mut manager = prover_state.task_manager();

    let mut is_registered = false;
    let mut is_success = true;

    for task in tasks.iter() {
        let status = manager.get_task_proving_status(task).await?;

        let Some((latest_status, ..)) = status.last() else {
            // If there are no tasks with provided config, create a new one.
            manager.enqueue_task(task).await?;

            prover_state.task_channel.try_send(Message::from(task))?;
            is_registered = true;
            continue;
        };

        match latest_status {
            // If task has been cancelled add it to the queue again
            TaskStatus::Cancelled
            | TaskStatus::Cancelled_Aborted
            | TaskStatus::Cancelled_NeverStarted
            | TaskStatus::CancellationInProgress => {
                manager
                    .update_task_progress(task.clone(), TaskStatus::Registered, None)
                    .await?;

                prover_state.task_channel.try_send(Message::from(task))?;

                is_registered = true;
                is_success = false;
            }
            // If the task has succeeded, return the proof.
            TaskStatus::Success => {}
            // For all other statuses just return the status.
            _status => {}
        }
    }

    if is_registered {
        Ok(TaskStatus::Registered.into())
    } else if is_success {
        // TODO:(petar) aggregate the proofs and return the result without blocking
        let mut proofs = Vec::with_capacity(tasks.len());
        for task in tasks {
            let raw_proof = manager.get_task_proof(&task).await?;
            let proof = serde_json::from_slice(&raw_proof)?;
            proofs.push(proof);
        }

        let proof_type = ProofType::from_str(
            aggregation_request
                .proof_type
                .as_ref()
                .ok_or_else(|| anyhow!("No proof type"))?,
        )?;
        let input = AggregationGuestInput { proofs };
        let output = AggregationGuestOutput { hash: B256::ZERO };
        let config = serde_json::to_value(aggregation_request)?;

        let proof = proof_type
            .aggregate_proofs(input, &output, &config, Some(&mut manager))
            .await?;

        Ok(proof.into())
    } else {
        Ok(TaskStatus::WorkInProgress.into())
    }
}

#[derive(OpenApi)]
#[openapi(paths(proof_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    [
        cancel::create_docs(),
        v2::proof::report::create_docs(),
        v2::proof::list::create_docs(),
        v2::proof::prune::create_docs(),
    ]
    .into_iter()
    .fold(Docs::openapi(), |mut docs, curr| {
        docs.merge(curr);
        docs
    })
}

pub fn create_router() -> Router<ProverState> {
    Router::new()
        .route("/", post(proof_handler))
        .nest("/cancel", cancel::create_router())
        .nest("/report", v2::proof::report::create_router())
        .nest("/list", v2::proof::list::create_router())
        .nest("/prune", v2::proof::prune::create_router())
}
