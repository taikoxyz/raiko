use raiko_host::server::api::{v1, v2};
use raiko_tasks::TaskStatus;

use crate::common::{make_request, start_raiko, ProofClient};

/// Test v1 API interface.
pub async fn test_v1_api_format() -> anyhow::Result<()> {
    let token = start_raiko().await.expect("Failed to start Raiko server");

    // Send a proof request to the server.
    let client = ProofClient::new();

    let request = make_request().await?;
    let response = client.send_proof_v1(request).await?;

    assert!(
        matches!(
            response,
            v1::Status::Ok {
                data: v1::ProofResponse { .. }
            }
        ),
        "Got error response from server"
    );

    token.cancel();
    Ok(())
}

/// Test v2 API response for a initial proof request and for requesting the proof status on further
/// requests.
pub async fn test_v2_api_response() -> anyhow::Result<()> {
    let token = start_raiko().await.expect("Failed to start Raiko server");

    // Send a proof request to the server.
    let client = ProofClient::new();

    let request = make_request().await?;

    let response = client
        .send_proof_v2(request.clone())
        .await
        .expect("Failed to send proof request");

    assert!(
        matches!(
            response,
            v2::Status::Ok {
                data: v2::ProofResponse::Status {
                    status: TaskStatus::Registered
                }
            } | v2::Status::Ok {
                data: v2::ProofResponse::Proof { .. }
            }
        ),
        "Got error response from server"
    );

    // Wait a second to allow the server to process the request.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Check the server state.
    let response = client
        .send_proof_v2(request)
        .await
        .expect("Failed to send proof request");

    assert!(
        matches!(
            response,
            v2::Status::Ok {
                data: v2::ProofResponse::Status {
                    status: TaskStatus::WorkInProgress
                }
            } | v2::Status::Ok {
                data: v2::ProofResponse::Proof { .. }
            }
        ),
        "Got incorrect response from server"
    );

    token.cancel();
    Ok(())
}

/// Test the v2 API cancellation behavior.
pub async fn test_v2_cancellation() -> anyhow::Result<()> {
    let token = start_raiko().await.expect("Failed to start Raiko server");

    // Send a proof request to the server.
    let client = ProofClient::new();

    let request = make_request().await?;

    let response = client
        .send_proof_v2(request.clone())
        .await
        .expect("Failed to send proof request");

    assert!(
        matches!(
            response,
            v2::Status::Ok {
                data: v2::ProofResponse::Status {
                    status: TaskStatus::Registered
                }
            } | v2::Status::Ok {
                data: v2::ProofResponse::Proof { .. }
            }
        ),
        "Got error response from server"
    );

    // Cancel the proof request.
    let response = client
        .cancel_proof(request.clone())
        .await
        .expect("Failed to cancel proof request");

    assert!(
        matches!(response, v2::CancelStatus::Ok),
        "Got error response from server"
    );

    // Check that we can restart the proof request.
    let response = client
        .send_proof_v2(request)
        .await
        .expect("Failed to send proof request");

    assert!(
        matches!(
            response,
            v2::Status::Ok {
                data: v2::ProofResponse::Status {
                    status: TaskStatus::Registered
                }
            }
        ),
        "Got error response from server"
    );

    token.cancel();
    Ok(())
}

/// Test the v2 API report functionality before and after sending a request.
pub async fn test_v2_report() -> anyhow::Result<()> {
    let token = start_raiko().await.expect("Failed to start Raiko server");
    // Send a proof request to the server.
    let client = ProofClient::new();

    let response = client.report_proof().await?;

    assert!(response.is_empty(), "Proof report is not empty");

    let request = make_request().await?;

    let response = client
        .send_proof_v2(request)
        .await
        .expect("Failed to send proof request");

    assert!(
        matches!(
            response,
            v2::Status::Ok {
                data: v2::ProofResponse::Status {
                    status: TaskStatus::Registered
                }
            }
        ),
        "Got error response from server"
    );

    let response = client.report_proof().await?;

    assert!(
        !response.is_empty(),
        "No proof report found after sending proof request"
    );

    token.cancel();
    Ok(())
}

/// Test the v2 API pruning functionality after having requests in the task db.
pub async fn test_v2_prune() -> anyhow::Result<()> {
    let token = start_raiko().await.expect("Failed to start Raiko server");
    // Send a proof request to the server.
    let client = ProofClient::new();

    let response = client.report_proof().await?;

    assert!(response.is_empty(), "Proof report is not empty");

    let request = make_request().await?;

    let response = client
        .send_proof_v2(request)
        .await
        .expect("Failed to send proof request");

    assert!(
        matches!(
            response,
            v2::Status::Ok {
                data: v2::ProofResponse::Status {
                    status: TaskStatus::Registered
                }
            }
        ),
        "Got error response from server"
    );

    let response = client.report_proof().await?;

    assert!(
        !response.is_empty(),
        "No proof report found after sending proof request"
    );

    let response = client.prune_proof().await?;

    assert!(
        matches!(response, v2::PruneStatus::Ok),
        "Got error response from server"
    );

    let response = client.report_proof().await?;

    assert!(
        response.is_empty(),
        "Proof report is not empty after pruning"
    );

    token.cancel();
    Ok(())
}
