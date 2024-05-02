use std::{net::SocketAddr, str::FromStr};

use anyhow::Context;
use tokio::net::TcpListener;
use tracing::debug;

use crate::{error::HostError, server::api::create_router, ProverState};

pub mod api;

/// Starts the proverd server.
pub async fn serve(state: ProverState) -> anyhow::Result<()> {
    let addr = SocketAddr::from_str(&state.opts.address)
        .map_err(|_| HostError::InvalidAddress(state.opts.address.clone()))?;
    let listener = TcpListener::bind(addr).await?;

    debug!("Listening on: {}", listener.local_addr()?);

    let router = create_router(state.opts.concurrency_limit).with_state(state);
    axum::serve(listener, router)
        .await
        .context("Server couldn't serve")?;

    Ok(())
}
