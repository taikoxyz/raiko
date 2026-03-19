use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct FieldSchema {
    pub path: String,
    pub type_name: String,
    pub required: bool,
    pub description: String,
    pub semantics: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShastaApiSpec {
    pub route: String,
    pub request_type: String,
    pub key_request_fields: Vec<String>,
    pub request_field_schemas: Vec<FieldSchema>,
    pub response_field_schemas: Vec<FieldSchema>,
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
        request_field_schemas: vec![
            FieldSchema {
                path: "aggregate".to_string(),
                type_name: "bool".to_string(),
                required: false,
                description: "Controls whether the request should be treated as an aggregate request path.".to_string(),
                semantics: vec![
                    "false or absent means normal proof request flow".to_string(),
                    "true means aggregate request flow and may have separate behavior only when user intent says so".to_string(),
                ],
                allowed_values: vec!["true".to_string(), "false".to_string()],
            },
            FieldSchema {
                path: "proof_type".to_string(),
                type_name: "string".to_string(),
                required: false,
                description: "Requested proof system identifier from the caller.".to_string(),
                semantics: vec![
                    "helpers preserve the incoming proof_type unless proof_type_override is set".to_string(),
                    "proof_type_override changes the top-level response proof_type value without changing the request body".to_string(),
                ],
                allowed_values: vec![
                    "native".to_string(),
                    "sp1".to_string(),
                    "risc0".to_string(),
                    "zisk".to_string(),
                ],
            },
            FieldSchema {
                path: "proposals[].proposal_id".to_string(),
                type_name: "u64".to_string(),
                required: true,
                description: "Primary stable identifier for the first proposal in the request.".to_string(),
                semantics: vec![
                    "proposal_batch_id(body) reads the first proposals[].proposal_id as batch_id".to_string(),
                    "proposal_id is a strong candidate for request identity and per-request memory".to_string(),
                ],
                allowed_values: Vec::new(),
            },
            FieldSchema {
                path: "proposals[].l1_inclusion_block_number".to_string(),
                type_name: "u64".to_string(),
                required: false,
                description: "Additional stable proposal field that can help distinguish logically different requests.".to_string(),
                semantics: vec![
                    "useful for request_key_fields when proposal_id alone is not enough".to_string(),
                ],
                allowed_values: Vec::new(),
            },
        ],
        response_field_schemas: vec![
            FieldSchema {
                path: "status".to_string(),
                type_name: "string".to_string(),
                required: true,
                description: "Top-level response mode.".to_string(),
                semantics: vec![
                    "successful status/proof responses use status=ok".to_string(),
                    "error responses use status=error".to_string(),
                ],
                allowed_values: vec!["ok".to_string(), "error".to_string()],
            },
            FieldSchema {
                path: "proof_type".to_string(),
                type_name: "string".to_string(),
                required: false,
                description: "Top-level proof type returned by ok/proof-shaped responses.".to_string(),
                semantics: vec![
                    "defaults to proof_type(body) unless proof_type_override is set".to_string(),
                ],
                allowed_values: vec![
                    "native".to_string(),
                    "sp1".to_string(),
                    "risc0".to_string(),
                    "zisk".to_string(),
                ],
            },
            FieldSchema {
                path: "batch_id".to_string(),
                type_name: "u64|null".to_string(),
                required: false,
                description: "Resolved from proposal_batch_id(body) for supported helpers.".to_string(),
                semantics: vec![
                    "proof/status helpers preserve the existing batch_id contract".to_string(),
                ],
                allowed_values: Vec::new(),
            },
        ],
        response_contract: vec![
            "Use error_status(error, message) for error responses".to_string(),
            "Use ok_status(proof_type(body), proposal_batch_id(body), task_status) for registered/work-in-progress style responses".to_string(),
            "Use mock_proof_response(body, label) for proof-shaped success responses that preserve request proof_type".to_string(),
            "Use mock_proof_response_with_type(body, label, Some(\"<exact proof_type_override>\")) only when proof_type_override is set".to_string(),
            "Do not change the outer JSON envelope shape".to_string(),
        ],
        aggregation_notes: vec![
            "aggregate=true should be treated as an aggregation request path".to_string(),
            "aggregate requests may have distinct behavior only when explicitly required by user intent".to_string(),
        ],
        memory_contract: vec![
            "Memory Contract".to_string(),
            "ctx.has_seen_request(body) -> bool".to_string(),
            "ctx.mark_request_seen(body) marks the request key as seen".to_string(),
            "ctx.request_key(body) returns a stable per-request key".to_string(),
            "ctx.request_key(body), ctx.has_seen_request(body), and ctx.mark_request_seen(body) are the preferred primitives for repeated-request behavior".to_string(),
            "ctx.call_index() is global process-wide state and must not be used for per-request nth behavior".to_string(),
        ],
        helper_contract: vec![
            "Exact signature: pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value".to_string(),
            "Required imports: use serde_json::Value; and use crate::{error_status, mock_proof_response, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext};".to_string(),
            "Allowed context methods: request_key, has_seen_request, mark_request_seen, call_index".to_string(),
            "Prefer request_key/has_seen_request/mark_request_seen for repeated-request behavior; do not use call_index for per-request nth behavior".to_string(),
        ],
        reference_snippets: vec![
            "Real Shasta handler distinguishes aggregate requests and normal requests before returning Status/ProofResponse".to_string(),
            "A normal request may first return registered and later return a proof-shaped success response".to_string(),
            "Example repeated-request pattern: derive a request_key from the body, use ctx.has_seen_request(body) to detect repeats, use ctx.mark_request_seen(body) to store first-seen state, and return the requested proof or error behavior for that logical request".to_string(),
        ],
    }
}
