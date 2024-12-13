use crate::common::{
    complete_aggregate_proof_request, complete_proof_request, make_aggregate_proof_request,
    make_proof_request, randomly_select_blocks, setup, v2_assert_report,
};
use raiko_host::server::api;
use raiko_lib::consts::Network;
use raiko_lib::proof_type::ProofType;
use raiko_tasks::TaskStatus;

#[tokio::test]
pub async fn test_v2_mainnet_native_aggregate() {
    let api_version = "v2";
    let network = Network::TaikoMainnet;
    let proof_type = ProofType::Native;
    let block_numbers = randomly_select_blocks(network, 2)
        .await
        .expect("randomly select blocks failed");
    println!(
        "test_aggregate_v2_mainnet_native network: {network}, proof_type: {proof_type}, block_numbers: {block_numbers:?}"
    );

    let (_server, client) = setup().await;
    let requests: Vec<_> = block_numbers
        .iter()
        .map(|block_number| make_proof_request(&network, &proof_type, *block_number))
        .collect();
    let mut proofs = Vec::with_capacity(block_numbers.len());

    for request in requests {
        let status: api::v2::Status = client
            .post("/v2/proof", &request)
            .await
            .expect("failed to send request");
        assert!(
            matches!(
                status,
                api::v2::Status::Ok {
                    data: api::v2::ProofResponse::Status {
                        status: TaskStatus::Registered,
                        ..
                    }
                }
            ),
            "status: {status:?}"
        );

        let proof = complete_proof_request(api_version, &client, &request).await;
        proofs.push(proof);
    }

    let aggregate_request =
        make_aggregate_proof_request(&network, &proof_type, block_numbers, proofs).await;

    // NOTE: Only v3 supports aggregate proof
    complete_aggregate_proof_request("v3", &client, &aggregate_request).await;

    // santy check for report format
    v2_assert_report(&client).await;
}
