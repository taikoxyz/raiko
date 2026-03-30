#![cfg(feature = "enable")]

use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    str::{self},
};

use once_cell::sync::Lazy;
use raiko_lib::{
    consts::TaikoSpecId,
    input::{
        AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput,
        GuestInput, GuestOutput, ShastaAggregationGuestInput,
    },
    primitives::B256,
    proof_type::ProofType,
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use serde_with::serde_as;

pub mod local_prover;
use local_prover::LocalSgxProver;
mod remote_prover;
use remote_prover::RemoteSgxProver;
use tracing::debug;
// to register the instance id
mod sgx_register_utils;

fn read_bootstrap_quote(bootstrap_file_name: String) -> Result<Vec<u8>, String> {
    // Get home directory and construct path to bootstrap.json
    let home_dir =
        std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;

    let bootstrap_path = PathBuf::from(home_dir)
        .join(".config")
        .join("raiko")
        .join("config")
        .join(&bootstrap_file_name);

    // Read and parse bootstrap.json
    let bootstrap_content = fs::read_to_string(&bootstrap_path).map_err(|e| {
        format!(
            "Failed to read bootstrap.json from {}: {}",
            bootstrap_path.display(),
            e
        )
    })?;

    let bootstrap_data: serde_json::Value = serde_json::from_str(&bootstrap_content)
        .map_err(|e| format!("Failed to parse bootstrap.json: {}", e))?;

    // Extract quote field
    let quote_hex = bootstrap_data["quote"].as_str().ok_or_else(|| {
        format!(
            "Missing or invalid 'quote' field in {}",
            bootstrap_file_name
        )
    })?;

    // Decode hex string to bytes (handle both 0x prefixed and non-prefixed)
    let quote_hex_clean = if quote_hex.starts_with("0x") || quote_hex.starts_with("0X") {
        &quote_hex[2..]
    } else {
        quote_hex
    };
    let quote = hex::decode(quote_hex_clean)
        .map_err(|e| format!("Failed to decode quote hex string: {}", e))?;

    if quote.len() < 432 {
        return Err("SGX quote too short".to_string());
    }

    Ok(quote)
}

static SGX_RETH_GUEST_DATA: Lazy<Result<Value, String>> = Lazy::new(|| {
    let quote = read_bootstrap_quote("bootstrap.json".to_string())?;
    // Extract MR_ENCLAVE (32 bytes at offset 112-144)
    let mr_enclave = hex::encode(&quote[112..144]);

    // Extract MR_SIGNER (32 bytes at offset 176-208)
    let mr_signer = hex::encode(&quote[176..208]);

    let quote_hex = hex::encode(&quote);

    Ok(json!({
            "mr_enclave": mr_enclave,
            "mr_signer": mr_signer,
            "quote": quote_hex
    }))
});

static SGX_GETH_GUEST_DATA: Lazy<Result<Value, String>> = Lazy::new(|| {
    let quote = read_bootstrap_quote("bootstrap.gaiko.json".to_string())?;
    // Extract MR_ENCLAVE (32 bytes at offset 112-144)
    let mr_enclave = hex::encode(&quote[112..144]);

    // Extract MR_SIGNER (32 bytes at offset 176-208)
    let mr_signer = hex::encode(&quote[176..208]);

    let quote_hex = hex::encode(&quote);

    Ok(json!({
            "mr_enclave": mr_enclave,
            "mr_signer": mr_signer,
            "quote": quote_hex
    }))
});

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SgxParam {
    pub instance_ids: HashMap<TaikoSpecId, u64>,
    pub setup: bool,
    pub bootstrap: bool,
    pub prove: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SgxResponse {
    /// proof format: 4b(id)+20b(pubkey)+65b(signature)
    pub proof: String,
    pub quote: String,
    pub input: B256,
}

impl From<SgxResponse> for Proof {
    fn from(value: SgxResponse) -> Self {
        Self {
            proof: Some(value.proof),
            input: Some(value.input),
            quote: Some(value.quote),
            uuid: None,
            kzg_proof: None,
            extra_data: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
enum SgxProverType {
    /// Local SGX prover
    /// This is the default prover.
    #[default]
    Local,
    /// Remote SGX prover
    Remote,
}

impl std::str::FromStr for SgxProverType {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "local" => Ok(SgxProverType::Local),
            "remote" => Ok(SgxProverType::Remote),
            _ => unimplemented!("unknown sgx mode"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SgxProver {
    /// Local SGX prover
    /// This is the default prover.
    Local(LocalSgxProver),
    /// Remote SGX prover
    Remote(RemoteSgxProver),
}

impl SgxProver {
    pub fn new(prove_type: ProofType) -> Self {
        let service_type = &std::env::var("SGX_MODE")
            .unwrap_or_else(|_| "local".to_string())
            .parse::<SgxProverType>()
            .unwrap_or_default();
        debug!("sgx mode: {:?}, prove_type: {}", service_type, prove_type);
        let prover = match service_type {
            SgxProverType::Local => SgxProver::Local(local_prover::LocalSgxProver::new(prove_type)),
            SgxProverType::Remote => {
                SgxProver::Remote(remote_prover::RemoteSgxProver::new(prove_type))
            }
        };
        prover
    }
}

impl Prover for SgxProver {
    async fn get_guest_data() -> ProverResult<serde_json::Value> {
        let sgx_reth_guest_data = SGX_RETH_GUEST_DATA
            .as_ref()
            .map_err(|e| ProverError::GuestError(e.clone()))?;

        let mut json = json!({
            "sgx_reth": sgx_reth_guest_data,
        });

        if std::env::var("SGXGETH").unwrap_or_default() == "true" {
            let sgx_geth_guest_data = SGX_GETH_GUEST_DATA
                .as_ref()
                .map_err(|e| ProverError::GuestError(e.clone()))?;

            json.as_object_mut()
                .unwrap()
                .insert("sgx_geth".to_string(), sgx_geth_guest_data.clone());
        }

        Ok(json)
    }
    async fn run(
        &self,
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        match self {
            SgxProver::Local(prover) => prover.run(input, output, config, store).await,
            SgxProver::Remote(prover) => prover.run(input, output, config, store).await,
        }
    }
    async fn batch_run(
        &self,
        input: GuestBatchInput,
        output: &GuestBatchOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        match self {
            SgxProver::Local(prover) => prover.batch_run(input, output, config, store).await,
            SgxProver::Remote(prover) => prover.batch_run(input, output, config, store).await,
        }
    }

    async fn aggregate(
        &self,
        input: AggregationGuestInput,
        output: &AggregationGuestOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        match self {
            SgxProver::Local(prover) => prover.aggregate(input, output, config, store).await,
            SgxProver::Remote(prover) => prover.aggregate(input, output, config, store).await,
        }
    }

    async fn shasta_aggregate(
        &self,
        input: ShastaAggregationGuestInput,
        output: &AggregationGuestOutput,
        config: &ProverConfig,
        store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        match self {
            SgxProver::Local(prover) => prover.shasta_aggregate(input, output, config, store).await,
            SgxProver::Remote(prover) => {
                prover.shasta_aggregate(input, output, config, store).await
            }
        }
    }

    async fn cancel(&self, proof_key: ProofKey, read: Box<&mut dyn IdStore>) -> ProverResult<()> {
        match self {
            SgxProver::Local(prover) => prover.cancel(proof_key, read).await,
            SgxProver::Remote(prover) => prover.cancel(proof_key, read).await,
        }
    }

    fn proof_type(&self) -> ProofType {
        match self {
            SgxProver::Local(prover) => prover.proof_type(),
            SgxProver::Remote(prover) => prover.proof_type(),
        }
    }
}
