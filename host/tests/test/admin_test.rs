use crate::common::setup;

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
