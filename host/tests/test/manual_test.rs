use crate::common::{complete_batch_proof_request, v3_assert_report, Client};
use raiko_host::server::api;
use raiko_tasks::TaskStatus;
use serde_json::json;

/// This test is used to manually test the proof process. Operator can use this to test case to
/// simplly test online service.
///
/// To run this test, you need to set the `RAIKO_TEST_MANUAL_PROVE_ENABLED` environment variable to `true`, and
/// specify the parameters via environment variables.
///
/// ```bash
/// RAIKO_TEST_MANUAL_PROVE_ENABLED=true \
/// RAIKO_TEST_MANUAL_PROVE_NETWORK=taiko_mainnet \
/// RAIKO_TEST_MANUAL_PROVE_PROOF_TYPE=native \
/// RAIKO_TEST_MANUAL_PROVE_BATCH_ID=656443 \
/// RAIKO_TEST_MANUAL_PROVE_L1_INCLUSION_BLOCK_NUMBER=656444 \
/// RAIKO_TEST_MANUAL_PROVE_RAIKO_RPC_URL=https://rpc.raiko.xyz \
/// cargo test --test test_manual_prove -- --ignored
/// ```
#[test_log::test(tokio::test)]
#[ignore]
pub async fn test_manual_prove() {
    let enabled = std::env::var("RAIKO_TEST_MANUAL_PROVE_ENABLED").unwrap_or_default() == "true";
    if !enabled {
        return;
    }

    // Manual test enabled, we require all parameters to be set.
    // Currently, we don't validate the parameters, so operator should ensure the parameters are intended.
    let network = std::env::var("RAIKO_TEST_MANUAL_PROVE_NETWORK").unwrap_or_default();
    let proof_type = std::env::var("RAIKO_TEST_MANUAL_PROVE_PROOF_TYPE").unwrap_or_default();
    let batch_id = std::env::var("RAIKO_TEST_MANUAL_PROVE_BATCH_ID")
        .map(|s| s.parse::<u64>().unwrap())
        .unwrap();
    let l1_inclusion_block_number = std::env::var("RAIKO_TEST_MANUAL_PROVE_L1_INCLUSION_BLOCK_NUMBER")
        .map(|s| s.parse::<u64>().unwrap())
        .unwrap();
    let raiko_rpc_url = std::env::var("RAIKO_TEST_MANUAL_PROVE_RAIKO_RPC_URL").unwrap_or_default();

    let client = Client::new(raiko_rpc_url.clone());
    let request = json!({
        "network": network.clone(),
        "l1_network": "ethereum",
        "batches": [{
            "batch_id": batch_id,
            "l1_inclusion_block_number": l1_inclusion_block_number
        }],
        "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
        "aggregate": false,
        "proof_type": proof_type.clone(),
        "blob_proof_type": "proof_of_equivalence"
    });

    println!(
        "test_manual_prove parameters {}",
        json!({
            "request": request,
            "network": network,
            "proof_type": proof_type,
            "batch_id": batch_id,
            "l1_inclusion_block_number": l1_inclusion_block_number,
            "raiko_rpc_url": raiko_rpc_url,
        })
    );

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

    complete_batch_proof_request(&client, &request).await;
    v3_assert_report(&client).await;
}
