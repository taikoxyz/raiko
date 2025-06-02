use crate::{
    methods::risc0_aggregation::RISC0_AGGREGATION_ELF, methods::risc0_batch::RISC0_BATCH_ELF,
    methods::risc0_guest::RISC0_GUEST_ELF,
};
use alloy_signer_local::PrivateKeySigner;
use anyhow::ensure;
use boundless_market::{
    client::ClientBuilder,
    contracts::{Input, Offer, Predicate, ProofRequestBuilder, Requirements},
    input::InputBuilder,
    storage::storage_provider_from_env,
};
use raiko_lib::primitives::address;
use raiko_lib::primitives::utils::parse_ether;
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ZkAggregationGuestInput,
    },
    proof_type::ProofType,
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use reqwest::Url;
use risc0_zkvm::{
    compute_image_id, default_executor, default_prover,
    serde::to_vec,
    sha::{Digest, Digestible},
    ExecutorEnv, ProverOpts, Receipt,
};
use std::time::Duration;

mod helper;
mod ipfs_utils;

pub struct Risc0BoundlessProver;

impl Risc0BoundlessProver {
    pub fn new() -> Self {
        Self {}
    }
}

impl Prover for Risc0BoundlessProver {
    async fn run(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        unimplemented!("No need for post pacaya");
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        todo!()
    }

    async fn cancel(&self, key: ProofKey, id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        todo!()
    }

    /*
    export RPC_URL="https://ethereum-sepolia-rpc.publicnode.com"
    export PRIVATE_KEY="0x6935664027d5c9ad52696da5982e0b7ce6ee7d5eb479825d608a8154f74f2507"
    export BOUNDLESS_MARKET_ADDRESS="0x006b92674E2A8d397884e293969f8eCD9f615f4C"
    export VERIFIER_ADDRESS="0x925d8331ddc0a1F0d96E68CF073DFE1d92b69187"
    export SET_VERIFIER_ADDRESS="0xad2c6335191EA71Ffe2045A8d54b93A851ceca77"
    export ORDER_STREAM_URL="https://eth-sepolia.beboundless.xyz/"
     */
    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        // Create a Boundless client from the provided parameters.
        // let args = helper::Args::parse();
        let url = Url::parse("https://ethereum-sepolia-rpc.publicnode.com").unwrap();
        let order_stream_url = Url::parse("https://eth-sepolia.beboundless.xyz/").ok();
        let signer = PrivateKeySigner::from_str(
            "0x6935664027d5c9ad52696da5982e0b7ce6ee7d5eb479825d608a8154f74f2507",
        );
        let boundless_client = ClientBuilder::new()
            .with_rpc_url(url)
            .with_boundless_market_address(address!("006b92674E2A8d397884e293969f8eCD9f615f4C"))
            .with_set_verifier_address(address!("ad2c6335191EA71Ffe2045A8d54b93A851ceca77"))
            .with_order_stream_url(order_stream_url)
            // .with_storage_provider_config(args.storage_config).await?
            .with_storage_provider(Some(
                storage_provider_from_env()
                    .await
                    .expect("Failed to create storage provider from environment"),
            ))
            .with_private_key(signer)
            .build()
            .await
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to create Boundless client: {e}"))
            })?;

        // Upload the ELF to the storage provider so that it can be fetched by the market.
        assert!(
            boundless_client.storage_provider.is_some(),
            "a storage provider is required to upload the zkVM guest ELF"
        );
        let image_url = boundless_client
            .upload_image(RISC0_BATCH_ELF)
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to upload image: {e}")))?;
        tracing::info!("Uploaded image to {}", image_url);

        // Encode the input and upload it to the storage provider.
        let encoded_input = to_vec(&input).expect("Could not serialize proving input!");
        let input_builder = InputBuilder::new().write_slice(&encoded_input);
        tracing::info!("input builder: {:?}", input_builder);

        let guest_env = input_builder.clone().build_env().map_err(|e| {
            ProverError::GuestError(format!("Failed to build guest environment: {e}"))
        })?;
        let guest_env_bytes = guest_env.encode().map_err(|e| {
            ProverError::GuestError(format!("Failed to encode guest environment: {e}"))
        })?;

        // Dry run the ELF with the input to get the journal and cycle count.
        // This can be useful to estimate the cost of the proving request.
        // It can also be useful to ensure the guest can be executed correctly and we do not send into
        // the market unprovable proving requests. If you have a different mechanism to get the expected
        // journal and set a price, you can skip this step.
        let session_info = default_executor()
            .execute(guest_env.try_into().unwrap(), RISC0_BATCH_ELF)
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to execute guest environment: {e}"))
            })?;
        let mcycles_count = session_info
            .segments
            .iter()
            .map(|segment| 1 << segment.po2)
            .sum::<u64>()
            .div_ceil(1_000_000);
        let journal = session_info.journal;

        // Create a proof request with the image, input, requirements and offer.
        // The ELF (i.e. image) is specified by the image URL.
        // The input can be specified by an URL, as in this example, or can be posted on chain by using
        // the `with_inline` method with the input bytes.
        // The requirements are the image ID and the digest of the journal. In this way, the market can
        // verify that the proof is correct by checking both the committed image id and digest of the
        // journal. The offer specifies the price range and the timeout for the request.
        // Additionally, the offer can also specify:
        // - the bidding start time: the block number when the bidding starts;
        // - the ramp up period: the number of blocks before the price start increasing until reaches
        //   the maxPrice, starting from the the bidding start;
        // - the lockin price: the price at which the request can be locked in by a prover, if the
        //   request is not fulfilled before the timeout, the prover can be slashed.
        // If the input exceeds 2 kB, upload the input and provide its URL instead, as a rule of thumb.
        let request_input = if guest_env_bytes.len() > 2 << 10 {
            let input_url = boundless_client
                .upload_input(&guest_env_bytes)
                .await
                .map_err(|e| ProverError::GuestError(format!("Failed to upload input: {e}")))?;
            tracing::info!("Uploaded input to {}", input_url);
            Input::url(input_url)
        } else {
            tracing::info!("Sending input inline with request");
            Input::inline(guest_env_bytes.clone())
        };

        let request = ProofRequestBuilder::new()
            .with_image_url(image_url.to_string())
            .with_input(request_input)
            .with_requirements(Requirements::new(
                compute_image_id(RISC0_BATCH_ELF).unwrap(),
                Predicate::digest_match(journal.digest()),
            ))
            .with_offer(
                Offer::default()
                    // The market uses a reverse Dutch auction mechanism to match requests with provers.
                    // Each request has a price range that a prover can bid on. One way to set the price
                    // is to choose a desired (min and max) price per million cycles and multiply it
                    // by the number of cycles. Alternatively, you can use the `with_min_price` and
                    // `with_max_price` methods to set the price directly.
                    .with_min_price_per_mcycle(
                        parse_ether("0.001").unwrap_or_default(),
                        mcycles_count,
                    )
                    // NOTE: If your offer is not being accepted, try increasing the max price.
                    .with_max_price_per_mcycle(
                        parse_ether("0.05").unwrap_or_default(),
                        mcycles_count,
                    )
                    // The timeout is the maximum number of blocks the request can stay
                    // unfulfilled in the market before it expires. If a prover locks in
                    // the request and does not fulfill it before the timeout, the prover can be
                    // slashed.
                    .with_timeout(1000)
                    .with_lock_timeout(500)
                    .with_ramp_up_period(100),
            )
            .build()
            .unwrap();

        let offchain = false;
        // Send the request and wait for it to be completed.
        let (request_id, expires_at) = if offchain {
            boundless_client
                .submit_request_offchain(&request)
                .await
                .map_err(|e| {
                    ProverError::GuestError(format!("Failed to submit request offchain: {e}"))
                })?
        } else {
            boundless_client
                .submit_request(&request)
                .await
                .map_err(|e| {
                    ProverError::GuestError(format!("Failed to submit request onchain: {e}"))
                })?
        };
        tracing::info!("Request 0x{request_id:x} submitted");

        // Wait for the request to be fulfilled by the market, returning the journal and seal.
        tracing::info!("Waiting for 0x{request_id:x} to be fulfilled");
        let (_journal, seal) = boundless_client
            .wait_for_request_fulfillment(request_id, Duration::from_secs(5), expires_at)
            .await
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to wait for request fulfillment: {e}"))
            })?;
        tracing::info!("Request 0x{request_id:x} fulfilled");

        Ok(Proof::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raiko_lib::input::GuestBatchOutput;
    use raiko_lib::primitives::U256;

    #[tokio::test]
    async fn test_batch_run() {
        let prover = Risc0BoundlessProver::new();
    }
}
