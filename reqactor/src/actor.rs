use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use raiko_core::interfaces::ProofRequestOpt;
use raiko_lib::consts::SupportedChainSpecs;
use raiko_reqpool::{Pool, RequestKey, StatusWithContext};
use tokio::sync::{mpsc::Sender, oneshot};

use crate::Action;

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
}

impl Actor {
    pub fn new(
        pool: Pool,
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
            pool: Arc::new(Mutex::new(pool)),
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

    /// Check if the system is paused.
    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
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
        self.is_paused.store(true, Ordering::SeqCst);
        self.pause_tx
            .send(())
            .await
            .map_err(|e| format!("failed to send pause signal: {e}"))?;
        Ok(())
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
        memory_pool, Pool, RequestEntity, RequestKey, SingleProofRequestEntity,
        SingleProofRequestKey, StatusWithContext,
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
