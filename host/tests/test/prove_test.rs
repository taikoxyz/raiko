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
                },
                ..
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

#[ignore = "v2 prove is not supported"]
#[test_log::test(tokio::test)]
pub async fn test_v2_mainnet_zk_any_prove() {
    let api_version = "v2";
    let network = Network::TaikoMainnet;

    let block_number = crate::test::TEST_BLOCK_NUMBER;
    let (_server, client) = setup().await;
    let mut request = make_proof_request(&network, &ProofType::Native, block_number);

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
    request.proof_type = Some("zk_any".to_string());

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
                },
                ..
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

#[ignore = "v2 prove is not supported"]
#[test_log::test(tokio::test)]
pub async fn test_v2_mainnet_zk_any_prove_but_not_drawn() {
    let network = Network::TaikoMainnet;

    let block_number = crate::test::TEST_BLOCK_NUMBER;
    let (_server, client) = setup().await;
    let mut request = make_proof_request(&network, &ProofType::Native, block_number);

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
    request.proof_type = Some("zk_any".to_string());

    let _status: api::v2::Status = client
        .post("/v2/proof", &request)
        .await
        .expect("failed to send request");
    // NOTE: API changed
    // assert!(
    //     matches!(
    //         status,
    //         api::v2::Status::Error {
    //             ref error,
    //             ref message,
    //         } if error == "zk_any_not_drawn_error" && message == "The zk_any request is not drawn",
    //     ),
    //     "status: {status:?}"
    // );
}
