use serde_json::Value;

use crate::{
    make_response, BatchIdSourceSpec, MockResponseKind, MockResponseSpec, ProofPayloadSpec,
    StringSourceSpec, MockContext,
};

pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value {
    match ctx.call_index() {
        4 => make_response(
            body,
            &MockResponseSpec {
                kind: MockResponseKind::Error,
                error: Some("mock_error".to_string()),
                message: Some("forced failure on 4th request".to_string()),
                ..MockResponseSpec::default()
            },
        ),
        _ => make_response(
            body,
            &MockResponseSpec {
                kind: MockResponseKind::Proof,
                proof_type: StringSourceSpec::fixed("sp1"),
                batch_id: BatchIdSourceSpec::request(),
                proof_payload: Some(ProofPayloadSpec {
                    proof: Some("0x1234abcd".to_string()),
                    input: None,
                    quote: None,
                    uuid: None,
                    kzg_proof: None,
                    extra_data: None,
                }),
                ..MockResponseSpec::default()
            },
        ),
    }
}
