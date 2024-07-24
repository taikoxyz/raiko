use std::collections::HashMap;

use alloy_primitives::Address;
use alloy_rpc_types::EIP1186AccountProofResponse;
use raiko_lib::{
    builder::{create_mem_db, RethBlockBuilder},
    consts::ChainSpec,
    input::{GuestInput, GuestOutput, TaikoProverData},
    protocol_instance::ProtocolInstance,
    prover::{IdStore, IdWrite, Proof, ProofKey},
};
use tracing::{debug, info, warn};

use crate::{
    interfaces::{ProofRequest, RaikoError, RaikoResult},
    preflight::preflight,
    provider::BlockDataProvider,
    utils::check_header,
};

pub mod interfaces;
pub mod preflight;
pub mod prover;
pub mod provider;
pub mod utils;

#[cfg(test)]
mod tests;

pub type MerkleProof = HashMap<Address, EIP1186AccountProofResponse>;

pub struct Raiko {
    l1_chain_spec: ChainSpec,
    taiko_chain_spec: ChainSpec,
    request: ProofRequest,
}

impl Raiko {
    pub fn new(
        l1_chain_spec: ChainSpec,
        taiko_chain_spec: ChainSpec,
        request: ProofRequest,
    ) -> Self {
        Self {
            l1_chain_spec,
            taiko_chain_spec,
            request,
        }
    }

    pub async fn generate_input<BDP: BlockDataProvider>(
        &self,
        provider: BDP,
    ) -> RaikoResult<GuestInput> {
        preflight(
            provider,
            self.request.block_number,
            self.l1_chain_spec.to_owned(),
            self.taiko_chain_spec.to_owned(),
            TaikoProverData {
                graffiti: self.request.graffiti,
                prover: self.request.prover,
            },
            self.request.blob_proof_type.clone(),
        )
        .await
        .map_err(Into::<RaikoError>::into)
    }

    pub fn get_output(&self, input: &GuestInput) -> RaikoResult<GuestOutput> {
        let db = create_mem_db(&mut input.clone()).unwrap();
        let mut builder = RethBlockBuilder::new(input, db);
        builder.execute_transactions(false).expect("execute");
        let result = builder.finalize();

        match result {
            Ok(header) => {
                info!("Verifying final state using provider data ...");
                info!(
                    "Final block hash derived successfully. {}",
                    header.hash_slow()
                );
                debug!("Final block header derived successfully. {header:?}");
                // Check if the header is the expected one
                check_header(&input.block.header, &header)?;

                Ok(GuestOutput {
                    header: header.clone(),
                    hash: ProtocolInstance::new(input, &header, self.request.proof_type.into())?
                        .instance_hash(),
                })
            }
            Err(e) => {
                warn!("Proving bad block construction!");
                Err(RaikoError::Guest(
                    raiko_lib::prover::ProverError::GuestError(e.to_string()),
                ))
            }
        }
    }

    pub async fn prove(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        store: Option<&mut dyn IdWrite>,
    ) -> RaikoResult<Proof> {
        let config = serde_json::to_value(&self.request)?;
        self.request
            .proof_type
            .run_prover(input, output, &config, store)
            .await
    }

    pub async fn cancel(
        &self,
        proof_key: ProofKey,
        read: Box<&mut dyn IdStore>,
    ) -> RaikoResult<()> {
        self.request.proof_type.cancel_proof(proof_key, read).await
    }
}
