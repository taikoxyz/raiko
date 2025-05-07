#![cfg(feature = "enable")]

use dashmap::DashMap;
use once_cell::sync::Lazy;
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ZkAggregationGuestInput,
    },
    proof_type::ProofType,
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
    Measurement,
};
use reth_primitives::B256;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use sp1_prover::{components::CpuProverComponents, Groth16Bn254Proof};
use sp1_sdk::{
    network::FulfillmentStrategy, NetworkProver, Prover as SP1ProverTrait, SP1Proof, SP1ProofMode,
    SP1ProofWithPublicValues, SP1ProvingKey, SP1VerifyingKey,
};
use sp1_sdk::{HashableKey, ProverClient, SP1Stdin};
use std::{borrow::BorrowMut, env, sync::Arc, time::Duration};
use tracing::{debug, info};

mod proof_verify;
use proof_verify::remote_contract_verify::verify_sol_by_contract_call;

pub const ELF: &[u8] = include_bytes!("../../guest/elf/sp1-guest");
pub const AGGREGATION_ELF: &[u8] = include_bytes!("../../guest/elf/sp1-aggregation");
pub const BATCH_ELF: &[u8] = include_bytes!("../../guest/elf/sp1-batch");

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sp1Param {
    #[serde(default = "RecursionMode::default")]
    pub recursion: RecursionMode,
    pub prover: Option<ProverMode>,
    #[serde(default = "DEFAULT_TRUE")]
    pub verify: bool,
}

const DEFAULT_TRUE: fn() -> bool = || true;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RecursionMode {
    /// The proof mode for an SP1 core proof.
    Core,
    /// The proof mode for a compressed proof.
    Compressed,
    /// The proof mode for a PlonK proof.
    #[default]
    Plonk,
}

impl From<RecursionMode> for SP1ProofMode {
    fn from(value: RecursionMode) -> Self {
        match value {
            RecursionMode::Core => SP1ProofMode::Core,
            RecursionMode::Compressed => SP1ProofMode::Compressed,
            RecursionMode::Plonk => SP1ProofMode::Plonk,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ProverMode {
    Mock,
    Local,
    Network,
}

impl From<Sp1Response> for Proof {
    fn from(value: Sp1Response) -> Self {
        Self {
            proof: value.proof,
            quote: value
                .sp1_proof
                .as_ref()
                .map(|p| serde_json::to_string(&p.proof).unwrap()),
            input: value
                .sp1_proof
                .as_ref()
                .map(|p| B256::from_slice(p.public_values.as_slice())),
            uuid: value.vkey.map(|v| serde_json::to_string(&v).unwrap()),
            kzg_proof: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Sp1Response {
    pub proof: Option<String>,
    /// for aggregation
    pub sp1_proof: Option<SP1ProofWithPublicValues>,
    pub vkey: Option<SP1VerifyingKey>,
}

pub struct Sp1Prover;

#[derive(Clone)]
struct Sp1ProverClient {
    pub(crate) client: Arc<Box<dyn SP1ProverTrait<CpuProverComponents>>>,
    pub(crate) network_client: Arc<NetworkProver>,
    pub(crate) pk: SP1ProvingKey,
    pub(crate) vk: SP1VerifyingKey,
}

//TODO: use prover object to save such local storage members.
static BLOCK_PROOF_CLIENT: Lazy<DashMap<ProverMode, Sp1ProverClient>> = Lazy::new(DashMap::new);
static AGGREGATION_CLIENT: Lazy<DashMap<ProverMode, Sp1ProverClient>> = Lazy::new(DashMap::new);
static BATCH_PROOF_CLIENT: Lazy<DashMap<ProverMode, Sp1ProverClient>> = Lazy::new(DashMap::new);

impl Prover for Sp1Prover {
    async fn run(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let mut param = Sp1Param::deserialize(config.get("sp1").unwrap()).unwrap();
        let mode = param.prover.clone().unwrap_or_else(get_env_mock);

        println!("param: {param:?}");
        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        let Sp1ProverClient {
            client,
            pk,
            vk,
            network_client,
        } = BLOCK_PROOF_CLIENT
            .entry(mode.clone())
            .or_insert_with(|| {
                let network_client = Arc::new(ProverClient::builder().network().build());
                let base_client: Box<dyn SP1ProverTrait<CpuProverComponents>> = match mode {
                    ProverMode::Mock => Box::new(ProverClient::builder().mock().build()),
                    ProverMode::Local => Box::new(ProverClient::builder().cpu().build()),
                    ProverMode::Network => Box::new(ProverClient::builder().network().build()),
                };

                let client = Arc::new(base_client);
                let (pk, vk) = client.setup(ELF);
                info!(
                    "new client and setup() for block {:?}.",
                    output.header.number
                );
                Sp1ProverClient {
                    client,
                    network_client,
                    pk,
                    vk,
                }
            })
            .clone();

        info!(
            "Sp1 Prover: block {:?} with vk {:?}",
            output.header.number,
            vk.bytes32()
        );

        let prove_result = if !matches!(mode, ProverMode::Network) {
            debug!("Proving locally with recursion mode: {:?}", param.recursion);
            let prove_mode = match param.recursion {
                RecursionMode::Core => SP1ProofMode::Core,
                RecursionMode::Compressed => SP1ProofMode::Compressed,
                RecursionMode::Plonk => SP1ProofMode::Plonk,
            };
            client
                .prove(&pk, &stdin, prove_mode)
                .map_err(|e| ProverError::GuestError(format!("Sp1: local proving failed: {e}")))?
        } else {
            let proof_id = network_client
                .prove(&pk, &stdin)
                .mode(param.recursion.clone().into())
                .cycle_limit(1_000_000_000_000)
                .skip_simulation(true)
                .strategy(FulfillmentStrategy::Reserved)
                .request_async()
                .await
                .map_err(|e| {
                    ProverError::GuestError(format!("Sp1: requesting proof failed: {e}"))
                })?;
            if let Some(id_store) = id_store {
                id_store
                    .store_id(
                        (
                            input.chain_spec.chain_id,
                            input.block.header.number,
                            output.hash,
                            ProofType::Sp1 as u8,
                        ),
                        proof_id.clone().to_string(),
                    )
                    .await?;
            }
            info!(
                "Sp1: network proof id: {:?} for block {:?}",
                proof_id, output.header.number
            );
            network_client
                .wait_proof(proof_id.clone(), Some(Duration::from_secs(3600)))
                .await
                .map_err(|e| ProverError::GuestError(format!("Sp1: network proof failed {e:?}")))?
        };

        let proof_bytes = match param.recursion {
            RecursionMode::Compressed => {
                info!("Compressed proof is used in aggregation mode only");
                vec![]
            }
            _ => prove_result.bytes(),
        };
        if param.verify && !proof_bytes.is_empty() {
            let time = Measurement::start("verify", false);
            let pi_hash = prove_result
                .clone()
                .borrow_mut()
                .public_values
                .read::<[u8; 32]>();
            let fixture = RaikoProofFixture {
                vkey: vk.bytes32(),
                public_values: B256::from_slice(&pi_hash).to_string(),
                proof: proof_bytes.clone(),
            };

            verify_sol_by_contract_call(&fixture).await?;
            time.stop_with("==> Verification complete");
        }

        let proof_string = (!proof_bytes.is_empty()).then_some(
            // 0x + 64 bytes of the vkey + the proof
            // vkey itself contains 0x prefix
            format!(
                "{}{}",
                vk.bytes32(),
                reth_primitives::hex::encode(proof_bytes)
            ),
        );

        info!(
            "Sp1 Prover: block {:?} completed! proof: {proof_string:?}",
            output.header.number,
        );
        Ok::<_, ProverError>(
            Sp1Response {
                proof: proof_string,
                sp1_proof: Some(prove_result),
                vkey: Some(vk.clone()),
            }
            .into(),
        )
    }

    async fn cancel(&self, key: ProofKey, id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        // let proof_id = match id_store.read_id(key).await {
        //     Ok(proof_id) => proof_id,
        //     Err(e) => {
        //         if e.to_string().contains("No data for query") {
        //             return Ok(());
        //         } else {
        //             return Err(ProverError::GuestError(e.to_string()));
        //         }
        //     }
        // };
        // let private_key = env::var("SP1_PRIVATE_KEY").map_err(|_| {
        //     ProverError::GuestError("SP1_PRIVATE_KEY must be set for remote proving".to_owned())
        // })?;
        // let rpc_url = env::var("SP1_RPC_URL").map_err(|_| {
        //     ProverError::GuestError("SP1_RPC_URL must be set for remote proving".to_owned())
        // })?;
        // let network_client = NetworkClient::new(&private_key, &rpc_url);
        // network_client
        //     .unclaim_proof(proof_id, UnclaimReason::Abandoned, "".to_owned())
        //     .await
        //     .map_err(|_| ProverError::GuestError("Sp1: couldn't unclaim proof".to_owned()))?;
        id_store.remove_id(key).await?;
        Ok(())
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let mut param = Sp1Param::deserialize(config.get("sp1").unwrap()).unwrap();

        // TODO: remove param.recursion, hardcode to Plonk
        param.recursion = RecursionMode::Plonk;

        let mode = param.prover.clone().unwrap_or_else(get_env_mock);
        let block_inputs: Vec<B256> = input
            .proofs
            .iter()
            .map(|proof| proof.input.unwrap())
            .collect::<Vec<_>>();
        let block_proof_vk = serde_json::from_str::<SP1VerifyingKey>(
            &input.proofs.first().unwrap().uuid.clone().unwrap(),
        )
        .map_err(|e| ProverError::GuestError(format!("Failed to parse SP1 vk: {e}")))?;
        let stark_vk = block_proof_vk.vk.clone();
        let image_id = block_proof_vk.hash_u32();
        let aggregation_input = ZkAggregationGuestInput {
            image_id,
            block_inputs,
        };
        info!(
            "Collect {:?} proofs aggregation pi inputs: {:?}",
            input.proofs.len(),
            aggregation_input.block_inputs
        );

        let mut stdin = SP1Stdin::new();
        stdin.write(&aggregation_input);
        for proof in input.proofs.iter() {
            let sp1_proof = serde_json::from_str::<SP1Proof>(&proof.quote.clone().unwrap())
                .map_err(|e| ProverError::GuestError(format!("Failed to parse SP1 proof: {e}")))?;
            match sp1_proof {
                SP1Proof::Compressed(block_proof) => {
                    stdin.write_proof(*block_proof, stark_vk.clone());
                }
                _ => {
                    tracing::error!("unsupported proof type for aggregation: {sp1_proof:?}");
                }
            }
        }

        // Generate the proof for the given program.
        let Sp1ProverClient {
            client,
            pk,
            vk,
            network_client,
        } = AGGREGATION_CLIENT
            .entry(param.prover.clone().unwrap_or_else(get_env_mock))
            .or_insert_with(|| {
                let network_client = Arc::new(ProverClient::builder().network().build());
                let base_client: Box<dyn SP1ProverTrait<CpuProverComponents>> = param
                    .prover
                    .map(|mode| {
                        let prover: Box<dyn SP1ProverTrait<CpuProverComponents>> = match mode {
                            ProverMode::Mock => Box::new(ProverClient::builder().mock().build()),
                            ProverMode::Local => Box::new(ProverClient::builder().cpu().build()),
                            ProverMode::Network => {
                                Box::new(ProverClient::builder().network().build())
                            }
                        };
                        prover
                    })
                    .unwrap_or_else(|| Box::new(ProverClient::from_env()));

                let client = Arc::new(base_client);
                let (pk, vk) = client.setup(AGGREGATION_ELF);
                info!(
                    "new client and setup() for aggregation based on {:?} proofs with vk {:?}",
                    input.proofs.len(),
                    vk.bytes32()
                );
                Sp1ProverClient {
                    client,
                    pk,
                    vk,
                    network_client,
                }
            })
            .clone();
        info!(
            "sp1 aggregate: {:?} based {:?} blocks with vk {:?}",
            reth_primitives::hex::encode_prefixed(stark_vk.hash_bytes()),
            input.proofs.len(),
            vk.bytes32()
        );

        let prove_result = if !matches!(mode, ProverMode::Network) {
            let prove_result = client
                .prove(&pk, &stdin, SP1ProofMode::Plonk)
                .expect("proving failed");
            prove_result
        } else {
            let proof_id = network_client
                .prove(&pk, &stdin)
                .mode(param.recursion.clone().into())
                .cycle_limit(1_000_000_000_000)
                .skip_simulation(true)
                .strategy(FulfillmentStrategy::Reserved)
                .request_async()
                .await
                .map_err(|e| {
                    ProverError::GuestError(format!("Sp1: network proving failed: {e}"))
                })?;
            info!("Sp1: network proof id: {proof_id:?} for aggregation");
            network_client
                .wait_proof(proof_id.clone(), Some(Duration::from_secs(3600)))
                .await
                .map_err(|e| ProverError::GuestError(format!("Sp1: network proof failed {e:?}")))?
        };

        let proof_bytes = prove_result.bytes();
        if param.verify && !proof_bytes.is_empty() {
            let time = Measurement::start("verify", false);
            let aggregation_pi = prove_result.clone().borrow_mut().public_values.raw();
            let fixture = RaikoProofFixture {
                vkey: vk.bytes32().to_string(),
                public_values: aggregation_pi,
                proof: proof_bytes.clone(),
            };

            verify_sol_by_contract_call(&fixture).await?;
            time.stop_with("==> Aggregation verification complete");
        }

        let proof = (!proof_bytes.is_empty()).then_some(
            // 0x + 64 bytes of the vkey + the proof
            // vkey itself contains 0x prefix
            format!(
                "{}{}{}",
                vk.bytes32(),
                reth_primitives::hex::encode(stark_vk.hash_bytes()),
                reth_primitives::hex::encode(proof_bytes)
            ),
        );

        Ok::<_, ProverError>(
            Sp1Response {
                proof,
                sp1_proof: None,
                vkey: None,
            }
            .into(),
        )
    }

    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let mut param = Sp1Param::deserialize(config.get("sp1").unwrap()).unwrap();

        // TODO: remove param.recursion, hardcode to Compressed
        param.recursion = RecursionMode::Compressed;

        let mode = param.prover.clone().unwrap_or_else(get_env_mock);

        println!("batch_run param: {param:?}");
        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        let Sp1ProverClient {
            client,
            pk,
            vk,
            network_client,
        } = BATCH_PROOF_CLIENT
            .entry(mode.clone())
            .or_insert_with(|| {
                let network_client = Arc::new(ProverClient::builder().network().build());
                let base_client: Box<dyn SP1ProverTrait<CpuProverComponents>> = match mode {
                    ProverMode::Mock => Box::new(ProverClient::builder().mock().build()),
                    ProverMode::Local => Box::new(ProverClient::builder().cpu().build()),
                    ProverMode::Network => Box::new(ProverClient::builder().network().build()),
                };

                let client = Arc::new(base_client);
                let (pk, vk) = client.setup(BATCH_ELF);
                info!(
                    "new client and setup() for batch {:?}.",
                    input.taiko.batch_id
                );
                Sp1ProverClient {
                    client,
                    network_client,
                    pk,
                    vk,
                }
            })
            .clone();

        info!(
            "Sp1 Prover: batch {:?} with vk {:?}, output.hash: {}",
            input.taiko.batch_id,
            vk.bytes32(),
            output.hash
        );

        let prove_result = if !matches!(mode, ProverMode::Network) {
            let prove_mode = match param.recursion {
                RecursionMode::Core => SP1ProofMode::Core,
                RecursionMode::Compressed => SP1ProofMode::Compressed,
                RecursionMode::Plonk => SP1ProofMode::Plonk,
            };
            let profiling = std::env::var("PROFILING").unwrap_or_default() == "1";
            if profiling {
                info!(
                    "Profiling locally with recursion mode: {:?}",
                    param.recursion
                );
                client.execute(BATCH_ELF, &stdin).map_err(|e| {
                    ProverError::GuestError(format!("Sp1: local proving failed: {e}"))
                })?;
                SP1ProofWithPublicValues {
                    proof: SP1Proof::Groth16(Groth16Bn254Proof::default()),
                    public_values: sp1_primitives::io::SP1PublicValues::new(),
                    sp1_version: "0".to_owned(),
                }
            } else {
                info!("Execute locally with recursion mode: {:?}", param.recursion);
                client.prove(&pk, &stdin, prove_mode).map_err(|e| {
                    ProverError::GuestError(format!("Sp1: local proving failed: {e}"))
                })?
            }
        } else {
            let proof_id = network_client
                .prove(&pk, &stdin)
                .mode(param.recursion.clone().into())
                .cycle_limit(1_000_000_000_000)
                .skip_simulation(true)
                .strategy(FulfillmentStrategy::Reserved)
                .request_async()
                .await
                .map_err(|e| {
                    ProverError::GuestError(format!("Sp1: requesting proof failed: {e}"))
                })?;
            if let Some(id_store) = id_store {
                id_store
                    .store_id(
                        (
                            input.taiko.chain_spec.chain_id,
                            input.taiko.batch_id,
                            output.hash,
                            ProofType::Sp1 as u8,
                        ),
                        proof_id.clone().to_string(),
                    )
                    .await?;
            }
            info!(
                "Sp1 Prover: batch {:?} - proof id {proof_id:?}",
                input.taiko.batch_id
            );
            network_client
                .wait_proof(proof_id.clone(), Some(Duration::from_secs(3600)))
                .await
                .map_err(|e| ProverError::GuestError(format!("Sp1: network proof failed {e:?}")))?
        };

        let proof_bytes = match param.recursion {
            RecursionMode::Compressed => {
                info!("Compressed proof is used in aggregation mode only");
                vec![]
            }
            _ => prove_result.bytes(),
        };
        if param.verify && !proof_bytes.is_empty() {
            let time = Measurement::start("verify", false);
            let pi_hash = prove_result
                .clone()
                .borrow_mut()
                .public_values
                .read::<[u8; 32]>();
            let fixture = RaikoProofFixture {
                vkey: vk.bytes32(),
                public_values: B256::from_slice(&pi_hash).to_string(),
                proof: proof_bytes.clone(),
            };

            verify_sol_by_contract_call(&fixture).await?;
            time.stop_with("==> Verification complete");
        }

        let proof_string = (!proof_bytes.is_empty()).then_some(
            // 0x + 64 bytes of the vkey + the proof
            // vkey itself contains 0x prefix
            format!(
                "{}{}",
                vk.bytes32(),
                reth_primitives::hex::encode(proof_bytes)
            ),
        );

        info!(
            "Sp1 Prover: batch {:?} completed! proof: {proof_string:?}",
            input.taiko.batch_id,
        );
        Ok::<_, ProverError>(
            Sp1Response {
                proof: proof_string,
                sp1_proof: Some(prove_result),
                vkey: Some(vk.clone()),
            }
            .into(),
        )
    }
}

fn get_env_mock() -> ProverMode {
    match env::var("SP1_PROVER")
        .unwrap_or("local".to_string())
        .to_lowercase()
        .as_str()
    {
        "mock" => ProverMode::Mock,
        "local" => ProverMode::Local,
        "network" => ProverMode::Network,
        _ => ProverMode::Local,
    }
}

/// A fixture that can be used to test the verification of SP1 zkVM proofs inside Solidity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RaikoProofFixture {
    vkey: String,
    public_values: String,
    proof: Vec<u8>,
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;
    const TEST_ELF: &[u8] = include_bytes!("../../guest/elf/test-sp1-guest");

    #[test]
    fn test_deserialize_sp1_param() {
        let json = json!(
            {
                "recursion": "core",
                "prover": "network",
                "verify": true
            }
        );
        let param = Sp1Param {
            recursion: RecursionMode::Core,
            prover: Some(ProverMode::Network),
            verify: true,
        };
        let serialized = serde_json::to_value(param).unwrap();
        assert_eq!(json, serialized);

        let deserialized: Sp1Param = serde_json::from_value(serialized).unwrap();
        println!("{json:?} {deserialized:?}");
    }

    #[ignore = "elf needs input, ignore for now"]
    #[test]
    fn run_unittest_elf() {
        // TODO(Cecilia): imple GuestInput::mock() for unit test
        let client = ProverClient::new();
        let stdin = SP1Stdin::new();
        let (pk, vk) = client.setup(TEST_ELF);
        let proof = client.prove(&pk, &stdin).run().unwrap();
        client
            .verify(&proof, &vk)
            .expect("Sp1: verification failed");
    }

    #[ignore = "This is for docker image build only"]
    #[test]
    fn test_show_sp1_elf_vk() {
        let client = ProverClient::new();
        let (_pk, vk) = client.setup(ELF);
        println!("SP1 ELF VK: {:?}", vk.bytes32());
    }
}
