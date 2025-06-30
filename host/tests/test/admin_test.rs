use crate::common::setup;
use serde_json::Value;

#[test_log::test(tokio::test)]
async fn test_pause() -> Result<(), reqwest::Error> {
    let (_server, client) = setup().await;

    let response = client
        .reqwest_client
        .post(client.build_url("/admin/pause"))
        .send()
        .await?;

    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await?, "System paused successfully");
    Ok(())
}

#[test_log::test(tokio::test)]
async fn test_admin_ballot() {
    let (_server, client) = setup().await;

    // Test set_ballot
    let updating_ballot = serde_json::json!({"Sp1": [0.123, 0], "Risc0": [0.456, 0]});
    let set_response = client
        .reqwest_client
        .post(&client.build_url("/admin/set_ballot"))
        .json(&updating_ballot)
        .send()
        .await
        .unwrap();
    assert_eq!(
        set_response.text().await.unwrap(),
        "Ballot set successfully".to_string()
    );

    // Verify the ballot was set correctly
    let updated_ballot: Value = client
        .reqwest_client
        .get(client.build_url("/admin/get_ballot"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(updating_ballot, updated_ballot);
}
