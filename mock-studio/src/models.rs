use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct CreateTicketRequest {
    pub requirement: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockSpec {
    pub summary: String,
    pub default_task_status: String,
    #[serde(default)]
    pub memory_required: bool,
    #[serde(default)]
    pub request_key_fields: Vec<String>,
    #[serde(default)]
    pub normal_request_policy: Option<String>,
    #[serde(default)]
    pub aggregation_policy: Option<String>,
    #[serde(default)]
    pub proof_response_policy: Option<String>,
    pub nth_responses: Vec<NthResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NthResponse {
    pub n: u64,
    pub kind: String,
    pub error: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedIndexEntry {
    pub rule_id: String,
    pub summary: String,
    pub status: String,
    #[serde(default)]
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecGeneration {
    pub spec: MockSpec,
    pub prompt: String,
    pub response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandlerGeneration {
    pub source: String,
    pub prompt: String,
    pub response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReceipt {
    pub status: String,
    pub base_url: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketStatus {
    Pending,
    Running,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketRecord {
    pub ticket_id: String,
    pub rule_id: String,
    pub requirement: String,
    pub summary: String,
    pub status: TicketStatus,
    pub base_url: Option<String>,
    pub error: Option<String>,
}

impl TicketRecord {
    pub fn pending(ticket_id: String, requirement: String) -> Self {
        Self {
            rule_id: ticket_id.clone(),
            ticket_id,
            requirement,
            summary: "pending".to_string(),
            status: TicketStatus::Pending,
            base_url: None,
            error: None,
        }
    }

    pub fn failed_lookup(ticket_id: &str) -> Self {
        Self {
            ticket_id: ticket_id.to_string(),
            rule_id: ticket_id.to_string(),
            requirement: String::new(),
            summary: "ticket_not_found".to_string(),
            status: TicketStatus::Failed,
            base_url: None,
            error: Some("ticket not found".to_string()),
        }
    }
}
