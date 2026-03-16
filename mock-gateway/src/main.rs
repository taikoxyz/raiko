use std::env;

use raiko_mock_gateway::{app, AppState};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let bind = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:4000".to_string());
    let listener = TcpListener::bind(&bind).await?;

    tracing::info!("starting mock gateway on {bind}");
    axum::serve(listener, app(AppState::default())).await?;
    Ok(())
}
