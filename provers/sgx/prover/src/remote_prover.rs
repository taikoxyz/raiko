#![cfg(feature = "enable")]

use crate::{SgxParam, SgxResponse};
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, RawAggregationGuestInput, RawProof, ShastaAggregationGuestInput,
        ShastaRawAggregationGuestInput,
    },
    proof_type::ProofType,
    prover::{
        IdStore, IdWrite, Proof, ProofCarryData, ProofKey, Prover, ProverConfig, ProverError,
        ProverResult,
    },
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::time::Duration;

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct RemoteSgxResponse {
    pub status: String,
    pub message: String,
    #[serde(alias = "proof")]
    pub sgx_response: SgxResponse,
}

// raiko end point
const RAIKO_REMOTE_URL: &str = "http://localhost:9090";
// gaiko end point
const GAIKO_REMOTE_URL: &str = "http://localhost:8090";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteSgxProver {
    proof_type: ProofType,
    remote_prover_url: String,
}

impl RemoteSgxProver {
    pub fn new(proof_type: ProofType) -> Self {
        let remote_prover_url =
            match proof_type {
                ProofType::SgxGeth => std::env::var("GAIKO_REMOTE_URL")
                    .unwrap_or_else(|_| GAIKO_REMOTE_URL.to_string()),
                ProofType::Sgx => std::env::var("RAIKO_REMOTE_URL")
                    .unwrap_or_else(|_| RAIKO_REMOTE_URL.to_string()),
                _ => panic!("Unsupported proof type for remote prover"),
            };
        Self {
            proof_type,
            remote_prover_url,
        }
    }
}

impl Prover for RemoteSgxProver {
    async fn run(
        &self,
        input: GuestInput,
        _output: &GuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param =
            SgxParam::deserialize(config.get(self.proof_type.to_string()).unwrap()).unwrap();

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup {
            unimplemented!("SGX setup not implemented for remote prover");
        }

        let mut sgx_proof = if sgx_param.bootstrap {
            bootstrap(&self.remote_prover_url, self.proof_type).await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(SgxResponse::default())
        };

        if sgx_param.prove {
            // overwrite sgx_proof as the bootstrap quote stays the same in bootstrap & prove.
            let instance_id = get_instance_id_from_params(&input, &sgx_param)?;
            sgx_proof = prove(&self.remote_prover_url, input.clone(), instance_id).await
        }

        sgx_proof.map(|r| r.into())
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param =
            SgxParam::deserialize(config.get(self.proof_type.to_string()).unwrap()).unwrap();

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup {
            unimplemented!("SGX setup not implemented for remote prover");
        }

        if sgx_param.bootstrap {
            unimplemented!("SGX bootstrap not implemented for aggregation request");
        };

        let sgx_proof = aggregate(&self.remote_prover_url, input.clone(), self.proof_type).await?;
        Ok(sgx_proof.into())
    }

    async fn cancel(&self, _proof_key: ProofKey, _read: Box<&mut dyn IdStore>) -> ProverResult<()> {
        Ok(())
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        _output: &GuestBatchOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param =
            SgxParam::deserialize(config.get(self.proof_type.to_string()).unwrap()).unwrap();

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup {
            unimplemented!("SGX setup not implemented for remote prover");
        }

        let mut sgx_proof = if sgx_param.bootstrap {
            bootstrap(&self.remote_prover_url, self.proof_type).await
        } else {
            // Dummy proof: it's ok when only setup/bootstrap was requested
            Ok(SgxResponse::default())
        };

        if sgx_param.prove {
            // overwrite sgx_proof as the bootstrap quote stays the same in bootstrap & prove.
            let instance_id = get_instance_id_from_params(&input.inputs[0], &sgx_param)?;
            sgx_proof = batch_prove(
                &self.remote_prover_url,
                input.clone(),
                instance_id,
                self.proof_type,
            )
            .await
        }

        sgx_proof.map(|r| r.into())
    }

    async fn shasta_aggregate(
        &self,
        input: ShastaAggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let sgx_param =
            SgxParam::deserialize(config.get(self.proof_type.to_string()).unwrap()).unwrap();

        // Setup: run this once while setting up your SGX instance
        if sgx_param.setup {
            unimplemented!("SGX setup not implemented for remote prover");
        }

        if sgx_param.bootstrap {
            unimplemented!("SGX bootstrap not implemented for aggregation request");
        };

        let sgx_proof =
            shasta_aggregate(&self.remote_prover_url, input.clone(), self.proof_type).await?;
        Ok(sgx_proof.into())
    }

    fn proof_type(&self) -> ProofType {
        self.proof_type
    }
}

pub async fn bootstrap(
    remote_sgx_url: &str,
    _proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    // post to remote sgx provider/bootstrap
    let client = Client::new();
    let post_url = format!("{}/bootstrap", remote_sgx_url);
    let response = client
        .post(post_url)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to send request: {e}")))?;

    if response.status().is_success() {
        let response_text = response
            .text()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to read response: {e}")))?;
        println!("Response: {}", response_text);
        serde_json::from_str(&response_text)
            .map_err(|e| ProverError::GuestError(format!("Failed to parse response: {e}")))
    } else {
        println!("Request failed with status: {}", response.status());
        Err(ProverError::GuestError(format!(
            "Failed to read error response: {}",
            response.status()
        )))
    }
}

async fn prove(
    remote_sgx_url: &str,
    input: GuestInput,
    _instance_id: u64,
) -> ProverResult<SgxResponse, ProverError> {
    // post to remote sgx provider/bootstrap
    let client = Client::new();
    let post_url = format!("{}/prove/block", remote_sgx_url);
    let json_input = serde_json::to_string(&input)
        .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
    let response = client
        .post(post_url)
        .header("Content-Type", "application/json")
        .body(json_input)
        .send()
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to send request: {e}")))?;

    if response.status().is_success() {
        let response_text = response
            .text()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to read response: {e}")))?;
        let sgx_proof: RemoteSgxResponse = serde_json::from_str(&response_text)
            .map_err(|e| ProverError::GuestError(format!("Failed to parse response: {e}")))?;
        if sgx_proof.status == "success" {
            Ok(sgx_proof.sgx_response)
        } else {
            tracing::error!("Response has error status: {}", sgx_proof.status);
            Err(ProverError::GuestError(format!(
                "Response has error status: {}",
                sgx_proof.message
            )))
        }
    } else {
        Err(ProverError::GuestError(format!(
            "Request failed with status: {}",
            response.status()
        )))
    }
}

async fn batch_prove(
    remote_sgx_url: &str,
    input: GuestBatchInput,
    _instance_id: u64,
    _proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    // post to remote sgx provider/bootstrap
    let client = Client::new();
    let post_url = format!("{}/prove/batch", remote_sgx_url);
    let json_input = serde_json::to_string(&input)
        .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
    let response = client
        .post(post_url)
        .header("Content-Type", "application/json")
        .body(json_input)
        .timeout(Duration::from_secs(200))
        .send()
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to send request: {e}")))?;

    if response.status().is_success() {
        let response_text = response
            .text()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to read response: {e}")))?;
        let sgx_proof: RemoteSgxResponse = serde_json::from_str(&response_text)
            .map_err(|e| ProverError::GuestError(format!("Failed to parse response: {e}")))?;
        if sgx_proof.status == "success" {
            Ok(sgx_proof.sgx_response)
        } else {
            tracing::error!("Response has error status: {}", sgx_proof.status);
            Err(ProverError::GuestError(format!(
                "Response has error status: {}",
                sgx_proof.message
            )))
        }
    } else {
        tracing::error!("Request failed with status: {}", response.status());
        Err(ProverError::GuestError(format!(
            "Failed to read error response: {}",
            response.status()
        )))
    }
}

async fn aggregate(
    remote_sgx_url: &str,
    input: AggregationGuestInput,
    _proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    // Extract the useful parts of the proof here so the guest doesn't have to do it
    let raw_input = RawAggregationGuestInput {
        proofs: input
            .proofs
            .iter()
            .map(|proof| RawProof {
                input: proof.clone().input.unwrap(),
                proof: hex::decode(&proof.clone().proof.unwrap()[2..]).unwrap(),
            })
            .collect(),
    };
    // Extract the instance id from the first proof
    let _instance_id = {
        let mut instance_id_bytes = [0u8; 4];
        instance_id_bytes[0..4].copy_from_slice(&raw_input.proofs[0].proof.clone()[0..4]);
        u32::from_be_bytes(instance_id_bytes)
    };

    // post to remote sgx provider/bootstrap
    let client = Client::new();
    let post_url = format!("{}/prove/aggregate", remote_sgx_url);
    let json_input = serde_json::to_string(&raw_input)
        .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
    let response = client
        .post(post_url)
        .header("Content-Type", "application/json")
        .body(json_input)
        .timeout(Duration::from_secs(200))
        .send()
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to send request: {e}")))?;

    if response.status().is_success() {
        let response_text = response
            .text()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to read response: {e}")))?;
        let sgx_proof: RemoteSgxResponse = serde_json::from_str(&response_text)
            .map_err(|e| ProverError::GuestError(format!("Failed to parse response: {e}")))?;
        if sgx_proof.status == "success" {
            Ok(sgx_proof.sgx_response)
        } else {
            tracing::error!("Response has error status: {}", sgx_proof.status);
            Err(ProverError::GuestError(format!(
                "Response has error status: {}",
                sgx_proof.message
            )))
        }
    } else {
        tracing::error!("Request failed with status: {}", response.status());
        Err(ProverError::GuestError(format!(
            "Request failed with status: {}",
            response.status()
        )))
    }
}

async fn shasta_aggregate(
    remote_sgx_url: &str,
    input: ShastaAggregationGuestInput,
    _proof_type: ProofType,
) -> ProverResult<SgxResponse, ProverError> {
    // Extract the useful parts of the proof here so the guest doesn't have to do it
    let (proofs, proof_carry_data_vec): (Vec<_>, Vec<_>) = input
        .proofs
        .iter()
        .map(|proof| {
            (
                RawProof {
                    input: proof.input.clone().unwrap(),
                    proof: hex::decode(&proof.proof.clone().unwrap()[2..]).unwrap(),
                },
                {
                    let extra_data = proof.extra_data.clone().unwrap();
                    ProofCarryData {
                        chain_id: extra_data.chain_id,
                        verifier: extra_data.verifier,
                        transition_input: extra_data.transition_input,
                    }
                },
            )
        })
        .unzip();
    let raw_input = ShastaRawAggregationGuestInput {
        proofs,
        proof_carry_data_vec,
    };

    // Extract the instance id from the first proof
    let _instance_id = {
        let mut instance_id_bytes = [0u8; 4];
        instance_id_bytes[0..4].copy_from_slice(&raw_input.proofs[0].proof.clone()[0..4]);
        u32::from_be_bytes(instance_id_bytes)
    };

    // post to remote sgx provider/bootstrap
    let client = Client::new();
    let post_url = format!("{}/prove/shasta-aggregate", remote_sgx_url);
    let json_input = serde_json::to_string(&raw_input)
        .map_err(|e| ProverError::GuestError(format!("Failed to serialize input: {e}")))?;
    let response = client
        .post(post_url)
        .header("Content-Type", "application/json")
        .body(json_input)
        .timeout(Duration::from_secs(200))
        .send()
        .await
        .map_err(|e| ProverError::GuestError(format!("Failed to send request: {e}")))?;

    if response.status().is_success() {
        let response_text = response
            .text()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to read response: {e}")))?;
        let sgx_proof: RemoteSgxResponse = serde_json::from_str(&response_text)
            .map_err(|e| ProverError::GuestError(format!("Failed to parse response: {e}")))?;
        if sgx_proof.status == "success" {
            Ok(sgx_proof.sgx_response)
        } else {
            tracing::error!("Response has error status: {}", sgx_proof.status);
            Err(ProverError::GuestError(format!(
                "Response has error status: {}",
                sgx_proof.message
            )))
        }
    } else {
        tracing::error!("Request failed with status: {}", response.status());
        Err(ProverError::GuestError(format!(
            "Request failed with status: {}",
            response.status()
        )))
    }
}

pub fn get_instance_id_from_params(input: &GuestInput, sgx_param: &SgxParam) -> ProverResult<u64> {
    let spec_id = input
        .chain_spec
        .active_fork(input.block.number, input.block.timestamp)
        .map_err(|e| ProverError::GuestError(e.to_string()))?;

    let instance_id = sgx_param
        .instance_ids
        .get(&spec_id)
        .cloned()
        .ok_or_else(|| {
            ProverError::GuestError(format!("No instance id found for spec id: {:?}", spec_id))
        });

    instance_id
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use raiko_lib::consts::{SpecId, SupportedChainSpecs};

    use super::*;

    #[test]
    fn test_get_instance_id_from_params() {
        let _ = env_logger::builder().is_test(true).try_init();
        let taiko_chain_spec = SupportedChainSpecs::merge_from_file(PathBuf::from(
            "../../../host/config/chain_spec_list_devnet.json",
        ))
        .expect("ok")
        .get_chain_spec("taiko_dev")
        .unwrap();
        let sgx_param = SgxParam {
            instance_ids: vec![(SpecId::PACAYA, 0), (SpecId::SHASTA, 10)]
                .into_iter()
                .collect(),
            setup: false,
            bootstrap: false,
            prove: false,
        };
        let spec_id = taiko_chain_spec
            .active_fork(1, 0)
            .map_err(|e| ProverError::GuestError(e.to_string()))
            .expect("ok");
        assert_eq!(spec_id, SpecId::SHASTA);

        let instance_id = sgx_param
            .instance_ids
            .get(&spec_id)
            .cloned()
            .ok_or_else(|| {
                ProverError::GuestError(format!("No instance id found for spec id: {:?}", spec_id))
            })
            .expect("ok");
        assert_eq!(instance_id, 10);

        let spec_id = taiko_chain_spec
            .active_fork(15, 0)
            .map_err(|e| ProverError::GuestError(e.to_string()))
            .expect("ok");
        assert_eq!(spec_id, SpecId::SHASTA);

        let instance_id = sgx_param
            .instance_ids
            .get(&spec_id)
            .cloned()
            .ok_or_else(|| {
                ProverError::GuestError(format!("No instance id found for spec id: {:?}", spec_id))
            })
            .expect("ok");
        assert_eq!(instance_id, 10);
    }
}
