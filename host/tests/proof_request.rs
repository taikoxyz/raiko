#![cfg(feature = "integration")]
use crate::common::scenarios::{
    test_v1_api_format, test_v2_api_response, test_v2_cancellation, test_v2_prune, test_v2_report,
};

mod common;

#[tokio::test]
async fn run_scenarios_sequentially() -> anyhow::Result<()> {
    test_v2_prune().await?;
    test_v2_report().await?;
    test_v1_api_format().await?;
    test_v2_api_response().await?;
    test_v2_cancellation().await?;
    Ok(())
}
