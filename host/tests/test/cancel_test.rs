use crate::common::{make_proof_request, randomly_select_block, setup};
use raiko_host::server::api;
use raiko_lib::consts::Network;
use raiko_lib::proof_type::ProofType;
use raiko_tasks::TaskStatus;
use test_log::test;

#[cfg(all(feature = "risc0", feature = "test-mock-guest"))]
#[test(tokio::test)]
async fn test_v2_mainnet_risc0_cancel() {
    test_v2_cancel(Network::TaikoMainnet, ProofType::Risc0).await;
}

#[test(tokio::test)]
async fn test_v2_mainnet_native_cancel() {
    test_v2_cancel(Network::TaikoMainnet, ProofType::Native).await;
}

async fn test_v2_cancel(network: Network, proof_type: ProofType) {
    let api_version = "v2";
    let block_number = randomly_select_block(network)
        .await
        .expect("randomly select block failed");

    let (_server, client) = setup().await;
    let request = make_proof_request(&network, &proof_type, block_number);

    let status: api::v2::Status = client
        .post(&format!("/{api_version}/proof"), &request)
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

    let status: api::v2::CancelStatus = client
        .post(&format!("/{api_version}/proof/cancel"), &request)
        .await
        .expect("failed to send request");
    assert!(matches!(status, api::v2::CancelStatus::Ok),);

    // Cancel again, should be ok
    let status: api::v2::CancelStatus = client
        .post(&format!("/{api_version}/proof/cancel"), &request)
        .await
        .expect("failed to send request");
    assert!(matches!(status, api::v2::CancelStatus::Ok),);
}

#[tokio::test]
pub async fn test_v2_mainnet_native_cancel_non_registered() {
    let api_version = "v2";
    let network = Network::TaikoMainnet;
    let proof_type = ProofType::Native;
    let block_number = randomly_select_block(network)
        .await
        .expect("randomly select block failed");

    let (_server, client) = setup().await;
    let request = make_proof_request(&network, &proof_type, block_number);

    // Did not register the proof request, cancel should fail
    let status: api::v2::CancelStatus = client
        .post(&format!("/{api_version}/proof/cancel"), &request)
        .await
        .expect("failed to send request");
    assert!(
        matches!(status, api::v2::CancelStatus::Error { .. }),
        "status should be error, got: {status:?}"
    );
}

#[tokio::test]
pub async fn test_v2_mainnet_native_cancel_then_register() {
    let api_version = "v2";
    let network = Network::TaikoMainnet;
    let proof_type = ProofType::Native;
    let block_number = randomly_select_block(network)
        .await
        .expect("randomly select block failed");

    let (_server, client) = setup().await;
    let request = make_proof_request(&network, &proof_type, block_number);

    let status: api::v2::Status = client
        .post(&format!("/{api_version}/proof"), &request)
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

    let status: api::v2::CancelStatus = client
        .post(&format!("/{api_version}/proof/cancel"), &request)
        .await
        .expect("failed to send request");
    assert!(matches!(status, api::v2::CancelStatus::Ok),);

    let status: api::v2::Status = client
        .post(&format!("/{api_version}/proof"), &request)
        .await
        .expect("failed to send request");
    assert!(matches!(status, api::v2::Status::Ok { .. }),);
}
