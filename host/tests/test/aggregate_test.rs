use crate::common::{
    complete_batch_proof_request, make_aggregate_proof_request,
    make_batch_proof_request, setup, v3_assert_report, v3_complete_aggregate_proof_request,
};
use raiko_host::server::api;
use raiko_lib::consts::Network;
use raiko_lib::proof_type::ProofType;
use raiko_tasks::TaskStatus;

#[ignore]
#[tokio::test]
async fn test_v3_mainnet_aggregate_native() {
    v3_mainnet_aggregate(Network::TaikoMainnet, ProofType::Native).await;
}

#[ignore]
#[cfg(feature = "risc0")]
#[test_log::test(tokio::test)]
async fn test_v3_mainnet_aggregate_risc0() {
    v3_mainnet_aggregate(Network::TaikoMainnet, ProofType::Risc0).await;
}

async fn v3_mainnet_aggregate(network: Network, proof_type: ProofType) {
    setup_mock_zkvm_elf();


    let batch_ids = vec![crate::test::TEST_BLOCK_NUMBER];

    let (_server, client) = setup().await;
    let requests: Vec<_> = batch_ids
        .iter()
        .map(|batch_id| {
            let l1_inclusion_block_number = batch_id + 1; // Use next block as L1 inclusion
            make_batch_proof_request(&network, &proof_type, *batch_id, l1_inclusion_block_number)
        })
        .collect();
    let mut proofs = Vec::with_capacity(batch_ids.len());

    for request in requests {
        let status: api::v3::Status = client
            .post("/v3/proof/batch", &request)
            .await
            .expect("failed to send request");
        assert!(
            matches!(
                status,
                api::v3::Status::Ok {
                    data: api::v3::ProofResponse::Status {
                        status: TaskStatus::Registered
                            | TaskStatus::WorkInProgress
                            | TaskStatus::Success,
                        ..
                    },
                    ..
                }
            ),
            "status: {status:?}"
        );

        let proof = complete_batch_proof_request(&client, &request).await;
        proofs.push(proof);
    }

    let aggregate_request =
        make_aggregate_proof_request(&network, &proof_type, batch_ids, proofs).await;

    let status: api::v3::Status = client
        .post("/v3/proof/aggregate", &aggregate_request)
        .await
        .expect("failed to send aggregate proof request");
    assert!(
        matches!(
            status,
            api::v3::Status::Ok {
                data: api::v3::ProofResponse::Status {
                    status: TaskStatus::Registered
                        | TaskStatus::WorkInProgress
                        | TaskStatus::Success,
                    ..
                },
                ..
            }
        ),
        "status: {status:?}"
    );

    v3_complete_aggregate_proof_request(&client, &aggregate_request).await;

    // santy check for report format
    v3_assert_report(&client).await;
}

// Use mock zkvm elf for testing
fn setup_mock_zkvm_elf() {
    std::env::set_var("RAIKO_MOCK_ZKVM_ELF", "true");
}
