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
use bonsai_sdk::alpha::responses::SnarkReceipt;
use ethers_contract::abigen;
use ethers_core::types::H160;
use ethers_providers::{Http, Provider, RetryClient};
use log::{error, info};
use raiko_lib::primitives::keccak::keccak;
use risc0_zkvm::{
    sha::{Digest, Digestible},
    Groth16ReceiptVerifierParameters, Receipt,
};

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

pub async fn stark2snark(
    image_id: Digest,
    stark_uuid: String,
    stark_receipt: Receipt,
) -> Result<(String, SnarkReceipt)> {
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

    let client = bonsai_sdk::alpha_async::get_client_from_env(risc0_zkvm::VERSION).await?;
    let snark_uuid = client.create_snark(stark_uuid)?;

    let snark_receipt = loop {
        let res = snark_uuid.status(&client)?;

        if res.status == "RUNNING" {
            info!("Current status: {} - continue polling...", res.status);
            std::thread::sleep(std::time::Duration::from_secs(15));
        } else if res.status == "SUCCEEDED" {
            break res
                .output
                .expect("Bonsai response is missing SnarkReceipt.");
        } else {
            panic!(
                "Workflow exited: {} - | err: {}",
                res.status,
                res.error_msg.unwrap_or_default()
            );
        }
    };

    let stark_psd = stark_receipt.claim()?.as_value().unwrap().post.digest();
    let snark_psd = Digest::try_from(snark_receipt.post_state_digest.as_slice())?;

    if stark_psd != snark_psd {
        error!("SNARK/STARK Post State Digest mismatch!");
        error!("STARK: {}", hex::encode(stark_psd));
        error!("SNARK: {}", hex::encode(snark_psd));
    }

    if snark_receipt.journal != stark_receipt.journal.bytes {
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
    snark_receipt: SnarkReceipt,
) -> Result<Vec<u8>> {
    let seal = encode(snark_receipt.snark.to_vec())?;
    let journal_digest = snark_receipt.journal.digest();
    let post_state_digest = snark_receipt.post_state_digest.digest();
    verify_groth16_snark_impl(image_id, seal, journal_digest, post_state_digest).await
}

pub async fn verify_groth16_snark_from_receipt(
    image_id: Digest,
    receipt: Receipt,
) -> Result<Vec<u8>> {
    let seal = receipt.inner.groth16().unwrap().seal.clone();
    let journal_digest = receipt.journal.digest();
    let post_state_digest = receipt.claim()?.as_value().unwrap().post.digest();
    verify_groth16_snark_impl(image_id, seal, journal_digest, post_state_digest).await
}

pub async fn verify_groth16_snark_impl(
    image_id: Digest,
    seal: Vec<u8>,
    journal_digest: Digest,
    post_state_digest: Digest,
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

    let enc_seal = encode(seal)?;
    tracing_info!("Verifying SNARK:");
    tracing_info!("Seal: {}", hex::encode(&enc_seal));
    tracing_info!("Image ID: {}", hex::encode(image_id.as_bytes()));
    tracing_info!("Post State Digest: {}", hex::encode(&post_state_digest));
    tracing_info!("Journal Digest: {}", hex::encode(journal_digest));
    let verify_call_res = IRiscZeroVerifier::new(groth16_verifier_addr, http_client)
        .verify(
            enc_seal.clone().into(),
            image_id.as_bytes().try_into().unwrap(),
            journal_digest.into(),
        )
        .await;

    if verify_call_res.is_ok() {
        tracing_info!("SNARK verified successfully using {groth16_verifier_addr:?}!");
    } else {
        tracing_err!("SNARK verification failed: {:?}!", verify_call_res);
    }

    Ok(make_risc0_groth16_proof(enc_seal, image_id))
}

pub fn make_risc0_groth16_proof(seal: Vec<u8>, image_id: Digest) -> Vec<u8> {
    (seal, B256::from_slice(image_id.as_bytes()))
        .abi_encode()
        .iter()
        .skip(32)
        .copied()
        .collect()
}
