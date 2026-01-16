use crate::common::{complete_batch_proof_request, make_batch_proof_request, setup, v3_assert_report};
use raiko_host::server::api;
use raiko_lib::consts::Network;
use raiko_lib::proof_type::ProofType;
use raiko_tasks::TaskStatus;

#[ignore]
#[test_log::test(tokio::test)]
pub async fn test_v3_mainnet_native_batch_prove() {
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

    complete_batch_proof_request(&client, &request).await;

    // sending the same completed request should should be ok
    complete_batch_proof_request(&client, &request).await;

    // santy check for report format
    v3_assert_report(&client).await;
}

#[ignore = "v3 batch prove is not supported"]
#[test_log::test(tokio::test)]
pub async fn test_v3_mainnet_zk_any_batch_prove() {
    let network = Network::TaikoMainnet;

    let batch_id = crate::test::TEST_BLOCK_NUMBER;
    let l1_inclusion_block_number = batch_id + 1; // Use next block as L1 inclusion
    let (_server, client) = setup().await;
    let mut request = make_batch_proof_request(&network, &ProofType::Native, batch_id, l1_inclusion_block_number);

    // Ensure the ballot is set to {"native": (1.0, 0)}, so that our zk_any request will always been drawn
    let set_response = client
        .reqwest_client
        .post(&client.build_url("/admin/set_ballot"))
        .json(&serde_json::json!({"Native": [1.0, 0]}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        set_response.text().await.unwrap(),
        "Ballot set successfully".to_string()
    );

    // Modify to zk_any request
    request["proof_type"] = serde_json::Value::String("zk_any".to_string());

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

    complete_batch_proof_request(&client, &request).await;

    // sending the same completed request should should be ok
    complete_batch_proof_request(&client, &request).await;

    // santy check for report format
    v3_assert_report(&client).await;
}

#[ignore = "v3 batch prove is not supported"]
#[test_log::test(tokio::test)]
pub async fn test_v3_mainnet_zk_any_batch_prove_but_not_drawn() {
    let network = Network::TaikoMainnet;

    let batch_id = crate::test::TEST_BLOCK_NUMBER;
    let l1_inclusion_block_number = batch_id + 1; // Use next block as L1 inclusion
    let (_server, client) = setup().await;
    let mut request = make_batch_proof_request(&network, &ProofType::Native, batch_id, l1_inclusion_block_number);

    // Ensure the ballot is set to {}, so that our zk_any request will always not been drawn
    let set_response = client
        .reqwest_client
        .post(&client.build_url("/admin/set_ballot"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        set_response.text().await.unwrap(),
        "Ballot set successfully".to_string()
    );

    // Modify to zk_any request
    request["proof_type"] = serde_json::Value::String("zk_any".to_string());

    let _status: api::v3::Status = client
        .post("/v3/proof/batch", &request)
        .await
        .expect("failed to send request");
    // NOTE: API changed
    // assert!(
    //     matches!(
    //         status,
    //         api::v3::Status::Error {
    //             ref error,
    //             ref message,
    //         } if error == "zk_any_not_drawn_error" && message == "The zk_any request is not drawn",
    //     ),
    //     "status: {status:?}"
    // );
}
