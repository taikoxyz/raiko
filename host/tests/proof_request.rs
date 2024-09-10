mod common;

#[tokio::test]
#[cfg(feature = "integration")]
async fn run_scenarios_sequentially() -> anyhow::Result<()> {
    use crate::common::scenarios::{
        test_v1_api_format, test_v2_api_response, test_v2_cancellation, test_v2_prune,
        test_v2_report,
    };

    let cwd = std::env::current_dir()?;
    let main_dir = cwd.join("../");
    std::env::set_current_dir(main_dir)?;
    test_v2_prune().await?;
    test_v2_report().await?;
    test_v1_api_format().await?;
    test_v2_api_response().await?;
    test_v2_cancellation().await?;
    Ok(())
}
