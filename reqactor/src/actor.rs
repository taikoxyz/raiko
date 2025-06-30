use std::{
    collections::HashMap,
    // ops::DerefMut,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use raiko_ballot::Ballot;
use raiko_core::interfaces::ProofRequestOpt;
use raiko_lib::{
    consts::{ChainSpec, SupportedChainSpecs},
    proof_type::ProofType,
};
use raiko_reqpool::{Pool, RequestEntity, RequestKey, Status, StatusWithContext};
use reth_primitives::BlockHash;
use tokio::sync::{Mutex, Notify};

use crate::queue::Queue;

/// Actor is the main interface interacting with the backend and the pool.
#[derive(Debug, Clone)]
pub struct Actor {
    default_request_config: ProofRequestOpt,
    chain_specs: SupportedChainSpecs,
    is_paused: Arc<AtomicBool>,

    // TODO: Remove Mutex. currently, in order to pass `&mut Pool`, we need to use Arc<Mutex<Pool>>.
    pool: Arc<Mutex<Pool>>,
    // In order to support dynamic config via HTTP, we need to use Arc<Mutex<Ballot>>.
    ballot: Arc<Mutex<Ballot>>,
    queue: Arc<Mutex<Queue>>,
    notify: Arc<Notify>,
}

impl Actor {
    pub fn new(
        pool: Pool,
        ballot: Ballot,
        default_request_config: ProofRequestOpt,
        chain_specs: SupportedChainSpecs,
        queue: Arc<Mutex<Queue>>,
        notify: Arc<Notify>,
    ) -> Self {
        Self {
            default_request_config,
            chain_specs,
            is_paused: Arc::new(AtomicBool::new(false)),
            ballot: Arc::new(Mutex::new(ballot)),
            pool: Arc::new(Mutex::new(pool)),
            queue,
            notify,
        }
    }

    /// Return the default request config.
    pub fn default_request_config(&self) -> &ProofRequestOpt {
        &self.default_request_config
    }

    /// Return the chain specs.
    pub fn chain_specs(&self) -> &SupportedChainSpecs {
        &self.chain_specs
    }

    pub fn get_chain_spec(&self, network: &str) -> Result<ChainSpec, String> {
        self.chain_specs
            .get_chain_spec(network)
            .ok_or_else(|| format!("unsupported network: {}", network))
    }

    /// Check if the system is paused.
    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
    }

    /// Get the status of the request from the pool.
    pub async fn pool_get_status(
        &self,
        request_key: &RequestKey,
    ) -> Result<Option<StatusWithContext>, String> {
        self.pool.lock().await.get_status(request_key)
    }

    pub async fn pool_update_status(
        &self,
        request_key: RequestKey,
        status: StatusWithContext,
    ) -> Result<(), String> {
        self.pool
            .lock()
            .await
            .update_status(request_key, status)
            .map(|_| ())
    }

    pub async fn pool_add_new(
        &self,
        request_key: RequestKey,
        request_entity: RequestEntity,
        status: StatusWithContext,
    ) -> Result<(), String> {
        self.pool
            .lock()
            .await
            .add(request_key, request_entity, status)
    }

    pub async fn pool_list_status(&self) -> Result<HashMap<RequestKey, StatusWithContext>, String> {
        self.pool.lock().await.list()
    }

    pub async fn pool_remove_request(&self, request_key: &RequestKey) -> Result<usize, String> {
        self.pool.lock().await.remove(request_key)
    }

    /// Send an action to the backend and wait for the response.
    pub async fn act(
        &self,
        request_key: RequestKey,
        request_entity: RequestEntity,
        start_time: chrono::DateTime<chrono::Utc>,
    ) -> Result<StatusWithContext, String> {
        let pool_status_opt = self.pool_get_status(&request_key).await?;

        // Return successful status if the request is already succeeded
        if matches!(
            pool_status_opt.as_ref().map(|s| s.status()),
            Some(Status::Success { .. })
        ) {
            return Ok(pool_status_opt.unwrap());
        }

        // Mark the request as registered in the pool
        let status = StatusWithContext::new(Status::Registered, start_time);
        if pool_status_opt.is_none() {
            self.pool_add_new(request_key.clone(), request_entity.clone(), status.clone())
                .await?;
        } else {
            self.pool_update_status(request_key.clone(), status.clone())
                .await?;
        }

        // Push the request into the queue and notify to start the action
        let mut queue = self.queue.lock().await;
        if !queue.contains(&request_key) {
            queue.add_pending(request_key, request_entity);
            self.notify.notify_one();
        }

        return Ok(status);
    }

    pub async fn pause(&self) -> Result<(), String> {
        self.is_paused.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub async fn get_ballot(&self) -> Ballot {
        self.ballot.lock().await.clone()
    }

    pub async fn set_ballot(&self, new_ballot: Ballot) {
        let mut ballot = self.ballot.lock().await;
        *ballot = new_ballot;
    }
    pub async fn is_ballot_disabled(&self) -> bool {
        self.ballot.lock().await.probabilities().is_empty()
    }

    /// Draw proof types based on the block hash.
    pub async fn draw(&self, block_hash: &BlockHash) -> Option<ProofType> {
        self.ballot.lock().await.draw_with_poisson(block_hash)
    }

//     pub fn draw(&self, block_hash: &BlockHash) -> Option<ProofType> {
//         self.ballot
//             .lock()
//             .unwrap()
//             .deref_mut()
//             .draw_with_poisson(block_hash)
//     }
}


