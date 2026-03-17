use std::env;

use raiko_mock_gateway::{app, gateway_bind_from_args, AppState};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let bind = gateway_bind_from_args(env::args())?;
    let listener = TcpListener::bind(&bind).await?;

    tracing::info!("starting mock gateway on {bind}");
    axum::serve(listener, app(AppState::default())).await?;
    Ok(())
}
