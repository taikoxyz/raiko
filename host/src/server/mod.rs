use crate::{interfaces::HostError, server::api::create_router, ProverState};
use anyhow::Context;
use std::{net::SocketAddr, str::FromStr};
use tokio::net::TcpListener;
use tracing::info;

pub mod api;

/// Starts the proverd server.
pub async fn serve(state: ProverState) -> anyhow::Result<()> {
    #[cfg(feature = "sp1")]
    if let Some(orchestrator_addr) = state.opts.sp1_orchestrator_address.as_ref() {
        sp1_driver::serve_worker(
            state.opts.sp1_worker_address.clone(),
            orchestrator_addr.clone(),
        )
        .await;
    }

    let addr = SocketAddr::from_str(&state.opts.address)
        .map_err(|_| HostError::InvalidAddress(state.opts.address.clone()))?;
    let listener = TcpListener::bind(addr).await?;

    info!("Listening on: {}", listener.local_addr()?);

    let router = create_router(
        state.opts.concurrency_limit,
        state.opts.jwt_secret.as_deref(),
    )
    .with_state(state);
    axum::serve(listener, router)
        .await
        .context("Server couldn't serve")?;

    Ok(())
}
