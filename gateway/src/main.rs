use clap::Parser;
use raiko_gateway::{app, AppState, config::Cli, Config};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .init();

    let cli = Cli::parse();
    let config = Config::load(&cli.config)?;
    let listener = TcpListener::bind(&config.bind).await?;

    let key_count = config.valid_api_keys().len();
    tracing::info!(
        "Starting gateway on {}, API key check: {}",
        config.bind,
        if key_count > 0 {
            format!("enabled ({} keys)", key_count)
        } else {
            "disabled".to_string()
        }
    );

    axum::serve(listener, app(AppState::new(config))).await?;
    Ok(())
}
