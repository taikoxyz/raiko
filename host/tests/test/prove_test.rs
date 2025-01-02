use crate::common::{
    complete_proof_request, make_proof_request, randomly_select_block, setup, v2_assert_report,
};
use raiko_host::server::api;
use raiko_lib::consts::Network;
use raiko_lib::proof_type::ProofType;
use raiko_tasks::TaskStatus;
use test_log::test;

#[cfg(all(feature = "risc0", feature = "test-mock-guest"))]
#[test(tokio::test)]
async fn test_v2_mainnet_prove_risc0() {
    test_v2_prove(Network::TaikoMainnet, ProofType::Risc0).await;
}

#[test(tokio::test)]
async fn test_v2_mainnet_native_prove() {
    test_v2_prove(Network::TaikoMainnet, ProofType::Native).await;
}

async fn test_v2_prove(network: Network, proof_type: ProofType) {
    let api_version = "v2";
    let block_number = randomly_select_block(network)
        .await
        .expect("randomly select block failed");
    println!(
        "test_prove_v2_mainnet_native network: {network}, proof_type: {proof_type}, block_number: {block_number}"
    );

    let (_server, client) = setup().await;
    let request = make_proof_request(&network, &proof_type, block_number);

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

    complete_proof_request(api_version, &client, &request).await;

    // sending the same completed request should should be ok
    complete_proof_request(api_version, &client, &request).await;

    // santy check for report format
    v2_assert_report(&client).await;
}
