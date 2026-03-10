use clap::Parser;
use raiko_gateway::{app, AppState, config::Cli, Config};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = Config::load(&cli.config)?;
    let listener = TcpListener::bind(&config.bind).await?;

    tracing::info!("Starting gateway on {}", config.bind);

    axum::serve(listener, app(AppState::new(config))).await?;
    Ok(())
}
