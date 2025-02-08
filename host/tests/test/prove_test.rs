use crate::common::{complete_proof_request, make_proof_request, setup, v2_assert_report};
use raiko_host::server::api;
use raiko_lib::consts::Network;
use raiko_lib::proof_type::ProofType;
use raiko_tasks::TaskStatus;

#[ignore]
#[test_log::test(tokio::test)]
pub async fn test_v2_mainnet_native_prove() {
    let api_version = "v2";
    let network = Network::TaikoMainnet;
    let proof_type = ProofType::Native;

    let block_number = crate::test::TEST_BLOCK_NUMBER;
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
