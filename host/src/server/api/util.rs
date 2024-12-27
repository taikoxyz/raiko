use crate::{
    interfaces::{HostError, HostResult},
    ProverState,
};

/// Ensure that the system is not paused, otherwise return an error.
pub fn ensure_not_paused(prover_state: &ProverState) -> HostResult<()> {
    if prover_state.is_paused() {
        return Err(HostError::SystemPaused);
    }
    Ok(())
}
