mod remote;

use raiko2_primitives::{AggregationGuestInput, GuestInput, Proof, ProverConfig, ProverResult};

pub use remote::{RemoteSgxProver, SgxParam, SgxResponse};

/// High level SGX prover wrapper that picks the appropriate backend.
#[derive(Clone, Debug)]
pub struct SgxProver {
    inner: RemoteSgxProver,
}

impl SgxProver {
    /// Construct a new SGX prover instance using the remote backend.
    pub fn new() -> Self {
        Self {
            inner: RemoteSgxProver::new(),
        }
    }
}

impl Default for SgxProver {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::Prover for SgxProver {
    async fn prove(&self, input: GuestInput, config: &ProverConfig) -> ProverResult<Proof> {
        self.inner.prove(input, config).await
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        config: &ProverConfig,
    ) -> ProverResult<Proof> {
        self.inner.aggregate(input, config).await
    }
}
