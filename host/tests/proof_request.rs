use common::{find_recent_block, start_raiko};
use raiko_core::interfaces::{ProofRequestOpt, ProverSpecificOpts};
use raiko_host::server::api::v2::{CancelStatus, ProofResponse, Status};
use raiko_lib::consts::Network;
use raiko_tasks::TaskStatus;

mod common;

#[tokio::test]
// #[cfg(feature = "integration")]
/// Test sending a proof request to the server. The server should respond with a `Registered`
/// status.
async fn send_proof_request() {
    let token = start_raiko().await.expect("Failed to start Raiko server");

    // Get block to test with.
    let block_number = find_recent_block(Network::TaikoMainnet)
        .await
        .expect("Failed to find recent block");

    // Send a proof request to the server.
    let client = common::ProofClient::new();
    let request = ProofRequestOpt {
        block_number: Some(block_number),
        l1_inclusive_block_number: None,
        network: Some("taiko_mainnet".to_owned()),
        l1_network: Some("ethereum".to_string()),
        graffiti: Some(
            "8008500000000000000000000000000000000000000000000000000000000000".to_owned(),
        ),
        prover: Some("0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_owned()),
        proof_type: Some("native".to_owned()),
        blob_proof_type: Some("kzg_versioned_hash".to_string()),
        prover_args: ProverSpecificOpts {
            native: None,
            sgx: None,
            sp1: None,
            risc0: None,
        },
    };

    let response = client
        .send_proof_v2(request.clone())
        .await
        .expect("Failed to send proof request");

    assert!(
        matches!(
            response,
            Status::Ok {
                data: ProofResponse::Status {
                    status: TaskStatus::Registered
                }
            }
        ),
        "Got error response from server"
    );

    // Wait a second to allow the server to process the request.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Check the server state.
    let response = client
        .send_proof_v2(request.clone())
        .await
        .expect("Failed to send proof request");

    assert!(
        matches!(
            response,
            Status::Ok {
                data: ProofResponse::Status {
                    status: TaskStatus::WorkInProgress
                }
            }
        ),
        "Got incorrect response from server"
    );

    // Cancel the proof request.
    let response = client
        .cancel_proof(request.clone())
        .await
        .expect("Failed to cancel proof request");

    assert!(
        matches!(response, CancelStatus::Ok),
        "Got error response from server"
    );

    // Check that we can restart the proof request.
    let response = client
        .send_proof_v2(request.clone())
        .await
        .expect("Failed to send proof request");

    assert!(
        matches!(
            response,
            Status::Ok {
                data: ProofResponse::Status {
                    status: TaskStatus::Registered
                }
            }
        ),
        "Got error response from server"
    );

    // Cancel the server.
    token.cancel();
}
