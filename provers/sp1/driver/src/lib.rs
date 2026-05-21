#![cfg(feature = "enable")]

use dashmap::DashMap;
use once_cell::sync::Lazy;
use raiko_lib::{
    input::{
        AggregationGuestOutput, GuestBatchInput, GuestBatchOutput, GuestInput, GuestOutput,
        ShastaAggregationGuestInput, ShastaSp1AggregationGuestInput,
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
use reth_primitives::B256;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use sp1_prover::Groth16Bn254Proof;
use sp1_sdk::{
    blocking::{EnvProver, EnvProvingKey, ProveRequest, Prover as SP1ProverTrait, ProverClient},
    network::{
        get_default_rpc_url_for_mode, signer::NetworkSigner, FulfillmentStrategy, NetworkMode,
    },
    Elf, NetworkProver as AsyncNetworkProver, ProveRequest as SP1AsyncProveRequestTrait,
    Prover as SP1AsyncProverTrait, ProvingKey, SP1Proof, SP1ProofMode, SP1ProofWithPublicValues,
    SP1ProvingKey, SP1VerifyingKey,
};
use sp1_sdk::{HashableKey, SP1Stdin};
use std::{borrow::BorrowMut, env, sync::Arc, time::Duration};
use tracing::info;

mod proof_verify;
use proof_verify::remote_contract_verify::verify_sol_by_contract_call;

pub const BATCH_ELF: &[u8] = include_bytes!("../../guest/elf/sp1-batch");
pub const SHASTA_AGG_ELF: &[u8] = include_bytes!("../../guest/elf/sp1-shasta-aggregation");

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sp1Param {
    #[serde(default = "RecursionMode::default")]
    pub recursion: RecursionMode,
    pub prover: Option<ProverMode>,
    #[serde(default = "DEFAULT_TRUE")]
    pub verify: bool,
    #[serde(default, skip_serializing_if = "is_default_sp1_network_mode")]
    pub network_mode: Sp1NetworkMode,
    #[serde(default, skip_serializing_if = "is_default_sp1_fulfillment_strategy")]
    pub fulfillment_strategy: Sp1FulfillmentStrategy,
}

const DEFAULT_TRUE: fn() -> bool = || true;

fn is_default_sp1_network_mode(mode: &Sp1NetworkMode) -> bool {
    *mode == Sp1NetworkMode::default()
}

fn is_default_sp1_fulfillment_strategy(strategy: &Sp1FulfillmentStrategy) -> bool {
    *strategy == Sp1FulfillmentStrategy::default()
}

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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Sp1NetworkMode {
    #[default]
    Reserved,
    Mainnet,
}

impl From<Sp1NetworkMode> for NetworkMode {
    fn from(value: Sp1NetworkMode) -> Self {
        match value {
            Sp1NetworkMode::Reserved => NetworkMode::Reserved,
            Sp1NetworkMode::Mainnet => NetworkMode::Mainnet,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Sp1FulfillmentStrategy {
    #[default]
    Reserved,
    Hosted,
    Auction,
}

impl From<Sp1FulfillmentStrategy> for FulfillmentStrategy {
    fn from(value: Sp1FulfillmentStrategy) -> Self {
        match value {
            Sp1FulfillmentStrategy::Reserved => FulfillmentStrategy::Reserved,
            Sp1FulfillmentStrategy::Hosted => FulfillmentStrategy::Hosted,
            Sp1FulfillmentStrategy::Auction => FulfillmentStrategy::Auction,
        }
    }
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

#[derive(Clone)]
enum Sp1ProverClient {
    Local {
        client: Arc<EnvProver>,
        pk: EnvProvingKey,
        vk: SP1VerifyingKey,
    },
    Network {
        client: Arc<AsyncNetworkProver>,
        pk: SP1ProvingKey,
        vk: SP1VerifyingKey,
    },
}

impl Sp1ProverClient {
    fn vk(&self) -> &SP1VerifyingKey {
        match self {
            Self::Local { vk, .. } | Self::Network { vk, .. } => vk,
        }
    }
}

//TODO: use prover object to save such local storage members.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Sp1ProverClientKey {
    mode: ProverMode,
    network_mode: Option<Sp1NetworkMode>,
}

static SHASTA_AGG_CLIENT: Lazy<DashMap<Sp1ProverClientKey, Sp1ProverClient>> =
    Lazy::new(DashMap::new);
static BATCH_PROOF_CLIENT: Lazy<DashMap<Sp1ProverClientKey, Sp1ProverClient>> =
    Lazy::new(DashMap::new);

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

        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        let (prover_client, built_client) =
            get_or_build_sp1_prover_client(&BATCH_PROOF_CLIENT, mode.clone(), BATCH_ELF, &param)
                .await?;
        if built_client {
            info!(
                "new client and setup() for batch {:?}.",
                input.taiko.batch_id
            );
        }
        let vk = prover_client.vk().clone();

        info!(
            "Sp1 Prover: batch {:?} with vk {:?}, output.hash: {}",
            input.taiko.batch_id,
            vk.bytes32(),
            output.hash
        );

        let prove_result = match prover_client {
            Sp1ProverClient::Local { client, pk, .. } => {
                let recursion = param.recursion.clone();
                let prove_mode = SP1ProofMode::from(recursion.clone());
                let profiling = std::env::var("PROFILING").unwrap_or_default() == "1";
                run_sp1_blocking("local batch proving", move || {
                    if profiling {
                        info!("Profiling locally with recursion mode: {:?}", recursion);
                        let (public_values, _report) = client
                            .execute(Elf::Static(BATCH_ELF), stdin)
                            .run()
                            .map_err(|e| {
                                ProverError::GuestError(format!("Sp1: local proving failed: {e}"))
                            })?;
                        Ok(SP1ProofWithPublicValues {
                            proof: SP1Proof::Groth16(Groth16Bn254Proof::default()),
                            public_values,
                            sp1_version: "0".to_owned(),
                            tee_proof: None,
                        })
                    } else {
                        info!("Execute locally with recursion mode: {:?}", recursion);
                        client
                            .prove(&pk, stdin)
                            .mode(prove_mode)
                            .run()
                            .map_err(|e| {
                                ProverError::GuestError(format!("Sp1: local proving failed: {e}"))
                            })
                    }
                })
                .await?
            }
            Sp1ProverClient::Network { client, pk, .. } => {
                let recursion = param.recursion.clone();
                let client_for_request = client.clone();
                let proof_id = client_for_request
                    .prove(&pk, stdin)
                    .mode(recursion.into())
                    .cycle_limit(1_000_000_000_000)
                    .skip_simulation(true)
                    .strategy(param.fulfillment_strategy.into())
                    .request()
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
                            proof_id.to_string(),
                        )
                        .await?;
                }
                info!(
                    "Sp1 Prover: batch {:?} - proof id {proof_id:?}",
                    input.taiko.batch_id
                );
                let proof_id_for_wait = proof_id.clone();
                let client_for_wait = client.clone();
                client_for_wait
                    .wait_proof(proof_id_for_wait, Some(sp1_network_wait_timeout()), None)
                    .await
                    .map_err(|e| {
                        ProverError::GuestError(format!("Sp1: network proof failed {e:?}"))
                    })?
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

    async fn shasta_aggregate(
        &self,
        input: ShastaAggregationGuestInput,
        _output: &AggregationGuestOutput,
        config: &ProverConfig,
        _store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let mut param = Sp1Param::deserialize(config.get("sp1").unwrap()).unwrap();
        param.recursion = RecursionMode::Plonk;

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

        let (prover_client, built_client) = get_or_build_sp1_prover_client(
            &SHASTA_AGG_CLIENT,
            mode.clone(),
            SHASTA_AGG_ELF,
            &param,
        )
        .await?;
        if built_client {
            info!("new client and setup() for shasta aggregation");
        }
        let vk = prover_client.vk().clone();
        info!(
            "Sp1 Shasta aggregation: {} proofs with vk {:?}",
            input.proofs.len(),
            vk.bytes32()
        );

        let prove_result =
            match prover_client {
                Sp1ProverClient::Local { client, pk, .. } => {
                    run_sp1_blocking("local shasta aggregation proving", move || {
                        client.prove(&pk, stdin).plonk().run().map_err(|e| {
                            ProverError::GuestError(format!("Sp1: proving failed: {e}"))
                        })
                    })
                    .await?
                }
                Sp1ProverClient::Network { client, pk, .. } => {
                    let recursion = param.recursion.clone();
                    let client_for_request = client.clone();
                    let proof_id = client_for_request
                        .prove(&pk, stdin)
                        .mode(recursion.into())
                        .cycle_limit(1_000_000_000_000)
                        .skip_simulation(true)
                        .strategy(param.fulfillment_strategy.into())
                        .request()
                        .await
                        .map_err(|e| {
                            ProverError::GuestError(format!("Sp1: network proving failed: {e}"))
                        })?;
                    info!("Sp1: network proof id: {proof_id:?} for shasta aggregation");
                    let proof_id_for_wait = proof_id.clone();
                    let client_for_wait = client.clone();
                    client_for_wait
                        .wait_proof(proof_id_for_wait, Some(sp1_network_wait_timeout()), None)
                        .await
                        .map_err(|e| {
                            ProverError::GuestError(format!("Sp1: network proof failed {e:?}"))
                        })?
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
            reth_primitives::hex::encode(stark_vk.hash_bytes()),
            reth_primitives::hex::encode(proof_bytes)
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

fn sp1_network_wait_timeout() -> Duration {
    Duration::from_secs(
        env::var("RAIKO_SP1_NETWORK_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(3_600),
    )
}

fn sp1_prover_client_key(mode: ProverMode, param: &Sp1Param) -> Sp1ProverClientKey {
    Sp1ProverClientKey {
        network_mode: (mode == ProverMode::Network).then_some(param.network_mode),
        mode,
    }
}

async fn get_or_build_sp1_prover_client(
    cache: &DashMap<Sp1ProverClientKey, Sp1ProverClient>,
    mode: ProverMode,
    elf: &'static [u8],
    param: &Sp1Param,
) -> ProverResult<(Sp1ProverClient, bool)> {
    let key = sp1_prover_client_key(mode.clone(), param);
    if let Some(client) = cache.get(&key) {
        return Ok((client.clone(), false));
    }

    let client = build_sp1_prover_client(mode, elf, param).await?;
    cache.insert(key, client.clone());
    Ok((client, true))
}

fn validate_sp1_network_config(param: &Sp1Param) -> ProverResult<()> {
    match (param.network_mode, param.fulfillment_strategy) {
        (Sp1NetworkMode::Mainnet, Sp1FulfillmentStrategy::Auction)
        | (
            Sp1NetworkMode::Reserved,
            Sp1FulfillmentStrategy::Reserved | Sp1FulfillmentStrategy::Hosted,
        ) => Ok(()),
        (Sp1NetworkMode::Mainnet, strategy) => Err(ProverError::GuestError(format!(
            "Sp1: network_mode=mainnet requires fulfillment_strategy=auction, got {strategy:?}"
        ))),
        (Sp1NetworkMode::Reserved, strategy) => Err(ProverError::GuestError(format!(
            "Sp1: network_mode=reserved requires fulfillment_strategy=reserved or hosted, got {strategy:?}"
        ))),
    }
}

fn sp1_network_private_key() -> ProverResult<String> {
    env::var("NETWORK_PRIVATE_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ProverError::GuestError(
                "Sp1: NETWORK_PRIVATE_KEY must be set for network proving".to_string(),
            )
        })
}

fn sp1_network_rpc_url(network_mode: Sp1NetworkMode) -> String {
    env::var("NETWORK_RPC_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| get_default_rpc_url_for_mode(network_mode.into()))
}

async fn run_sp1_blocking<T, F>(task_name: &'static str, task: F) -> ProverResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> ProverResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|e| ProverError::GuestError(format!("Sp1: {task_name} task failed: {e}")))?
}

async fn build_sp1_prover_client(
    mode: ProverMode,
    elf: &'static [u8],
    param: &Sp1Param,
) -> ProverResult<Sp1ProverClient> {
    match mode {
        ProverMode::Mock | ProverMode::Local => {
            run_sp1_blocking("local setup", move || {
                let client = Arc::new(match mode {
                    ProverMode::Mock => EnvProver::Mock(ProverClient::builder().mock().build()),
                    ProverMode::Local => EnvProver::Cpu(ProverClient::builder().cpu().build()),
                    ProverMode::Network => unreachable!(),
                });
                let pk = client
                    .setup(Elf::Static(elf))
                    .map_err(|e| ProverError::GuestError(format!("Sp1: setup failed: {e}")))?;
                let vk = pk.verifying_key().clone();
                Ok(Sp1ProverClient::Local { client, pk, vk })
            })
            .await
        }
        ProverMode::Network => {
            validate_sp1_network_config(param)?;
            let private_key = sp1_network_private_key()?;
            let signer = NetworkSigner::local(&private_key).map_err(|e| {
                ProverError::GuestError(format!(
                    "Sp1: NETWORK_PRIVATE_KEY is not a valid network signer: {e}"
                ))
            })?;
            let rpc_url = sp1_network_rpc_url(param.network_mode);
            let client = Arc::new(
                AsyncNetworkProver::new(signer, &rpc_url, param.network_mode.into()).await,
            );
            let pk = client
                .setup(Elf::Static(elf))
                .await
                .map_err(|e| ProverError::GuestError(format!("Sp1: setup failed: {e}")))?;
            let vk = pk.verifying_key().clone();
            Ok(Sp1ProverClient::Network { client, pk, vk })
        }
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
            network_mode: Sp1NetworkMode::Reserved,
            fulfillment_strategy: Sp1FulfillmentStrategy::Reserved,
        };
        let serialized = serde_json::to_value(param).unwrap();
        assert_eq!(json, serialized);

        let deserialized: Sp1Param = serde_json::from_value(serialized).unwrap();
        println!("{json:?} {deserialized:?}");
    }

    #[test]
    fn test_sp1_network_defaults_match_reserved_capacity() {
        let param: Sp1Param = serde_json::from_value(json!({
            "recursion": "plonk",
            "prover": "network",
            "verify": true
        }))
        .unwrap();

        assert_eq!(param.network_mode, Sp1NetworkMode::Reserved);
        assert_eq!(param.fulfillment_strategy, Sp1FulfillmentStrategy::Reserved);
        validate_sp1_network_config(&param).unwrap();
    }

    #[test]
    fn test_sp1_rejects_reserved_network_with_auction_strategy() {
        let param: Sp1Param = serde_json::from_value(json!({
            "recursion": "plonk",
            "prover": "network",
            "verify": true,
            "network_mode": "reserved",
            "fulfillment_strategy": "auction"
        }))
        .unwrap();

        assert!(validate_sp1_network_config(&param).is_err());
    }

    #[ignore = "elf needs input, ignore for now"]
    #[test]
    fn run_unittest_elf() {
        // TODO(Cecilia): imple GuestInput::mock() for unit test
        let client = ProverClient::builder().cpu().build();
        let stdin = SP1Stdin::new();
        let pk = client.setup(Elf::Static(TEST_ELF)).unwrap();
        let vk = pk.verifying_key();
        let proof = client.prove(&pk, stdin).run().unwrap();
        client
            .verify(&proof, vk, None)
            .expect("Sp1: verification failed");
    }

    #[ignore = "This is for docker image build only"]
    #[test]
    fn test_show_sp1_elf_vk() {
        let client = ProverClient::builder().cpu().build();
        let pk = client.setup(Elf::Static(BATCH_ELF)).unwrap();
        println!("SP1 ELF VK: {:?}", pk.verifying_key().bytes32());
    }
}
