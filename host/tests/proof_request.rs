use common::find_recent_block;
use raiko_core::interfaces::{ProofRequestOpt, ProverSpecificOpts};
use raiko_host::{
    server::{api::v2::Status, serve},
    ProverState,
};
use raiko_lib::consts::Network;
use tokio::select;
use tokio_util::sync::CancellationToken;

mod common;

#[tokio::test]
/// Test sending a proof request to the server. The server should respond with a `Registered`
/// status.
async fn send_proof_request() {
    // Initialize the server state.
    dotenv::dotenv().ok();
    let state = ProverState::init().expect("Failed to initialize prover state");
    let token = CancellationToken::new();
    let clone = token.clone();

    // Run the server in a separate thread with the ability to cancel it when our testing is done.
    tokio::spawn(async move {
        select! {
            _ = token.cancelled() => {
                println!("Test done");
            }
            result = serve(state) => {
                match result {
                    Ok(()) => {
                        assert!(false, "Unexpected server shutdown");
                    }
                    Err(error) => {
                        assert!(false, "Server failed due to: {error:?}");
                    }
                };
            }
        }
    });

    // Get block to test with.
    let block_number = find_recent_block(Network::TaikoMainnet)
        .await
        .expect("Failed to find recent block");

    // Send a proof request to the server.
    let client = common::ProofClient::new();
    let response = client
        .send_proof_v2(ProofRequestOpt {
            block_number: Some(block_number),
            l1_inclusive_block_number: None,
            network: Some("taiko_mainnet".to_owned()),
            l1_network: Some("ethereum".to_string()),
            graffiti: Some(
                "8008500000000000000000000000000000000000000000000000000000000000".to_owned(),
            ),
            prover: Some("0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_owned()),
            proof_type: Some("native".to_owned()),
            blob_proof_type: None,
            prover_args: ProverSpecificOpts {
                native: None,
                sgx: None,
                sp1: None,
                risc0: None,
            },
        })
        .await
        .expect("Failed to send proof request");

    assert!(
        matches!(response, Status::Ok { .. }),
        "Got error response from server"
    );

    // Cancel the server.
    clone.cancel();
}
