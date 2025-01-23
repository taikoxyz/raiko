use crate::common::setup;
use raiko_ballot::Ballot;
use raiko_lib::proof_type::ProofType;
use serde_json::Value;
use std::collections::BTreeMap;

#[test_log::test(tokio::test)]
async fn test_debug_ballot() {
    let (_server, client) = setup().await;

    // Test get_ballot
    let initial_ballot: Value = client
        .reqwest_client
        .get(client.build_url("/debug/get_ballot"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let initial_ballot: Ballot = serde_json::from_value(initial_ballot).unwrap();

    // Create a test ballot
    let test_ballot = Ballot::new(ProofType::Sp1, BTreeMap::new()).unwrap();

    // Test set_ballot
    let set_response = client
        .reqwest_client
        .post(&client.build_url("/debug/set_ballot"))
        .json(&test_ballot)
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
        .get(client.build_url("/debug/get_ballot"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let updated_ballot: Ballot = serde_json::from_value(updated_ballot).unwrap();

    // Tricky comparison
    assert_ne!(initial_ballot.draw(0), updated_ballot.draw(0));
    assert_eq!(test_ballot.draw(0), updated_ballot.draw(0));
}
