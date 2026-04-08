use alloy_provider::RootProvider;
use alloy_rpc_client::{ClientBuilder, RpcClient};
use alloy_rpc_types::{Block, BlockNumberOrTag};
pub use alloy_rpc_types_debug::ExecutionWitness;
use std::{future::Future, time::Duration};
use tokio::time::sleep;
use tracing::{debug, info, trace};

use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::BlockDataProvider,
};

#[derive(Clone, Debug)]
pub struct RpcBlockDataProvider {
    pub provider: RootProvider,
    pub client: RpcClient,
}

impl RpcBlockDataProvider {
    /// async will be used for future preflight optimization
    pub async fn new(url: &str) -> RaikoResult<Self> {
        let url =
            reqwest::Url::parse(url).map_err(|_| RaikoError::RPC("Invalid RPC URL".to_owned()))?;
        debug!("provider rpc url: {:?}", url);
        Ok(Self {
            provider: RootProvider::new_http(url.clone()),
            client: ClientBuilder::default().http(url),
        })
    }

    pub fn provider(&self) -> &RootProvider {
        &self.provider
    }

    async fn construct_and_send_batch(
        &self,
        blocks_to_fetch: &[(u64, bool)],
        max_batch_size: usize,
    ) -> RaikoResult<Vec<Block>> {
        let mut batch = self.client.new_batch();
        let mut requests = Vec::with_capacity(max_batch_size);

        for (block_number, full) in blocks_to_fetch {
            requests.push(Box::pin(
                batch
                    .add_call(
                        "eth_getBlockByNumber",
                        &(BlockNumberOrTag::from(*block_number), full),
                    )
                    .map_err(|_| {
                        RaikoError::RPC(
                            "Failed adding eth_getBlockByNumber call to batch".to_owned(),
                        )
                    })?,
            ));
        }

        batch.send().await.map_err(|e| {
            RaikoError::RPC(format!(
                "Error sending batch request for block {blocks_to_fetch:?}: {e}"
            ))
        })?;

        let mut blocks = Vec::with_capacity(max_batch_size);
        // Collect the data from the batch
        for request in requests {
            blocks.push(
                request
                    .await
                    .map_err(|e| RaikoError::RPC(format!("Error collecting request data: {e}")))?,
            );
        }

        Ok(blocks)
    }
}

const MAX_RETRIES: u32 = 3;
const INITIAL_DELAY: Duration = Duration::from_secs(1);

impl BlockDataProvider for RpcBlockDataProvider {
    async fn get_blocks(&self, blocks_to_fetch: &[(u64, bool)]) -> RaikoResult<Vec<Block>> {
        let mut all_blocks = Vec::with_capacity(blocks_to_fetch.len());

        let max_batch_size = 32;
        for blocks_to_fetch in blocks_to_fetch.chunks(max_batch_size) {
            let mut blocks = retry_in_case_of_error(
                || self.construct_and_send_batch(blocks_to_fetch, max_batch_size),
                MAX_RETRIES,
                INITIAL_DELAY,
            )
            .await?;
            all_blocks.append(&mut blocks);
        }

        Ok(all_blocks)
    }

    async fn execution_witness(&self, block_number: u64) -> Option<RaikoResult<ExecutionWitness>> {
        Some(RpcBlockDataProvider::execution_witness(self, block_number).await)
    }
}

impl RpcBlockDataProvider {
    /// Fetch the execution witness for a block via debug_executionWitness.
    /// Returns the complete witness needed for stateless block re-execution:
    /// all MPT node preimages, contract codes, key preimages, and block headers.
    pub async fn execution_witness(&self, block_number: u64) -> RaikoResult<ExecutionWitness> {
        let block_id = BlockNumberOrTag::from(block_number);

        let witness: ExecutionWitness = self
            .client
            .request("debug_executionWitness", (block_id,))
            .await
            .map_err(|e| RaikoError::RPC(format!("debug_executionWitness failed: {e}")))?;

        info!(
            "debug_executionWitness: {} state nodes, {} codes, {} keys, {} headers",
            witness.state.len(),
            witness.codes.len(),
            witness.keys.len(),
            witness.headers.len(),
        );

        Ok(witness)
    }
}

async fn retry_in_case_of_error<F, Fut, T>(
    operation: F,
    max_retries: u32,
    initial_delay: Duration,
) -> RaikoResult<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = RaikoResult<T>> + Send,
{
    let mut delay = initial_delay;
    let mut last_error = RaikoError::RPC("".to_owned());

    for attempt in 1..=max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = e;
                trace!(
                    "Batch request failed (attempt {}/{}), retrying in {:?}...",
                    attempt,
                    max_retries,
                    delay
                );
                sleep(delay).await;
                delay *= 2; // Exponential backoff
            }
        }
    }

    Err(RaikoError::RPC(format!(
        "Failed to send batch after {max_retries} attempts: {}",
        last_error
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn test_retry_in_case_of_error() {
        let attempt_counter = AtomicU32::new(0);
        let operation = || async {
            let attempt = attempt_counter.fetch_add(1, Ordering::SeqCst);
            if attempt < 2 {
                Err(RaikoError::RPC("Simulated failure".to_owned()))
            } else {
                Ok("success")
            }
        };

        let result = retry_in_case_of_error(
            operation,
            3,
            Duration::from_millis(1), // Short delay for tests
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 3);
        assert_eq!(result.unwrap(), "success");

        // Test max retries exceeded
        let attempt_counter = AtomicU32::new(0);
        let failing_operation = || async {
            attempt_counter.fetch_add(1, Ordering::SeqCst);
            Err(RaikoError::RPC("Always fails".to_owned()))
        };

        let result: RaikoResult<()> =
            retry_in_case_of_error(failing_operation, 2, Duration::from_millis(1)).await;

        assert!(result.is_err());
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 2);
        assert!(matches!(result, Err(RaikoError::RPC(_))));
    }
}
