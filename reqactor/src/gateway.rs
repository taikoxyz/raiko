use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use raiko_core::interfaces::ProofRequestOpt;
use raiko_lib::consts::SupportedChainSpecs;
use raiko_reqpool::{Pool, RequestKey, StatusWithContext};
use tokio::sync::{mpsc::Sender, oneshot};

use crate::Action;

#[derive(Debug, Clone)]
pub struct Gateway {
    default_request_config: ProofRequestOpt,
    chain_specs: SupportedChainSpecs,
    controller: Sender<(Action, oneshot::Sender<Result<StatusWithContext, String>>)>,
    pause_signal: Sender<()>,
    pool: Arc<Mutex<Pool>>,
    is_paused: Arc<AtomicBool>,
}

/// Gateway for the Actor.
impl Gateway {
    pub fn new(
        pool: Pool,
        default_request_config: ProofRequestOpt,
        chain_specs: SupportedChainSpecs,
        controller: Sender<(Action, oneshot::Sender<Result<StatusWithContext, String>>)>,
        pause_signal: Sender<()>,
    ) -> Self {
        Self {
            default_request_config,
            chain_specs,
            controller,
            pause_signal,
            pool: Arc::new(Mutex::new(pool)),
            is_paused: Arc::new(AtomicBool::new(false)),
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

    /// Send an action to the controller and wait for the response.
    pub async fn send(&self, action: Action) -> Result<StatusWithContext, String> {
        let (resp_tx, resp_rx) = oneshot::channel();

        // Send the action to the controller
        self.controller
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
        self.pause_signal
            .send(())
            .await
            .map_err(|e| format!("failed to send pause signal: {e}"))?;
        Ok(())
    }
}
