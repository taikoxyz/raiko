use std::{env, sync::Arc};

use raiko_mock_studio::{app, AppState, LocalCargoRunner, OpenRouterAgent};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let bind = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:4010".to_string());
    let agent = Arc::new(OpenRouterAgent::from_env()?);
    let runner = Arc::new(LocalCargoRunner::default());
    let listener = TcpListener::bind(&bind).await?;

    tracing::info!("starting mock studio on {bind}");
    axum::serve(listener, app(AppState::new(agent.clone(), agent, runner))).await?;
    Ok(())
}
