use crate::types::{
    AggregationGuestInput, AggregationGuestOutput, GuestBatchInput, GuestBatchOutput, 
    GuestInput, GuestOutput, IdStore, IdWrite, Proof, ProofKey, ProverError, ProverResult
};
use alloy_primitives::B256;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ZiskAgentResponse {
    pub proof: Option<String>,
    pub receipt: Option<String>,
    pub input: Option<[u8; 32]>, // B256 equivalent
    pub uuid: Option<String>,
}

impl From<ZiskAgentResponse> for Proof {
    fn from(value: ZiskAgentResponse) -> Self {
        Self {
            proof: value.proof,
            quote: value.receipt,
            input: value.input.map(B256::from),
            uuid: value.uuid,
            kzg_proof: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProofType {
    Batch,
    Aggregate,
}

#[derive(Debug, Serialize)]
pub struct AgentProofRequest {
    pub input: Vec<u8>,
    pub proof_type: ProofType,
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct AgentProofResponse {
    pub proof_data: Vec<u8>,
    pub proof_type: ProofType,
    pub success: bool,
    pub error: Option<String>,
}

pub struct ZiskAgentProver;

impl ZiskAgentProver {
    fn get_agent_url() -> String {
        std::env::var("ZISK_AGENT_URL")
            .unwrap_or_else(|_| "http://localhost:9998/proof".to_string())
    }

    async fn send_request(request: AgentProofRequest) -> ProverResult<ZiskAgentResponse> {
        let agent_url = Self::get_agent_url();
        let client = reqwest::Client::new();
        
        info!("Sending request to ZISK agent at {}: {:?} (input size: {})", 
              agent_url, request.proof_type, request.input.len());
        
        if request.input.is_empty() {
            return Err(ProverError::GuestError("Input data is empty".to_string()));
        }

        let response = client
            .post(&agent_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to send request to ZISK agent: {}", e)))?;

        if !response.status().is_success() {
            return Err(ProverError::GuestError(format!(
                "ZISK agent returned error status: {}",
                response.status()
            )));
        }

        let agent_response: AgentProofResponse = response
            .json()
            .await
            .map_err(|e| ProverError::GuestError(format!("Failed to parse agent response: {}", e)))?;

        if !agent_response.success {
            return Err(ProverError::GuestError(
                agent_response.error.unwrap_or_else(|| "Unknown agent error".to_string())
            ));
        }

        // Deserialize the proof data
        let zisk_response: ZiskAgentResponse = bincode::deserialize(&agent_response.proof_data)
            .map_err(|e| ProverError::GuestError(format!("Failed to deserialize agent response: {}", e)))?;

        info!("Received successful response from ZISK agent");
        Ok(zisk_response)
    }
}

// Implement methods directly on ZiskAgentProver (inherent methods)
impl ZiskAgentProver {
    pub async fn run(
        &self,
        _input: GuestInput,
        _output: &GuestOutput,
        _config: &Value,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        unimplemented!("no block run after pacaya fork")
    }

    pub async fn batch_run(
        &self,
        input: GuestBatchInput,
        _output: &GuestBatchOutput,
        _config: &Value,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        info!("ZISK Agent batch proof starting");

        // Serialize the GuestBatchInput for the agent service
        let serialized_input = bincode::serialize(&input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize GuestBatchInput: {e}")))?;

        let request = AgentProofRequest {
            input: serialized_input,
            proof_type: ProofType::Batch,
            config: None,
        };

        let agent_response = Self::send_request(request).await?;
        info!("ZISK Agent batch proof completed");

        Ok(agent_response.into())
    }

    pub async fn aggregate(
        &self,
        input: AggregationGuestInput,
        _output: &AggregationGuestOutput,
        _config: &Value,
        _id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        info!("ZISK Agent aggregation proof starting");

        // Serialize the AggregationGuestInput for the agent service
        let serialized_input = bincode::serialize(&input)
            .map_err(|e| ProverError::GuestError(format!("Failed to serialize AggregationGuestInput: {e}")))?;

        let request = AgentProofRequest {
            input: serialized_input,
            proof_type: ProofType::Aggregate,
            config: None,
        };

        let agent_response = Self::send_request(request).await?;
        info!("ZISK Agent aggregation proof completed");

        Ok(agent_response.into())
    }

    pub async fn cancel(&self, _proof_key: ProofKey, _id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        // ZISK agent doesn't support cancellation yet
        info!("ZISK Agent cancel requested - not implemented");
        Ok(())
    }
}