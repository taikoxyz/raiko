use raiko_core::interfaces::ProofRequestOpt;
use raiko_host::server::api::{v1, v2};
use raiko_tasks::{ProofTaskDescriptor, TaskStatus};

const URL: &str = "http://localhost:8080";

pub struct ProofClient {
    reqwest_client: reqwest::Client,
}

impl ProofClient {
    pub fn new() -> Self {
        Self {
            reqwest_client: reqwest::Client::new(),
        }
    }

    pub async fn send_proof_v1(
        &self,
        proof_request: ProofRequestOpt,
    ) -> anyhow::Result<v1::Status> {
        let response = self
            .reqwest_client
            .post(&format!("{URL}/v1/proof"))
            .json(&proof_request)
            .send()
            .await?;

        if response.status().is_success() {
            let proof_response = response.json::<v1::Status>().await?;
            Ok(proof_response)
        } else {
            Err(anyhow::anyhow!("Failed to send proof request"))
        }
    }

    pub async fn send_proof_v2(
        &self,
        proof_request: ProofRequestOpt,
    ) -> anyhow::Result<v2::Status> {
        let response = self
            .reqwest_client
            .post(&format!("{URL}/v2/proof"))
            .json(&proof_request)
            .send()
            .await?;

        if response.status().is_success() {
            let proof_response = response.json::<v2::Status>().await?;
            Ok(proof_response)
        } else {
            Err(anyhow::anyhow!("Failed to send proof request"))
        }
    }

    pub async fn cancel_proof(
        &self,
        proof_request: ProofRequestOpt,
    ) -> anyhow::Result<v2::CancelStatus> {
        let response = self
            .reqwest_client
            .post(&format!("{URL}/v2/proof/cancel"))
            .json(&proof_request)
            .send()
            .await?;

        if response.status().is_success() {
            let cancel_response = response.json::<v2::CancelStatus>().await?;
            Ok(cancel_response)
        } else {
            Err(anyhow::anyhow!("Failed to send proof request"))
        }
    }

    pub async fn prune_proof(&self) -> anyhow::Result<v2::PruneStatus> {
        let response = self
            .reqwest_client
            .post(&format!("{URL}/v2/proof/prune"))
            .send()
            .await?;

        if response.status().is_success() {
            let prune_response = response.json::<v2::PruneStatus>().await?;
            Ok(prune_response)
        } else {
            Err(anyhow::anyhow!("Failed to send proof request"))
        }
    }

    pub async fn report_proof(&self) -> anyhow::Result<Vec<(ProofTaskDescriptor, TaskStatus)>> {
        let response = self
            .reqwest_client
            .get(&format!("{URL}/v2/proof/report"))
            .send()
            .await?;

        if response.status().is_success() {
            let report_response = response
                .json::<Vec<(ProofTaskDescriptor, TaskStatus)>>()
                .await?;
            Ok(report_response)
        } else {
            Err(anyhow::anyhow!("Failed to send proof request"))
        }
    }
}
