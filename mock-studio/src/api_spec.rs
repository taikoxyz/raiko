use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ShastaApiSpec {
    pub route: String,
    pub request_type: String,
    pub key_request_fields: Vec<String>,
    pub response_contract: Vec<String>,
    pub aggregation_notes: Vec<String>,
    pub memory_contract: Vec<String>,
    pub helper_contract: Vec<String>,
    pub reference_snippets: Vec<String>,
}

pub fn shasta_api_spec() -> ShastaApiSpec {
    ShastaApiSpec {
        route: "/v3/proof/batch/shasta".to_string(),
        request_type: "JSON body compatible with ShastaProofRequest".to_string(),
        key_request_fields: vec![
            "aggregate: boolean; true means aggregation-style request".to_string(),
            "proof_type: string; proof helpers preserve it unless proof_type_override is set"
                .to_string(),
            "proposals[].proposal_id: stable request identity component".to_string(),
            "proposals[].l1_inclusion_block_number: useful for identity and behavior reasoning"
                .to_string(),
        ],
        response_contract: vec![
            "Use error_status(error, message) for error responses".to_string(),
            "Use ok_status(proof_type(body), proposal_batch_id(body), task_status) for registered/work-in-progress style responses".to_string(),
            "Use mock_proof_response(body, label) for proof-shaped success responses that preserve request proof_type".to_string(),
            "Use mock_proof_response_with_type(body, label, Some(\"fixed-type\")) only when proof_type_override is set".to_string(),
            "Do not change the outer JSON envelope shape".to_string(),
        ],
        aggregation_notes: vec![
            "aggregate=true should be treated as an aggregation request path".to_string(),
            "aggregation behavior can differ from normal requests and may directly return error in mocks".to_string(),
        ],
        memory_contract: vec![
            "Memory Contract".to_string(),
            "ctx.has_seen_request(body) -> bool".to_string(),
            "ctx.mark_request_seen(body) marks the request key as seen".to_string(),
            "ctx.request_key(body) returns a stable per-request key".to_string(),
            "ctx.call_index() returns the global call count for the running gateway".to_string(),
        ],
        helper_contract: vec![
            "Exact signature: pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value".to_string(),
            "Required imports: use serde_json::Value; and use crate::{error_status, mock_proof_response, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext};".to_string(),
            "Allowed context methods: call_index, request_key, has_seen_request, mark_request_seen".to_string(),
        ],
        reference_snippets: vec![
            "Real Shasta handler distinguishes aggregate requests and normal requests before returning Status/ProofResponse".to_string(),
            "A normal request may first return registered and later return a proof-shaped success response".to_string(),
            "Example stateful pattern: if aggregate is true return error_status(...); else if ctx.has_seen_request(body) return mock_proof_response_with_type(body, label, Some(\"risc0\")); else ctx.mark_request_seen(body) and return ok_status(..., \"registered\")".to_string(),
        ],
    }
}
