use clap::Parser;
use raiko_gateway::{app, AppState, Config};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::parse();
    let listener = TcpListener::bind(&config.bind).await?;

    tracing::info!("Starting shasta gateway on {}", config.bind);

    axum::serve(listener, app(AppState::new(config))).await?;
    Ok(())
}
