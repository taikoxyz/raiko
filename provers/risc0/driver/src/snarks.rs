// Copyright 2024 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{str::FromStr, sync::Arc};

use alloy_primitives::B256;
use alloy_sol_types::{sol, SolValue};
use anyhow::Result;
use hex;
use bonsai_sdk::blocking::Client;
use ethers_contract::abigen;
use ethers_core::types::H160;
use ethers_providers::{Http, Provider, RetryClient};
use log::{error, info};
use raiko_lib::primitives::keccak::keccak;
use risc0_zkvm::{
    sha::{Digest, Digestible},
    Groth16ReceiptVerifierParameters, Receipt,
};
use tokio::time::{sleep as tokio_async_sleep, Duration};

use tracing::{error as tracing_err, info as tracing_info};

use crate::bonsai::save_receipt;

sol!(
    /// A Groth16 seal over the claimed receipt claim.
    struct Seal {
        uint256[2] a;
        uint256[2][2] b;
        uint256[2] c;
    }
    /// Verifier interface for RISC Zero receipts of execution.
    #[derive(Debug)]
    interface RiscZeroVerifier {
        /// Verify that the given seal is a valid RISC Zero proof of execution with the
        /// given image ID, post-state digest, and journal digest. This method additionally
        /// ensures that the input hash is all-zeros (i.e. no committed input), the exit code
        /// is (Halted, 0), and there are no assumptions (i.e. the receipt is unconditional).
        /// Returns true if the receipt passes the verification checks. The return code must be checked.
        function verify(
            /// The encoded cryptographic proof (i.e. SNARK).
            bytes calldata seal,
            /// The identifier for the guest program.
            bytes32 imageId,
            /// A hash of the final memory state. Required to run the verifier, but otherwise can be left unconstrained for most use cases.
            bytes32 postStateDigest,
            /// The SHA-256 digest of the journal bytes.
            bytes32 journalDigest
        )
            external
            view
        returns (bool);
    }
);

abigen!(
    IRiscZeroVerifier,
    r#"[
        function verify(bytes calldata seal, bytes32 imageId, bytes32 journalDigest) external view
    ]"#
);

/// encoding of the seal with selector.
pub fn encode(seal: Vec<u8>) -> Result<Vec<u8>> {
    let verifier_parameters_digest = Groth16ReceiptVerifierParameters::default().digest();
    let selector = &verifier_parameters_digest.as_bytes()[..4];
    // Create a new vector with the capacity to hold both selector and seal
    let mut selector_seal = Vec::with_capacity(selector.len() + seal.len());
    selector_seal.extend_from_slice(selector);
    selector_seal.extend_from_slice(&seal);

    Ok(selector_seal)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SealEncoding {
    /// Seal bytes are the raw Groth16 seal and must be prefixed with the verifier-parameter selector.
    RawNeedsSelector,
    /// Seal bytes already include the verifier-parameter selector prefix.
    AlreadyEncoded,
}

fn normalize_seal(seal: Vec<u8>, encoding: SealEncoding) -> Result<Vec<u8>> {
    match encoding {
        SealEncoding::RawNeedsSelector => encode(seal),
        SealEncoding::AlreadyEncoded => Ok(seal),
    }
}

async fn verify_groth16_onchain(
    image_id: Digest,
    seal: Vec<u8>,
    journal_digest: Digest,
    post_state_digest: Option<Digest>,
) -> Result<Vec<u8>> {
    let verifier_rpc_url =
        std::env::var("GROTH16_VERIFIER_RPC_URL").expect("env GROTH16_VERIFIER_RPC_URL");
    let groth16_verifier_addr = {
        let addr = std::env::var("GROTH16_VERIFIER_ADDRESS").expect("env GROTH16_VERIFIER_RPC_URL");
        H160::from_str(&addr).unwrap()
    };

    let http_client = Arc::new(Provider::<RetryClient<Http>>::new_client(
        &verifier_rpc_url,
        3,
        500,
    )?);

    tracing_info!("Verifying SNARK:");
    tracing_info!("Seal: {}", hex::encode(&seal));
    tracing_info!("Image ID: {}", hex::encode(image_id.as_bytes()));
    if let Some(post_state_digest) = post_state_digest {
        tracing_info!("Post State Digest: {}", hex::encode(post_state_digest));
    }
    tracing_info!("Journal Digest: {}", hex::encode(journal_digest));

    let verify_call_res = IRiscZeroVerifier::new(groth16_verifier_addr, http_client)
        .verify(
            seal.clone().into(),
            image_id.as_bytes().try_into().unwrap(),
            journal_digest.into(),
        )
        .await;

    if verify_call_res.is_ok() {
        tracing_info!("SNARK verified successfully using {groth16_verifier_addr:?}!");
    } else {
        tracing_err!(
            "SNARK verification call to {groth16_verifier_addr:?} failed: {verify_call_res:?}!"
        );
    }

    Ok(seal)
}

pub async fn stark2snark(
    image_id: Digest,
    stark_uuid: String,
    stark_receipt: Receipt,
    max_retries: usize,
) -> Result<(String, Receipt)> {
    info!("Submitting SNARK workload");
    // Label snark output as journal digest
    let receipt_label = format!(
        "{}-{}",
        hex::encode_upper(image_id),
        hex::encode(keccak(stark_receipt.journal.bytes.digest()))
    );
    // Load cached receipt if found
    if let Ok(Some(cached_data)) = crate::bonsai::load_receipt(&receipt_label) {
        info!("Loaded locally cached snark receipt {receipt_label:?}");
        return Ok(cached_data);
    }
    // Otherwise compute on Bonsai
    let stark_uuid = if stark_uuid.is_empty() {
        crate::bonsai::upload_receipt(&stark_receipt).await?
    } else {
        stark_uuid
    };

    let client = Client::from_env(risc0_zkvm::VERSION)?;
    let snark_uuid = client.create_snark(stark_uuid.clone())?;

    let mut retry = 0;
    let snark_receipt = loop {
        let res = snark_uuid.status(&client)?;
        if res.status == "RUNNING" {
            info!(
                "Current {:?} status: {} - continue polling...",
                &stark_uuid, res.status
            );
            tokio_async_sleep(Duration::from_secs(15)).await;
        } else if res.status == "SUCCEEDED" {
            let download_url = res
                .output
                .expect("Bonsai response is missing SnarkReceipt.");
            let receipt_buf = client.download(&download_url)?;
            let snark_receipt: Receipt = bincode::deserialize(&receipt_buf)?;
            break snark_receipt;
        } else {
            if retry < max_retries {
                retry += 1;
                info!(
                    "Workflow {:?} exited: {} - | err: {} - retrying {}/{max_retries}",
                    stark_uuid,
                    res.status,
                    res.error_msg.unwrap_or_default(),
                    retry
                );
                tokio_async_sleep(Duration::from_secs(15)).await;
                continue;
            }
            panic!(
                "Workflow exited: {} - | err: {}",
                res.status,
                res.error_msg.unwrap_or_default()
            );
        }
    };

    let stark_psd = stark_receipt.claim()?.as_value().unwrap().post.digest();
    let snark_psd = snark_receipt.claim()?.as_value().unwrap().post.digest();

    if stark_psd != snark_psd {
        error!("SNARK/STARK Post State Digest mismatch!");
        error!("STARK: {}", hex::encode(stark_psd));
        error!("SNARK: {}", hex::encode(snark_psd));
    }

    if snark_receipt.journal.bytes != stark_receipt.journal.bytes {
        error!("SNARK/STARK Receipt Journal mismatch!");
        error!("STARK: {}", hex::encode(&stark_receipt.journal.bytes));
        error!("SNARK: {}", hex::encode(&snark_receipt.journal));
    };

    let snark_data = (snark_uuid.uuid, snark_receipt);

    save_receipt(&receipt_label, &snark_data);

    Ok(snark_data)
}

pub async fn verify_groth16_from_snark_receipt(
    image_id: Digest,
    snark_receipt: Receipt,
) -> Result<Vec<u8>> {
    let groth16_claim = snark_receipt.inner.groth16().unwrap();
    let seal = groth16_claim.seal.clone();
    let journal_digest = snark_receipt.journal.digest();
    let post_state_digest = snark_receipt.claim()?.as_value().unwrap().post.digest();
    let encoded_proof =
        verify_groth16_snark_impl(image_id, seal, journal_digest, post_state_digest).await?;
    let proof = (encoded_proof, B256::from_slice(image_id.as_bytes()))
        .abi_encode()
        .iter()
        .skip(32)
        .copied()
        .collect();
    Ok(proof)
}

pub async fn verify_aggregation_groth16_proof(
    block_proof_image_id: Digest,
    aggregation_image_id: Digest,
    receipt: Receipt,
) -> Result<Vec<u8>> {
    let seal = receipt
        .inner
        .groth16()
        .map_err(|e| anyhow::Error::msg(format!("receipt.inner.groth16() failed: {e:?}")))?
        .seal
        .clone();
    let journal_digest = receipt.journal.digest();
    let post_state_digest = receipt
        .claim()?
        .as_value()
        .map_err(|e| anyhow::Error::msg(format!("receipt.claim()?.as_value() failed: {e:?}")))?
        .post
        .digest();
    let encoded_proof = verify_groth16_snark_impl(
        aggregation_image_id,
        seal,
        journal_digest,
        post_state_digest,
    )
    .await?;
    let proof = (
        encoded_proof,
        B256::from_slice(block_proof_image_id.as_bytes()),
        B256::from_slice(aggregation_image_id.as_bytes()),
    )
        .abi_encode()
        .iter()
        .skip(32)
        .copied()
        .collect();
    Ok(proof)
}

pub async fn verify_groth16_snark_impl(
    image_id: Digest,
    seal: Vec<u8>,
    journal_digest: Digest,
    post_state_digest: Digest,
) -> Result<Vec<u8>> {
    let enc_seal = normalize_seal(seal, SealEncoding::RawNeedsSelector)?;
    verify_groth16_onchain(image_id, enc_seal, journal_digest, Some(post_state_digest)).await
}

/// Verify a boundless Groth16 seal (already encoded) against the on-chain verifier.
/// Unlike the standard flow, the post-state digest is not required because the seal
/// already contains the verifier parameters selector.
pub async fn verify_boundless_groth16_snark_impl(
    image_id: Digest,
    seal: Vec<u8>,
    journal_digest: Digest,
) -> Result<Vec<u8>> {
    let enc_seal = normalize_seal(seal, SealEncoding::AlreadyEncoded)?;
    verify_groth16_onchain(image_id, enc_seal, journal_digest, None).await
}
