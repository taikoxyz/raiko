//! HTTP API server for Raiko V2.

mod handlers;
mod routes;
mod state;

use crate::config::Config;
use anyhow::Result;
use axum::Router;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

pub use state::AppState;

/// Run the HTTP server.
pub async fn run_server(config: Config) -> Result<()> {
    // Create application state
    let state = AppState::new(config.clone())?;

    // Build router
    let app = Router::new()
        .merge(routes::api_routes())
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    // Bind to address
    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.port));
    let listener = TcpListener::bind(addr).await?;

    info!("Server listening on http://{}", addr);

    // Run server
    axum::serve(listener, app).await?;

    Ok(())
}
