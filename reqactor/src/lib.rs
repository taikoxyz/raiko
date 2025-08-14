mod action;
mod actor;
mod backend;
mod queue;

use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

use backend::Backend;
use queue::Queue;
use raiko_ballot::Ballot;
use raiko_core::interfaces::ProofRequestOpt;
use raiko_lib::consts::SupportedChainSpecs;

// re-export
pub use action::Action;
pub use actor::Actor;
pub use raiko_reqpool::{
    AggregationRequestEntity, AggregationRequestKey, Pool, RequestEntity, RequestKey,
    SingleProofRequestEntity, SingleProofRequestKey, StatusWithContext,
};

/// Run the actor backend in background, and return the actor.
pub async fn start_actor(
    pool: Pool,
    ballot: Ballot,
    chain_specs: SupportedChainSpecs,
    default_request_config: ProofRequestOpt,
    max_proving_concurrency: usize,
    max_queue_size: usize,
) -> Actor {
    let queue = Arc::new(Mutex::new(Queue::new(max_queue_size)));
    let notify = Arc::new(Notify::new());
    let actor = Actor::new(
        pool.clone(),
        ballot,
        default_request_config,
        chain_specs.clone(),
        Arc::clone(&queue),
        Arc::clone(&notify),
    );
    let backend = Backend::new(
        pool,
        chain_specs,
        max_proving_concurrency,
        Arc::clone(&queue),
        Arc::clone(&notify),
    );
    let _ = tokio::spawn(async move {
        backend.serve_in_background().await;
    });
    actor
}
