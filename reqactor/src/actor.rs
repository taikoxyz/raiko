use std::{
    collections::{HashMap, BinaryHeap},
    ops::DerefMut,
    sync::{
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        Arc, Mutex,
    },
    cmp::Ordering,
};

use raiko_ballot::Ballot;
use raiko_core::interfaces::ProofRequestOpt;
use raiko_lib::{
    consts::{ChainSpec, SupportedChainSpecs},
    proof_type::ProofType,
};
use raiko_reqpool::{Pool, RequestKey, StatusWithContext};
use reth_primitives::BlockHash;
use tokio::sync::{mpsc::Sender, oneshot};

use crate::Action;
#[derive(Debug)]
struct PrioritizedAction {
    priority: u32,
    action: Action,
    resp_tx: oneshot::Sender<Result<StatusWithContext, String>>,
}

impl Ord for PrioritizedAction {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority comes first
        other.priority.cmp(&self.priority)
    }
}

impl PartialOrd for PrioritizedAction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for PrioritizedAction {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Eq for PrioritizedAction {}

/// Actor is the main interface interacting with the backend and the pool.
#[derive(Debug, Clone)]
pub struct Actor {
    default_request_config: ProofRequestOpt,
    chain_specs: SupportedChainSpecs,
    action_tx: Sender<(Action, oneshot::Sender<Result<StatusWithContext, String>>)>,
    pause_tx: Sender<()>,
    is_paused: Arc<AtomicBool>,

    // TODO: Remove Mutex. currently, in order to pass `&mut Pool`, we need to use Arc<Mutex<Pool>>.
    pool: Arc<Mutex<Pool>>,
    // In order to support dynamic config via HTTP, we need to use Arc<Mutex<Ballot>>.
    ballot: Arc<Mutex<Ballot>>,
    // Priority queue for actions
    priority_queue: Arc<Mutex<BinaryHeap<PrioritizedAction>>>,
}

impl Actor {
    pub fn new(
        pool: Pool,
        ballot: Ballot,
        default_request_config: ProofRequestOpt,
        chain_specs: SupportedChainSpecs,
        action_tx: Sender<(Action, oneshot::Sender<Result<StatusWithContext, String>>)>,
        pause_tx: Sender<()>,
    ) -> Self {
        Self {
            default_request_config,
            chain_specs,
            action_tx,
            pause_tx,
            is_paused: Arc::new(AtomicBool::new(false)),
            ballot: Arc::new(Mutex::new(ballot)),
            pool: Arc::new(Mutex::new(pool)),
            priority_queue: Arc::new(Mutex::new(BinaryHeap::new())),
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
        self.is_paused.load(AtomicOrdering::SeqCst)
    }

    /// Get the status of the request from the pool.
    pub fn pool_get_status(
        &self,
        request_key: &RequestKey,
    ) -> Result<Option<StatusWithContext>, String> {
        self.pool.lock().unwrap().get_status(request_key)
    }

    pub fn pool_list_status(&self) -> Result<HashMap<RequestKey, StatusWithContext>, String> {
        self.pool.lock().unwrap().list()
    }

    pub fn pool_remove_request(&self, request_key: &RequestKey) -> Result<usize, String> {
        self.pool.lock().unwrap().remove(request_key)
    }

    /// Queue an action with priority, then call `act`.
    ///
    /// This function allows you to enqueue an action with a given priority.
    /// Higher priority actions should be processed before lower priority ones.
    ///
    /// # Arguments
    /// * `action` - The action to queue.
    /// * `priority` - The priority of the action (higher is more important).
    ///
    /// # Returns
    /// * `Result<StatusWithContext, String>` - The result of processing the action.
    pub async fn queue(&self, action: Action, priority: u32) -> Result<StatusWithContext, String> {
        let (resp_tx, resp_rx) = oneshot::channel();
        
        // Create prioritized action
        let prioritized_action = PrioritizedAction {
            priority,
            action,
            resp_tx,
        };

        // Always add to priority queue first
        {
            let mut queue = self.priority_queue.lock().unwrap();
            queue.push(prioritized_action);
        }

        // Try to process the highest priority action
        if let Some(next_action) = self.priority_queue.lock().unwrap().pop() {
            // Try to send to channel
            match self.action_tx.try_send((next_action.action.clone(), next_action.resp_tx)) {
                Ok(_) => {
                    tracing::info!("Action sent with priority {}", priority);
                }
                Err(e) => {
                    // If channel is full or closed, create a new action and put it back in the queue
                    tracing::info!("Channel error, action with priority {} will be processed later: {}", priority, e);
                    let (new_resp_tx, _) = oneshot::channel();
                    let new_action = PrioritizedAction {
                        priority: next_action.priority,
                        action: next_action.action,
                        resp_tx: new_resp_tx,
                    };
                    self.priority_queue.lock().unwrap().push(new_action);
                }
            }
        }
        
        // Wait for response
        resp_rx.await.map_err(|e| format!("failed to receive action response: {e}"))?
    }

    /// Send an action to the backend and wait for the response.
    pub async fn act(&self, action: Action) -> Result<StatusWithContext, String> {
        let (resp_tx, resp_rx) = oneshot::channel();

        // Send the action to the backend
        self.action_tx
            .send((action, resp_tx))
            .await
            .map_err(|e| format!("failed to send action: {e}"))?;

        // Wait for response of the action
        resp_rx
            .await
            .map_err(|e| format!("failed to receive action response: {e}"))?
    }

    /// Set the pause flag and notify the task manager to pause, then wait for the task manager to
    /// finish the pause process.
    ///
    /// Note that this function is blocking until the task manager finishes the pause process.
    pub async fn pause(&self) -> Result<(), String> {
        self.is_paused.store(true, AtomicOrdering::SeqCst);
        self.pause_tx
            .send(())
            .await
            .map_err(|e| format!("failed to send pause signal: {e}"))?;
        Ok(())
    }

    pub fn get_ballot(&self) -> Ballot {
        self.ballot.lock().unwrap().clone()
    }

    pub fn is_ballot_disabled(&self) -> bool {
        self.ballot.lock().unwrap().probabilities().is_empty()
    }

    pub fn set_ballot(&self, new_ballot: Ballot) {
        let mut ballot = self.ballot.lock().unwrap();
        *ballot = new_ballot;
    }

    /// Draw proof types based on the block hash.
    pub fn draw(&self, block_hash: &BlockHash) -> Option<ProofType> {
        self.ballot
            .lock()
            .unwrap()
            .deref_mut()
            .draw_with_poisson(block_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;
    use raiko_lib::{
        consts::SupportedChainSpecs,
        input::BlobProofType,
        primitives::{ChainId, B256},
        proof_type::ProofType,
    };
    use raiko_reqpool::{
        memory_pool, RequestEntity, RequestKey, SingleProofRequestEntity, SingleProofRequestKey,
        StatusWithContext,
    };
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_pause_sets_is_paused_flag() {
        let (action_tx, _) = mpsc::channel(1);
        let (pause_tx, _pause_rx) = mpsc::channel(1);

        let pool = memory_pool("test_pause_sets_is_paused_flag");
        let actor = Actor::new(
            pool,
            Ballot::default(),
            ProofRequestOpt::default(),
            SupportedChainSpecs::default(),
            action_tx,
            pause_tx,
        );

        assert!(!actor.is_paused(), "Actor should not be paused initially");

        actor.pause().await.expect("Pause should succeed");
        assert!(
            actor.is_paused(),
            "Actor should be paused after calling pause()"
        );
    }

    #[tokio::test]
    async fn test_act_sends_action_and_returns_response() {
        let (action_tx, mut action_rx) = mpsc::channel(1);
        let (pause_tx, _) = mpsc::channel(1);

        let pool = memory_pool("test_act_sends_action_and_returns_response");
        let actor = Actor::new(
            pool,
            Ballot::default(),
            ProofRequestOpt::default(),
            SupportedChainSpecs::default(),
            action_tx,
            pause_tx,
        );

        // Create a test action
        let request_key = RequestKey::SingleProof(SingleProofRequestKey::new(
            ChainId::default(),
            1,
            B256::default(),
            ProofType::default(),
            "test_prover".to_string(),
        ));
        let request_entity = RequestEntity::SingleProof(SingleProofRequestEntity::new(
            1,
            1,
            "test_network".to_string(),
            "test_l1_network".to_string(),
            B256::default(),
            Address::default(),
            ProofType::default(),
            BlobProofType::default(),
            HashMap::new(),
        ));
        let test_action = Action::Prove {
            request_key: request_key.clone(),
            request_entity,
        };

        // Spawn a task to handle the action and send back a response
        let status = StatusWithContext::new_registered();
        let status_clone = status.clone();
        let handle = tokio::spawn(async move {
            let (action, resp_tx) = action_rx.recv().await.expect("Should receive action");
            // Verify we received the expected action
            assert_eq!(action.request_key(), &request_key);
            // Send back a mock response with Registered status
            resp_tx
                .send(Ok(status_clone))
                .expect("Should send response");
        });

        // Send the action and wait for response
        let result = actor.act(test_action).await;

        // Make sure we got back an Ok response
        assert_eq!(result, Ok(status), "Should receive successful response");

        // Wait for the handler to complete
        handle.await.expect("Handler should complete");
    }
}
