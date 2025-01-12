use crate::{interfaces::HostError, server::api::create_router};
use anyhow::Context;
use std::{net::SocketAddr, str::FromStr};
use tokio::net::TcpListener;
use tracing::info;

pub mod api;
pub mod handler;
pub mod utils;

pub use handler::{cancel, cancel_aggregation, prove, prove_aggregation};
pub use utils::{to_v2_cancel_status, to_v2_result, to_v3_cancel_status, to_v3_result};

/// Starts the proverd server.
pub async fn serve<P: raiko_reqpool::Pool + 'static>(
    gateway: raiko_reqactor::Gateway<P>,
    address: &str,
    concurrency_limit: usize,
    jwt_secret: Option<String>,
) -> anyhow::Result<()> {
    let addr = SocketAddr::from_str(address)
        .map_err(|_| HostError::InvalidAddress(address.to_string()))?;
    let listener = TcpListener::bind(addr).await?;

    info!("Listening on: {}", listener.local_addr()?);

    let router = create_router(concurrency_limit, jwt_secret.as_deref()).with_state(gateway);
    axum::serve(listener, router)
        .await
        .context("Server couldn't serve")?;

    Ok(())
}
