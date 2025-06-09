use crate::{
    methods::{
        boundless_aggregation::BOUNDLESS_AGGREGATION_ELF, risc0_batch::RISC0_BATCH_ELF,
        risc0_batch::RISC0_BATCH_ID,
    },
    snarks::verify_boundless_groth16_snark_impl,
    Risc0Response,
};
use alloy_primitives::B256;
use alloy_signer_local_v12::PrivateKeySigner;
use alloy_sol_types::SolValue;
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
        GuestInput, GuestOutput,
    },
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use reqwest::Url;
use risc0_ethereum_contracts_boundless::receipt::Receipt as ContractReceipt;
use risc0_zkvm::{
    compute_image_id, default_executor, serde::to_vec, sha::Digestible, Digest,
    Receipt as ZkvmReceipt,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundlessAggregationGuestInput {
    pub image_id: Digest,
    pub receipts: Vec<ZkvmReceipt>,
}

use tokio::sync::OnceCell;

static RISCV_PROVER: OnceCell<Risc0BoundlessProver> = OnceCell::const_new();

pub async fn get_boundless_prover() -> &'static Risc0BoundlessProver {
    RISCV_PROVER
        .get_or_init(|| async {
            Risc0BoundlessProver::init_prover()
                .await
                .expect("Failed to initialize Boundless client")
        })
        .await
}

pub struct Risc0BoundlessProver {
    batch_image_url: Option<Url>,
    aggregation_image_url: Option<Url>,
}

impl Risc0BoundlessProver {
    pub async fn get() -> &'static Self {
        get_boundless_prover().await
    }

    async fn init_prover() -> Result<Self, ProverError> {
        let boundless_client = {
            // Create a Boundless client from the provided parameters.
            // let args = helper::Args::parse();
            let url = Url::parse("https://ethereum-sepolia-rpc.publicnode.com").unwrap();
            let order_stream_url = Url::parse("https://eth-sepolia.beboundless.xyz/").ok();
            let sender_priv_key = std::env::var("BOUNDLESS_SIGNER_KEY").unwrap_or_else(|_| {
                panic!("BOUNDLESS_PRIVATE_KEY is not set");
            });
            let signer: PrivateKeySigner = sender_priv_key.parse().unwrap();

            // Create a Boundless client from the provided parameters.
            ClientBuilder::new()
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
                })?
        };

        // Upload the ELF to the storage provider so that it can be fetched by the market.
        assert!(
            boundless_client.storage_provider.is_some(),
            "a storage provider is required to upload the zkVM guest ELF"
        );

        let batch_image_url = boundless_client
            .upload_image(RISC0_BATCH_ELF)
            .await
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to upload RISC0_BATCH_ELF image: {e}"))
            })?;

        let aggregation_image_url = boundless_client
            .upload_image(BOUNDLESS_AGGREGATION_ELF)
            .await
            .map_err(|e| {
                ProverError::GuestError(format!(
                    "Failed to upload BOUNDLESS_AGGREGATION_ELF image: {e}"
                ))
            })?;

        Ok(Risc0BoundlessProver {
            batch_image_url: Some(batch_image_url),
            aggregation_image_url: Some(aggregation_image_url),
        })
    }

    async fn get_batch_image_url(&self) -> Option<Url> {
        self.batch_image_url.clone()
    }

    async fn get_aggregation_image_url(&self) -> Option<Url> {
        self.aggregation_image_url.clone()
    }
}

impl Prover for Risc0BoundlessProver {
    async fn run(
        &self,
        _input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
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
        // Encode the input and upload it to the storage provider.
        // let encoded_input = to_vec(&input).expect("Could not serialize proving input!");
        let mut receipts = Vec::new();
        for proof in input.proofs {
            let receipt: ZkvmReceipt =
                serde_json::from_str::<ZkvmReceipt>(proof.quote.as_ref().unwrap()).unwrap();
            receipts.push(receipt);
        }
        // let agg_guest_input = (Digest::from(RISC0_BATCH_ID), receipts);
        let agg_guest_input = BoundlessAggregationGuestInput {
            image_id: Digest::from(RISC0_BATCH_ID),
            receipts,
        };

        tracing::info!("process agg_guest_input: {:?}", agg_guest_input);
        let encoded_input = to_vec(&agg_guest_input).expect("Could not serialize proving input!");
        let input_builder = InputBuilder::new().write_slice(&encoded_input);
        tracing::debug!("input builder: {:?}", input_builder);

        let guest_env = input_builder.clone().build_env().map_err(|e| {
            ProverError::GuestError(format!("Failed to build guest environment: {e}"))
        })?;
        let guest_env_bytes = guest_env.encode().map_err(|e| {
            ProverError::GuestError(format!("Failed to encode guest environment: {e}"))
        })?;

        let (mcycles_count, journal) = {
            // Dry run the ELF with the input to get the journal and cycle count.
            // This can be useful to estimate the cost of the proving request.
            // It can also be useful to ensure the guest can be executed correctly and we do not send into
            // the market unprovable proving requests. If you have a different mechanism to get the expected
            // journal and set a price, you can skip this step.
            let session_info = default_executor()
                .execute(guest_env.try_into().unwrap(), BOUNDLESS_AGGREGATION_ELF)
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
            (mcycles_count, journal)
        };
        tracing::info!("mcycles_count: {}", mcycles_count);

        let boundless_client = {
            // Create a Boundless client from the provided parameters.
            // let args = helper::Args::parse();
            let url = Url::parse("https://ethereum-sepolia-rpc.publicnode.com").unwrap();
            let order_stream_url = Url::parse("https://eth-sepolia.beboundless.xyz/").ok();
            let sender_priv_key = std::env::var("BOUNDLESS_SIGNER_KEY").unwrap_or_else(|_| {
                panic!("BOUNDLESS_SIGNER_KEY is not set");
            });
            let signer: PrivateKeySigner = sender_priv_key.parse().unwrap();

            // Create a Boundless client from the provided parameters.
            ClientBuilder::new()
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
                })?
        };

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

        let image_id = compute_image_id(BOUNDLESS_AGGREGATION_ELF)
            .map_err(|e| ProverError::GuestError(format!("Failed to compute image ID: {e}")))?;
        let image_url = self
            .get_aggregation_image_url()
            .await
            .ok_or_else(|| ProverError::GuestError("Failed to get batch image URL".to_string()))?;
        let request = ProofRequestBuilder::new()
            .with_image_url(image_url.to_string())
            .with_input(request_input)
            .with_requirements(
                Requirements::new(image_id, Predicate::digest_match(journal.digest()))
                    .with_groth16_proof(),
            )
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
                    .with_timeout(2000)
                    .with_lock_timeout(1000)
                    .with_ramp_up_period(500),
            )
            .build()
            .unwrap();

        let offchain = config.get("offchain").map_or(false, |v| v == "true");
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
        let (journal, seal) = boundless_client
            .wait_for_request_fulfillment(request_id, Duration::from_secs(5), expires_at)
            .await
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to wait for request fulfillment: {e}"))
            })?;
        tracing::info!("Request 0x{request_id:x} fulfilled");

        let journal_digest = journal.digest();
        let encoded_proof =
            verify_boundless_groth16_snark_impl(image_id, seal.to_vec(), journal_digest)
                .await
                .map_err(|e| {
                    ProverError::GuestError(format!("Failed to verify groth16 snark: {e}"))
                })?;
        let proof: Vec<u8> = (encoded_proof, B256::from_slice(image_id.as_bytes()))
            .abi_encode()
            .iter()
            .skip(32)
            .copied()
            .collect();

        Ok(Proof {
            proof: Some(alloy_primitives::hex::encode_prefixed(proof)),
            input: Some(B256::from_slice(journal_digest.as_bytes())),
            quote: None,
            uuid: None,
            kzg_proof: None,
        })
    }

    async fn cancel(&self, _key: ProofKey, _id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        todo!()
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        // Encode the input and upload it to the storage provider.
        let encoded_input = to_vec(&input).expect("Could not serialize proving input!");
        let input_builder = InputBuilder::new().write_slice(&encoded_input);
        tracing::debug!("input builder: {:?}", input_builder);

        let guest_env = input_builder.clone().build_env().map_err(|e| {
            ProverError::GuestError(format!("Failed to build guest environment: {e}"))
        })?;
        let guest_env_bytes = guest_env.encode().map_err(|e| {
            ProverError::GuestError(format!("Failed to encode guest environment: {e}"))
        })?;

        let (mcycles_count, journal) = {
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
            (mcycles_count, journal)
        };
        tracing::info!("mcycles_count: {}", mcycles_count);

        let boundless_client = {
            // Create a Boundless client from the provided parameters.
            // let args = helper::Args::parse();
            let url = Url::parse("https://ethereum-sepolia-rpc.publicnode.com").unwrap();
            let order_stream_url = Url::parse("https://eth-sepolia.beboundless.xyz/").ok();
            let sender_priv_key = std::env::var("BOUNDLESS_SIGNER_KEY").unwrap_or_else(|_| {
                panic!("BOUNDLESS_PRIVATE_KEY is not set");
            });
            let signer: PrivateKeySigner = sender_priv_key.parse().unwrap();

            // Create a Boundless client from the provided parameters.
            ClientBuilder::new()
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
                })?
        };

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

        let image_id = compute_image_id(RISC0_BATCH_ELF)
            .map_err(|e| ProverError::GuestError(format!("Failed to compute image ID: {e}")))?;
        let image_url = self
            .get_batch_image_url()
            .await
            .ok_or_else(|| ProverError::GuestError("Failed to get batch image URL".to_string()))?;
        let request = ProofRequestBuilder::new()
            .with_image_url(image_url.to_string())
            .with_input(request_input)
            .with_requirements(
                Requirements::new(image_id, Predicate::digest_match(journal.digest()))
                    .with_groth16_proof(),
            )
            .with_offer(
                Offer::default()
                    // The market uses a reverse Dutch auction mechanism to match requests with provers.
                    // Each request has a price range that a prover can bid on. One way to set the price
                    // is to choose a desired (min and max) price per million cycles and multiply it
                    // by the number of cycles. Alternatively, you can use the `with_min_price` and
                    // `with_max_price` methods to set the price directly.
                    .with_min_price_per_mcycle(
                        parse_ether("0.0001").unwrap_or_default(),
                        mcycles_count,
                    )
                    // NOTE: If your offer is not being accepted, try increasing the max price.
                    .with_max_price_per_mcycle(
                        parse_ether("0.0005").unwrap_or_default(),
                        mcycles_count,
                    )
                    // The timeout is the maximum number of blocks the request can stay
                    // unfulfilled in the market before it expires. If a prover locks in
                    // the request and does not fulfill it before the timeout, the prover can be
                    // slashed.
                    .with_timeout(2000)
                    .with_lock_timeout(1000)
                    .with_ramp_up_period(500),
            )
            .build()
            .unwrap();

        let offchain = config.get("offchain").map_or(false, |v| v == "true");
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
        let (journal, seal) = boundless_client
            .wait_for_request_fulfillment(request_id, Duration::from_secs(10), expires_at)
            .await
            .map_err(|e| {
                ProverError::GuestError(format!("Failed to wait for request fulfillment: {e}"))
            })?;
        tracing::info!(
            "Request 0x{request_id:x} fulfilled. Journal: {:?}, Seal: {:?}, image_id: {:?}",
            journal,
            seal,
            image_id
        );

        let journal_digest = journal.digest();
        let encoded_proof =
            verify_boundless_groth16_snark_impl(image_id, seal.to_vec(), journal_digest)
                .await
                .map_err(|e| {
                    ProverError::GuestError(format!("Failed to verify groth16 snark: {e}"))
                })?;
        let proof: Vec<u8> = (encoded_proof, B256::from_slice(image_id.as_bytes()))
            .abi_encode()
            .iter()
            .skip(32)
            .copied()
            .collect();

        let Ok(ContractReceipt::Base(boundless_receipt)) =
            risc0_ethereum_contracts_boundless::receipt::decode_seal(
                seal,
                RISC0_BATCH_ID,
                journal.clone(),
            )
        else {
            return Err(ProverError::GuestError(
                "did not receive requested unaggregated receipt".to_string(),
            ));
        };

        Ok(Risc0Response {
            proof: alloy_primitives::hex::encode_prefixed(proof),
            receipt: serde_json::to_string(&boundless_receipt).unwrap(),
            uuid: "".to_string(), // can be request tx hash
            input: output.hash,
        }
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use env_logger;
    use raiko_lib::input::GuestBatchOutput;

    #[tokio::test]
    async fn test_batch_run() {
        Risc0BoundlessProver::init_prover().await.unwrap();
    }

    #[tokio::test]
    async fn test_run_prover() {
        // init log
        env_logger::init();

        let input_file =
            std::fs::read("../../../gaiko/tests/fixtures/batch/input-1306738.json").unwrap();
        let output_file =
            std::fs::read("../../../gaiko/tests/fixtures/batch/output-1306738.json").unwrap();
        let input: GuestBatchInput = serde_json::from_slice(&input_file).unwrap();
        let output: GuestBatchOutput = serde_json::from_slice(&output_file).unwrap();
        let config = ProverConfig::default();
        let prover = Risc0BoundlessProver::init_prover().await.unwrap();
        let proof = prover
            .batch_run(input, &output, &config, None)
            .await
            .unwrap();
        println!("proof: {:?}", proof);

        // Save the boundless_receipt as JSON to a file for later deserialization.
        // The file name can be based on the request_id or image_id for uniqueness.
        let receipt_json = serde_json::to_string_pretty(&proof).unwrap();
        let file_name = format!("../../../boundless_receipt_test.json");
        if let Err(e) = std::fs::write(&file_name, receipt_json) {
            tracing::warn!(
                "Failed to write boundless_receipt to file {}: {}",
                file_name,
                e
            );
        } else {
            tracing::info!("Saved boundless_receipt to file: {}", file_name);
        }
    }

    #[tokio::test]
    async fn test_run_prover_with_seal() {
        env_logger::init();

        let seal = alloy_primitives::hex::decode("0x9f39696c021c04f95caa9962aa0022f0eae58f1cd7e13ccf553a152a3d0e91443d0aab4f25a24e93423c51f1ae46e604e20a360cfe2376e7270a10d1f4a9e665adcc91e713155b2e45e05edb00c7f044ab827a425cac6d0c932e3e14aeddf79200a8fe7711ad2207298cf2004c5dffc5956e9b30d6b98e9e2533b1e6944671f35dacf85823bb4fd3e0dd14a0000bc3304338f844b11095d1dbfedf3e90074bf7c666ed531dd4676c51fdf0111529d5c40719d36ba8ba11db8542fff1bca90c24255c515f1b6e32a396bf2bdb40ad165f949f1d46c533266a666e3b6684ddbbbc8c4ce5c1051676d81b1addd377e8b9665912d32347aac64c1a9b38faaab63ceeb1dcc67c").unwrap();
        let image_id = compute_image_id(RISC0_BATCH_ELF).unwrap();
        let journal = alloy_primitives::hex::decode(
            "0x20000000b0284764aae7f327410ae1355bef23dcccd0f9c723724d6638a8edde86091d65",
        )
        .unwrap();
        let journal_digest = journal.digest();
        let encoded_proof =
            verify_boundless_groth16_snark_impl(image_id, seal, journal_digest.into())
                .await
                .unwrap();
        println!("encoded_proof: {:?}", encoded_proof);
    }

    #[test]
    fn test_deserialize_proof() {
        // This test deserializes a proof from a JSON string (as would be returned from Boundless).
        let proof_json = r#"{
             "proof": "0x0000000000000000000000000000000000000000000000000000000000000040a9b03d0dd651aebfd77634799760072e8392c3c91e17d7c3da6785a61aaffdbe00000000000000000000000000000000000000000000000000000000000001049f39696c117e359f6a322d19b2ea8437271cda231c152d70fb553c6ed68e5c90e05c307c2787e39785bdec77c7cd712005367690160274f270397d7eca1e103c5633f7711ea988975445d70d2ce30d4da7648aa55d311b3796ffb35b3648ee7dd848f150002db50185bbc16d3aacf2d5ea19fe9368361b57ebc8590df4f637a91a142a32200efe06906e1e33c0e2caa7e8e9bec6aa0289e7f4ccb771ababe0a7df5e82960633839ddff0e44685ad0b9f137da03fd51cbeccc3d6cd163c83395814ed3d9618aca53e3ec65562300fee630606e22fe2b84c70a63dd60ffc42781f4d49ca08016bbe2581766d96144b1c90eb1eb65cfba92e9b4353c1fb9a6e89b957e3c1bf00000000000000000000000000000000000000000000000000000000",
             "input": "0x6f478ee63e81d8f341716638ebb2c524884af8441de92aed284176210169e942",
             "quote": "{\"inner\":{\"Groth16\":{\"seal\":[17,126,53,159,106,50,45,25,178,234,132,55,39,28,218,35,28,21,45,112,251,85,60,110,214,142,92,144,224,92,48,124,39,135,227,151,133,189,236,119,199,205,113,32,5,54,118,144,22,2,116,242,112,57,125,126,202,30,16,60,86,51,247,113,30,169,136,151,84,69,215,13,44,227,13,77,167,100,138,165,93,49,27,55,150,255,179,91,54,72,238,125,216,72,241,80,0,45,181,1,133,187,193,109,58,172,242,213,234,25,254,147,104,54,27,87,235,200,89,13,244,246,55,169,26,20,42,50,32,14,254,6,144,110,30,51,192,226,202,167,232,233,190,198,170,2,137,231,244,204,183,113,171,171,224,167,223,94,130,150,6,51,131,157,223,240,228,70,133,173,11,159,19,125,160,63,213,28,190,204,195,214,205,22,60,131,57,88,20,237,61,150,24,172,165,62,62,198,85,98,48,15,238,99,6,6,226,47,226,184,76,112,166,61,214,15,252,66,120,31,77,73,202,8,1,107,190,37,129,118,109,150,20,75,28,144,235,30,182,92,251,169,46,155,67,83,193,251,154,110,137,185,87,227,193,191],\"claim\":{\"Value\":{\"pre\":{\"Pruned\":[222146729,3215872470,2033481431,772235415,3385037443,3285653278,2793760730,3204296474]},\"post\":{\"Value\":{\"pc\":0,\"merkle_root\":[0,0,0,0,0,0,0,0]}},\"exit_code\":{\"Halted\":0},\"input\":{\"Value\":null},\"output\":{\"Value\":{\"journal\":{\"Value\":[32,0,0,0,176,40,71,100,170,231,243,39,65,10,225,53,91,239,35,220,204,208,249,199,35,114,77,102,56,168,237,222,134,9,29,101]},\"assumptions\":{\"Pruned\":[0,0,0,0,0,0,0,0]}}}}},\"verifier_parameters\":[1818835359,1620946611,2780288568,2130774364,576647948,727242602,2964052866,2234770906]}},\"journal\":{\"bytes\":[32,0,0,0,176,40,71,100,170,231,243,39,65,10,225,53,91,239,35,220,204,208,249,199,35,114,77,102,56,168,237,222,134,9,29,101]},\"metadata\":{\"verifier_parameters\":[1818835359,1620946611,2780288568,2130774364,576647948,727242602,2964052866,2234770906]}}}",
             "uuid": "",
             "kzg_proof": null
         }"#;

        // The ContractReceipt type is used for Boundless receipts.
        let proof: Proof =
            serde_json::from_str(proof_json).expect("Failed to deserialize proof JSON");
        println!("Deserialized receipt: {:#?}", proof);
    }

    #[test]
    fn test_deserialize_zkvm_receipt() {
        // let file_name = format!("../../../boundless_receipt_test.json");
        let file_name = format!("../../../boundless_receipt_test.json");
        let receipt_json = std::fs::read_to_string(file_name).unwrap();
        let proof: Proof = serde_json::from_str(&receipt_json).unwrap();
        println!("Deserialized proof: {:#?}", proof);

        let zkvm_receipt: ZkvmReceipt = serde_json::from_str(&proof.quote.unwrap()).unwrap();
        println!("Deserialized zkvm receipt: {:#?}", zkvm_receipt);
    }

    #[tokio::test]
    async fn test_run_proof_aggregation() {
        env_logger::init();

        let file_name = format!("../../../boundless_receipt_test.json");
        let receipt_json = std::fs::read_to_string(file_name).unwrap();
        let proof: Proof = serde_json::from_str(&receipt_json).unwrap();
        println!("Deserialized proof: {:#?}", proof);

        let input: AggregationGuestInput = AggregationGuestInput {
            proofs: vec![proof],
        };
        let output: AggregationGuestOutput = AggregationGuestOutput { hash: B256::ZERO };
        let config = ProverConfig::default();
        let prover = Risc0BoundlessProver::init_prover().await.unwrap();
        let proof = prover
            .aggregate(input, &output, &config, None)
            .await
            .unwrap();
        println!("proof: {:?}", proof);
    }
}
