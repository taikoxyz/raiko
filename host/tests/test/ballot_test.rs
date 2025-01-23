use crate::common::{complete_proof_request, make_proof_request, setup, v2_assert_report};
use raiko_core::interfaces::{ProofRequestOpt, ProverSpecificOpts};
use raiko_host::server::api;
use raiko_lib::consts::Network;
use raiko_lib::proof_type::ProofType;
use raiko_tasks::TaskStatus;

#[test_log::test(tokio::test)]
pub async fn test_ballot() {
    let api_version = "v2";
    let network = Network::TaikoMainnet;
    let proof_type = ProofType::Native;

    let block_number = crate::test::TEST_BLOCK_NUMBER;
    let (_server, client) = setup().await;
    let request = make_proof_request(&network, &proof_type, block_number);

    let request_and_statuses: Vec<api::v4::types::RequestAndStatus> = client
        .post("/v4/proof", &request)
        .await
        .expect("failed to send request");
    for request_and_status in request_and_statuses.iter() {
        let status = &request_and_status.status;
        assert!(
            matches!(
                *status,
                api::v2::Status::Ok {
                    data: api::v2::ProofResponse::Status {
                        status: TaskStatus::Registered,
                        ..
                    }
                }
            ),
            "status: {status:?}"
        );
    }

    for request_and_status in request_and_statuses.iter() {
        let proof_request_opt = ProofRequestOpt {
            block_number: Some(request_and_status.request.block_number),
            l1_inclusion_block_number: Some(request_and_status.request.l1_inclusion_block_number),
            network: Some(request_and_status.request.network.clone()),
            l1_network: Some(request_and_status.request.l1_network.clone()),
            graffiti: Some(request_and_status.request.graffiti.to_string()),
            prover: Some(request_and_status.request.prover.to_string()),
            proof_type: Some(request_and_status.request.proof_type.to_string()),

            // NOTE: these are not covered by the test, to keep the test simple, let's
            // just pass empty, the host will fulfill them.
            blob_proof_type: None,
            prover_args: ProverSpecificOpts::default(),
        };
        complete_proof_request(api_version, &client, &proof_request_opt).await;
    }

    // santy check for report format
    v2_assert_report(&client).await;
}
