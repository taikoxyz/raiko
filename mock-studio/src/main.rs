use std::{env, sync::Arc};

use raiko_mock_studio::{
    app,
    runner::parse_studio_args,
    AppState,
    LocalCargoRunner,
    OpenRouterAgent,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = parse_studio_args(env::args())?;
    let agent = Arc::new(OpenRouterAgent::from_env()?);
    let runner = Arc::new(LocalCargoRunner::new(args.public_base_url.clone()));
    let listener = TcpListener::bind(&args.bind).await?;

    tracing::info!("starting mock studio on {}", args.bind);
    axum::serve(listener, app(AppState::new(agent.clone(), agent, runner))).await?;
    Ok(())
}
