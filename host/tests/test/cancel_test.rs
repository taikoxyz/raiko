use crate::common::{make_batch_proof_request, setup};
use raiko_host::server::api;
use raiko_lib::consts::Network;
use raiko_lib::proof_type::ProofType;
use raiko_tasks::TaskStatus;

#[ignore]
#[test_log::test(tokio::test)]
pub async fn test_v3_mainnet_native_batch_cancel() {
    let network = Network::TaikoMainnet;
    let proof_type = ProofType::Native;

    let batch_id = crate::test::TEST_BLOCK_NUMBER;
    let l1_inclusion_block_number = batch_id + 1; // Use next block as L1 inclusion
    let (_server, client) = setup().await;
    let request = make_batch_proof_request(&network, &proof_type, batch_id, l1_inclusion_block_number);

    let status: api::v3::Status = client
        .post("/v3/proof/batch", &request)
        .await
        .expect("failed to send request");
    assert!(
        matches!(
            status,
            api::v3::Status::Ok {
                data: api::v3::ProofResponse::Status {
                    status: TaskStatus::Registered,
                },
                ..
            }
        ),
        "status: {status:?}"
    );

    let status: api::v3::CancelStatus = client
        .post("/v3/proof/batch/cancel", &request)
        .await
        .expect("failed to send request");
    assert!(matches!(status, api::v3::CancelStatus::Ok),);

    // Cancel again, should be ok
    let status: api::v3::CancelStatus = client
        .post("/v3/proof/batch/cancel", &request)
        .await
        .expect("failed to send request");
    assert!(matches!(status, api::v3::CancelStatus::Ok),);
}

#[ignore]
#[test_log::test(tokio::test)]
pub async fn test_v3_mainnet_native_batch_cancel_non_registered() {
    let network = Network::TaikoMainnet;
    let proof_type = ProofType::Native;

    let batch_id = crate::test::TEST_BLOCK_NUMBER;
    let l1_inclusion_block_number = batch_id + 1; // Use next block as L1 inclusion

    let (_server, client) = setup().await;
    let request = make_batch_proof_request(&network, &proof_type, batch_id, l1_inclusion_block_number);

    // Did not register the proof request, cancel should fail
    let status: api::v3::CancelStatus = client
        .post("/v3/proof/batch/cancel", &request)
        .await
        .expect("failed to send request");
    assert!(
        matches!(status, api::v3::CancelStatus::Error { .. }),
        "status should be error, got: {status:?}"
    );
}

#[ignore]
#[test_log::test(tokio::test)]
pub async fn test_v3_mainnet_native_batch_cancel_then_register() {
    let network = Network::TaikoMainnet;
    let proof_type = ProofType::Native;

    let batch_id = crate::test::TEST_BLOCK_NUMBER;
    let l1_inclusion_block_number = batch_id + 1; // Use next block as L1 inclusion

    let (_server, client) = setup().await;
    let request = make_batch_proof_request(&network, &proof_type, batch_id, l1_inclusion_block_number);

    let status: api::v3::Status = client
        .post("/v3/proof/batch", &request)
        .await
        .expect("failed to send request");
    assert!(
        matches!(
            status,
            api::v3::Status::Ok {
                data: api::v3::ProofResponse::Status {
                    status: TaskStatus::Registered,
                    ..
                },
                ..
            }
        ),
        "status: {status:?}"
    );

    let status: api::v3::CancelStatus = client
        .post("/v3/proof/batch/cancel", &request)
        .await
        .expect("failed to send request");
    assert!(matches!(status, api::v3::CancelStatus::Ok),);

    let status: api::v3::Status = client
        .post("/v3/proof/batch", &request)
        .await
        .expect("failed to send request");
    assert!(matches!(status, api::v3::Status::Ok { .. }),);
}
