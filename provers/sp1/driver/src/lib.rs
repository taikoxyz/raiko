#![cfg(feature = "enable")]

use alloy_primitives::{hex, B256};
use raiko_lib::{
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ShastaAggregationGuestInput, ShastaSp1AggregationGuestInput,
        ZkAggregationGuestInput,
    },
    libhash::hash_shasta_subproof_input,
    proof_type::ProofType,
    protocol_instance::validate_shasta_proof_carry_data_vec,
    prover::{
        IdStore, IdWrite, Proof, ProofCarryData, ProofKey, Prover, ProverConfig, ProverError,
        ProverResult,
    },
    Measurement,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_with::serde_as;
use sp1_prover::Groth16Bn254Proof;
#[cfg(feature = "network")]
use sp1_sdk::network::FulfillmentStrategy;
use sp1_sdk::{
    Elf, ProveRequest, Prover as SP1ProverTrait, ProvingKey, SP1Proof, SP1ProofMode,
    SP1ProofWithPublicValues, SP1VerifyingKey,
};
use sp1_sdk::{HashableKey, ProverClient, SP1Stdin};
#[cfg(feature = "network")]
use std::time::Duration;
use std::{borrow::BorrowMut, env};
use tokio::sync::OnceCell;
use tracing::info;

mod proof_verify;
use proof_verify::remote_contract_verify::verify_sol_by_contract_call;

pub const AGGREGATION_ELF: Elf = Elf::Static(include_bytes!("../../guest/elf/sp1-aggregation"));
pub const BATCH_ELF: Elf = Elf::Static(include_bytes!("../../guest/elf/sp1-batch"));
pub const SHASTA_AGG_ELF: Elf =
    Elf::Static(include_bytes!("../../guest/elf/sp1-shasta-aggregation"));

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
    /// The proof mode for a Groth16 proof.
    Groth16,
}

impl From<RecursionMode> for SP1ProofMode {
    fn from(value: RecursionMode) -> Self {
        match value {
            RecursionMode::Core => SP1ProofMode::Core,
            RecursionMode::Compressed => SP1ProofMode::Compressed,
            RecursionMode::Plonk => SP1ProofMode::Plonk,
            RecursionMode::Groth16 => SP1ProofMode::Groth16,
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
            extra_data: None,
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

/// Helper: setup a prover client and return (pk, vk).
async fn setup_prover<P: SP1ProverTrait>(
    client: &P,
    elf: Elf,
) -> Result<(P::ProvingKey, SP1VerifyingKey), ProverError> {
    let pk = client
        .setup(elf)
        .await
        .map_err(|e| ProverError::GuestError(format!("Cannot setup elf file: {e}")))?;
    let vk = pk.verifying_key().clone();
    Ok((pk, vk))
}

/// Helper: prove locally with a concrete prover and return the proof.
async fn prove_local<P: SP1ProverTrait>(
    client: &P,
    pk: &P::ProvingKey,
    stdin: SP1Stdin,
    mode: SP1ProofMode,
) -> ProverResult<SP1ProofWithPublicValues> {
    client
        .prove(pk, stdin)
        .mode(mode)
        .await
        .map_err(|e| ProverError::GuestError(format!("Sp1: local proving failed: {e}")))
}

/// Helper: prove on network and return the proof + vk.
#[cfg(feature = "network")]
async fn prove_network(
    stdin: SP1Stdin,
    elf: Elf,
    mode: SP1ProofMode,
) -> ProverResult<(SP1ProofWithPublicValues, SP1VerifyingKey)> {
    let network_client = ProverClient::builder().network().build().await;
    let pk = network_client
        .setup(elf)
        .await
        .map_err(|e| ProverError::GuestError(format!("Sp1: network setup failed: {e}")))?;
    let vk = pk.verifying_key().clone();
    let prove_result = network_client
        .prove(&pk, stdin)
        .mode(mode)
        .cycle_limit(1_000_000_000_000)
        .skip_simulation(true)
        .strategy(FulfillmentStrategy::Reserved)
        .timeout(Duration::from_secs(3600))
        .await
        .map_err(|e| ProverError::GuestError(format!("Sp1: network proving failed: {e}")))?;
    Ok((prove_result, vk))
}

static AGGREGATION_PROGRAM_HASH: OnceCell<String> = OnceCell::const_new();
static BLOCK_PROGRAM_HASH: OnceCell<String> = OnceCell::const_new();
static SHASTA_AGGREGATION_PROGRAM_HASH: OnceCell<String> = OnceCell::const_new();

/// Helper: get program hash from env or use default mock hash.
async fn vk_bytes32(elf: Elf) -> String {
    let client = ProverClient::builder().light().build().await;
    let pk = client.setup(elf).await.expect("ELF setup failed");
    pk.verifying_key().bytes32()
}

/// Helper: get program hash from env or use default mock hash.
async fn vk_hash_hex(elf: Elf) -> String {
    let client = ProverClient::builder().light().build().await;
    let pk = client.setup(elf).await.expect("ELF setup failed");
    hex::encode(pk.verifying_key().hash_bytes())
}

impl Prover for Sp1Prover {
    async fn run(
        &self,
        _input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        unimplemented!("no block run after pacaya fork")
    }

    async fn cancel(&self, key: ProofKey, id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
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

        // TODO: remove param.recursion, hardcode to Groth16
        param.recursion = RecursionMode::Groth16;

        let prove_mode: SP1ProofMode = param.recursion.clone().into();
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
        // Each prover type is a different concrete type, so we must handle them separately.
        let gpu_number: u32 = config
            .get("gpu_number")
            .and_then(|v| v.as_i64())
            .map(|v| v as u32)
            .unwrap();
        info!("GPU Number: {}", gpu_number);

        let (prove_result, vk) = match mode {
            ProverMode::Mock => {
                let client = ProverClient::builder().mock().build().await;
                let (pk, vk) = setup_prover(&client, AGGREGATION_ELF).await?;
                info!(
                    "sp1 aggregate: {:?} based {:?} blocks with vk {:?}",
                    hex::encode_prefixed(stark_vk.hash_bytes()),
                    input.proofs.len(),
                    vk.bytes32()
                );
                let result = prove_local(&client, &pk, stdin, prove_mode).await?;
                (result, vk)
            }
            ProverMode::Local => {
                let client = ProverClient::builder()
                    .cuda()
                    // .with_device_id(gpu_number)
                    .build()
                    .await;
                let (pk, vk) = setup_prover(&client, AGGREGATION_ELF).await?;
                info!(
                    "sp1 aggregate: {:?} based {:?} blocks with vk {:?}",
                    hex::encode_prefixed(stark_vk.hash_bytes()),
                    input.proofs.len(),
                    vk.bytes32()
                );
                let result = prove_local(&client, &pk, stdin, prove_mode).await?;
                (result, vk)
            }
            #[cfg(feature = "network")]
            ProverMode::Network => {
                let (result, vk) = prove_network(stdin, AGGREGATION_ELF, prove_mode).await?;
                info!(
                    "sp1 aggregate: {:?} based {:?} blocks with vk {:?}",
                    hex::encode_prefixed(stark_vk.hash_bytes()),
                    input.proofs.len(),
                    vk.bytes32()
                );
                (result, vk)
            }
            #[cfg(not(feature = "network"))]
            ProverMode::Network => {
                return Err(ProverError::GuestError(
                    "Network prover requires 'network' feature".to_string(),
                ));
            }
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
                hex::encode(stark_vk.hash_bytes()),
                hex::encode(proof_bytes)
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
        #[allow(unused_variables)] id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let mut param = Sp1Param::deserialize(config.get("sp1").unwrap()).unwrap();

        // TODO: remove param.recursion, hardcode to Compressed
        param.recursion = RecursionMode::Compressed;

        let mode = param.prover.clone().unwrap_or_else(get_env_mock);

        println!("batch_run param: {param:?}");
        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        let gpu_number: u32 = config
            .get("gpu_number")
            .and_then(|v| v.as_i64())
            .map(|v| v as u32)
            .unwrap();
        info!("GPU Number: {}", gpu_number);

        let prove_mode: SP1ProofMode = param.recursion.clone().into();

        // Each prover type is a different concrete type, so we must handle them separately.
        let (prove_result, vk) = match mode {
            ProverMode::Mock => {
                let client = ProverClient::builder().mock().build().await;
                let (pk, vk) = setup_prover(&client, BATCH_ELF).await?;
                info!(
                    "new client and setup() for batch {:?}.",
                    input.taiko.batch_id
                );
                info!(
                    "Sp1 Prover: batch {:?} with vk {:?}, output.hash: {}",
                    input.taiko.batch_id,
                    vk.bytes32(),
                    output.hash
                );
                let profiling = std::env::var("PROFILING").unwrap_or_default() == "1";
                let result = if profiling {
                    info!(
                        "Profiling locally with recursion mode: {:?}",
                        param.recursion
                    );
                    client.execute(BATCH_ELF, stdin).await.map_err(|e| {
                        ProverError::GuestError(format!("Sp1: local profiling failed: {e}"))
                    })?;
                    SP1ProofWithPublicValues {
                        proof: SP1Proof::Groth16(Groth16Bn254Proof::default()),
                        public_values: sp1_primitives::io::SP1PublicValues::new(),
                        sp1_version: "0".to_owned(),
                        tee_proof: None,
                    }
                } else {
                    info!("Execute locally with recursion mode: {:?}", param.recursion);
                    prove_local(&client, &pk, stdin, prove_mode).await?
                };
                (result, vk)
            }
            ProverMode::Local => {
                let client = ProverClient::builder()
                    .cuda()
                    // .with_device_id(gpu_number)
                    .build()
                    .await;
                let (pk, vk) = setup_prover(&client, BATCH_ELF).await?;
                info!(
                    "new client and setup() for batch {:?}.",
                    input.taiko.batch_id
                );
                info!(
                    "Sp1 Prover: batch {:?} with vk {:?}, output.hash: {}",
                    input.taiko.batch_id,
                    vk.bytes32(),
                    output.hash
                );
                let profiling = std::env::var("PROFILING").unwrap_or_default() == "1";
                let result = if profiling {
                    info!(
                        "Profiling locally with recursion mode: {:?}",
                        param.recursion
                    );
                    client.execute(BATCH_ELF, stdin).await.map_err(|e| {
                        ProverError::GuestError(format!("Sp1: local profiling failed: {e}"))
                    })?;
                    SP1ProofWithPublicValues {
                        proof: SP1Proof::Groth16(Groth16Bn254Proof::default()),
                        public_values: sp1_primitives::io::SP1PublicValues::new(),
                        sp1_version: "0".to_owned(),
                        tee_proof: None,
                    }
                } else {
                    info!("Execute locally with recursion mode: {:?}", param.recursion);
                    prove_local(&client, &pk, stdin, prove_mode).await?
                };
                (result, vk)
            }
            #[cfg(feature = "network")]
            ProverMode::Network => {
                let network_client = ProverClient::builder().network().build().await;
                let (pk, vk) = setup_prover(&network_client, BATCH_ELF).await?;
                info!(
                    "new client and setup() for batch {:?}.",
                    input.taiko.batch_id
                );
                info!(
                    "Sp1 Prover: batch {:?} with vk {:?}, output.hash: {}",
                    input.taiko.batch_id,
                    vk.bytes32(),
                    output.hash
                );
                let prove_result = network_client
                    .prove(&pk, stdin)
                    .mode(prove_mode)
                    .cycle_limit(1_000_000_000_000)
                    .skip_simulation(true)
                    .strategy(FulfillmentStrategy::Reserved)
                    .timeout(Duration::from_secs(3600))
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
                            "network_proof".to_string(),
                        )
                        .await?;
                }
                info!(
                    "Sp1 Prover: batch {:?} - network proof completed",
                    input.taiko.batch_id
                );
                (prove_result, vk)
            }
            #[cfg(not(feature = "network"))]
            ProverMode::Network => {
                return Err(ProverError::GuestError(
                    "Network prover requires 'network' feature".to_string(),
                ));
            }
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
            format!("{}{}", vk.bytes32(), hex::encode(proof_bytes)),
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

    async fn shasta_aggregate(
        &self,
        input: ShastaAggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let mut param = Sp1Param::deserialize(config.get("sp1").unwrap()).unwrap();
        param.recursion = RecursionMode::Groth16;

        let prove_mode: SP1ProofMode = param.recursion.clone().into();
        let mode = param.prover.clone().unwrap_or_else(get_env_mock);
        let first_proof = input.proofs.first().ok_or_else(|| {
            ProverError::GuestError("empty shasta aggregation request".to_string())
        })?;
        let vk_str = first_proof.uuid.clone().ok_or_else(|| {
            ProverError::GuestError("missing verifying key for shasta aggregation".to_string())
        })?;
        let block_proof_vk: SP1VerifyingKey = serde_json::from_str(&vk_str)
            .map_err(|e| ProverError::GuestError(format!("Failed to parse SP1 vk: {e}")))?;
        let stark_vk = block_proof_vk.vk.clone();
        let image_id = block_proof_vk.hash_u32();

        let proof_carry_data_vec: Vec<ProofCarryData> = input
            .proofs
            .iter()
            .map(|proof| {
                proof.extra_data.clone().ok_or_else(|| {
                    ProverError::GuestError("missing shasta proof carry data".into())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let block_inputs = build_shasta_block_inputs(&input.proofs, &proof_carry_data_vec)?;

        let shasta_input = ShastaSp1AggregationGuestInput {
            image_id,
            block_inputs,
            proof_carry_data_vec,
        };

        let mut stdin = SP1Stdin::new();
        stdin.write(&shasta_input);
        for proof in input.proofs.iter() {
            let quote = proof.quote.as_ref().ok_or_else(|| {
                ProverError::GuestError("missing quote for shasta aggregation proof".to_string())
            })?;
            let sp1_proof = serde_json::from_str::<SP1Proof>(quote)
                .map_err(|e| ProverError::GuestError(format!("Failed to parse SP1 proof: {e}")))?;
            match sp1_proof {
                SP1Proof::Compressed(block_proof) => {
                    stdin.write_proof(*block_proof, stark_vk.clone());
                }
                _ => {
                    return Err(ProverError::GuestError(
                        "unsupported proof type for shasta aggregation".to_string(),
                    ))
                }
            }
        }

        let gpu_number: u32 = config
            .get("gpu_number")
            .and_then(|v| v.as_i64())
            .map(|v| v as u32)
            .unwrap();
        info!("GPU Number: {}", gpu_number);

        // Each prover type is a different concrete type, so we must handle them separately.
        let (prove_result, vk) = match mode {
            ProverMode::Mock => {
                let client = ProverClient::builder().mock().build().await;
                let (pk, vk) = setup_prover(&client, SHASTA_AGG_ELF).await?;
                info!(
                    "Sp1 Shasta aggregation: {} proofs with vk {:?}",
                    input.proofs.len(),
                    vk.bytes32()
                );
                let result = prove_local(&client, &pk, stdin, prove_mode).await?;
                (result, vk)
            }
            ProverMode::Local => {
                let client = ProverClient::builder()
                    .cuda()
                    // .with_device_id(gpu_number)
                    .build()
                    .await;
                let (pk, vk) = setup_prover(&client, SHASTA_AGG_ELF).await?;
                info!(
                    "Sp1 Shasta aggregation: {} proofs with vk {:?}",
                    input.proofs.len(),
                    vk.bytes32()
                );
                let result = prove_local(&client, &pk, stdin, prove_mode).await?;
                (result, vk)
            }
            #[cfg(feature = "network")]
            ProverMode::Network => {
                let (result, vk) = prove_network(stdin, SHASTA_AGG_ELF, prove_mode).await?;
                info!(
                    "Sp1 Shasta aggregation: {} proofs with vk {:?}",
                    input.proofs.len(),
                    vk.bytes32()
                );
                (result, vk)
            }
            #[cfg(not(feature = "network"))]
            ProverMode::Network => {
                return Err(ProverError::GuestError(
                    "Network prover requires 'network' feature".to_string(),
                ));
            }
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
            time.stop_with("==> Shasta aggregation verification complete");
        }

        let proof = (!proof_bytes.is_empty()).then_some(format!(
            "{}{}{}",
            vk.bytes32(),
            hex::encode(stark_vk.hash_bytes()),
            hex::encode(proof_bytes)
        ));

        Ok::<_, ProverError>(
            Sp1Response {
                proof,
                sp1_proof: Some(prove_result),
                vkey: Some(vk.clone()),
            }
            .into(),
        )
    }

    fn proof_type(&self) -> ProofType {
        ProofType::Sp1
    }

    async fn get_guest_data() -> ProverResult<serde_json::Value> {
        let agg = AGGREGATION_PROGRAM_HASH
            .get_or_init(|| vk_bytes32(AGGREGATION_ELF))
            .await;
        let block = BLOCK_PROGRAM_HASH
            .get_or_init(|| vk_hash_hex(BATCH_ELF))
            .await;
        let shasta = SHASTA_AGGREGATION_PROGRAM_HASH
            .get_or_init(|| vk_bytes32(SHASTA_AGG_ELF))
            .await;
        Ok(json!({
            "sp1": {
                "aggregation_program_hash": agg,
                "block_program_hash": block,
                "shasta_aggregation_program_hash": shasta,
            }
        }))
    }
}

fn build_shasta_block_inputs(
    proofs: &[Proof],
    proof_carry_data_vec: &[ProofCarryData],
) -> ProverResult<Vec<B256>> {
    if proofs.len() != proof_carry_data_vec.len() {
        return Err(ProverError::GuestError(
            "shasta proofs length mismatch with carry data".to_string(),
        ));
    }
    if !validate_shasta_proof_carry_data_vec(proof_carry_data_vec) {
        return Err(ProverError::GuestError(
            "invalid shasta proof carry data".to_string(),
        ));
    }

    let mut block_inputs = Vec::with_capacity(proofs.len());
    for (idx, (proof, carry)) in proofs.iter().zip(proof_carry_data_vec).enumerate() {
        let proof_input = proof.input.ok_or_else(|| {
            ProverError::GuestError("missing public input for shasta aggregation proof".to_string())
        })?;
        let expected = hash_shasta_subproof_input(carry);
        if proof_input != expected {
            return Err(ProverError::GuestError(format!(
                "shasta proof input mismatch at index {idx}"
            )));
        }
        block_inputs.push(proof_input);
    }

    Ok(block_inputs)
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
    const TEST_ELF: &[u8] = include_bytes!("../../guest/elf/test-sp1-batch");

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
    #[tokio::test]
    async fn run_unittest_elf() {
        // TODO(Cecilia): imple GuestInput::mock() for unit test
        let client = ProverClient::builder().cpu().build().await;
        let stdin = SP1Stdin::new();
        let pk = client.setup(Elf::Static(TEST_ELF)).await.unwrap();
        let vk = pk.verifying_key().clone();
        let proof = client.prove(&pk, stdin).compressed().await.unwrap();
        client
            .verify(&proof, &vk, None)
            .expect("Sp1: verification failed");
    }

    #[ignore = "This is for docker image build only"]
    #[tokio::test]
    async fn test_show_sp1_elf_vk() {
        let client = ProverClient::from_env().await;
        let pk = client.setup(BATCH_ELF).await.unwrap();
        println!("SP1 ELF VK: {:?}", pk.verifying_key().bytes32());
    }
}
