use axum::routing::post;
use axum::Router;

pub mod proof;
pub mod types;

// Re-export reusing interfaces and types
pub use super::v1::health;
pub use super::v1::metrics;
pub use super::v2::proof::list;
pub use super::v2::proof::prune;
pub use super::v2::proof::report;
pub use super::v2::Status;
pub use super::v3::proof::aggregate;
pub use super::v3::proof::cancel;
pub use super::v3::CancelStatus;
pub use super::v3::ProofResponse;
pub use super::v3::PruneStatus;
pub use crate::interfaces::HostResult;

pub fn create_router() -> Router<raiko_reqactor::Actor> {
    Router::new()
        .nest(
            "/proof",
            Router::new()
                .route("/", post(proof::proof_handler))
                .nest("/cancel", cancel::create_router())
                .nest("/aggregate", aggregate::create_router())
                .nest("/report", report::create_router())
                .nest("/list", list::create_router())
                .nest("/prune", prune::create_router()),
        )
        .nest("/health", health::create_router())
        .nest("/metrics", metrics::create_router())
}
