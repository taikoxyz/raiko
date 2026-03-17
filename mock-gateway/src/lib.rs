pub mod generated;
pub mod router;
pub mod state;

pub use router::app;
pub use state::{AppState, MockContext};

use anyhow::{bail, Context};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MockResponseKind {
    #[default]
    Status,
    Error,
    Proof,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ValueSourceKind {
    #[default]
    Request,
    Fixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StringSourceSpec {
    pub source: ValueSourceKind,
    pub value: Option<String>,
}

impl StringSourceSpec {
    pub fn request() -> Self {
        Self::default()
    }

    pub fn fixed(value: impl Into<String>) -> Self {
        Self {
            source: ValueSourceKind::Fixed,
            value: Some(value.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BatchIdSourceKind {
    #[default]
    Request,
    Fixed,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchIdSourceSpec {
    pub source: BatchIdSourceKind,
    pub value: Option<u64>,
}

impl BatchIdSourceSpec {
    pub fn request() -> Self {
        Self::default()
    }

    pub fn fixed(value: u64) -> Self {
        Self {
            source: BatchIdSourceKind::Fixed,
            value: Some(value),
        }
    }

    pub fn none() -> Self {
        Self {
            source: BatchIdSourceKind::None,
            value: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProofPayloadSpec {
    pub proof: Option<String>,
    pub input: Option<Value>,
    pub quote: Option<Value>,
    pub uuid: Option<String>,
    pub kzg_proof: Option<String>,
    pub extra_data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MockResponseSpec {
    pub kind: MockResponseKind,
    pub proof_type: StringSourceSpec,
    pub batch_id: BatchIdSourceSpec,
    pub task_status: Option<String>,
    pub error: Option<String>,
    pub message: Option<String>,
    pub proof_payload: Option<ProofPayloadSpec>,
}

pub fn proposal_batch_id(body: &Value) -> Option<u64> {
    body.get("proposals")
        .and_then(Value::as_array)
        .and_then(|proposals| proposals.first())
        .and_then(|proposal| proposal.get("proposal_id"))
        .and_then(Value::as_u64)
}

pub fn proof_type(body: &Value) -> &str {
    body.get("proof_type")
        .and_then(Value::as_str)
        .unwrap_or("native")
}

pub fn ok_status(proof_type: &str, batch_id: Option<u64>, task_status: &str) -> Value {
    json!({
        "status": "ok",
        "proof_type": proof_type,
        "batch_id": batch_id,
        "data": {
            "status": task_status
        }
    })
}

pub fn error_status(error: &str, message: &str) -> Value {
    json!({
        "status": "error",
        "error": error,
        "message": message
    })
}

pub fn make_response(body: &Value, spec: &MockResponseSpec) -> Value {
    match spec.kind {
        MockResponseKind::Status => ok_status(
            &resolve_proof_type(body, &spec.proof_type),
            resolve_batch_id(body, &spec.batch_id),
            spec.task_status.as_deref().unwrap_or("registered"),
        ),
        MockResponseKind::Error => error_status(
            spec.error.as_deref().unwrap_or("mock_error"),
            spec.message.as_deref().unwrap_or("generated mock error"),
        ),
        MockResponseKind::Proof => json!({
            "status": "ok",
            "proof_type": resolve_proof_type(body, &spec.proof_type),
            "batch_id": resolve_batch_id(body, &spec.batch_id),
            "data": {
                "proof": {
                    "proof": spec.proof_payload.as_ref().and_then(|payload| payload.proof.clone()),
                    "input": spec.proof_payload.as_ref().and_then(|payload| payload.input.clone()),
                    "quote": spec.proof_payload.as_ref().and_then(|payload| payload.quote.clone()),
                    "uuid": spec.proof_payload.as_ref().and_then(|payload| payload.uuid.clone()),
                    "kzg_proof": spec.proof_payload.as_ref().and_then(|payload| payload.kzg_proof.clone()),
                    "extra_data": spec.proof_payload.as_ref().and_then(|payload| payload.extra_data.clone()),
                }
            }
        }),
    }
}

pub fn mock_proof_response<S>(body: &Value, label: S) -> Value
where
    S: AsRef<str>,
{
    mock_proof_response_with_type(body, label, None)
}

pub fn mock_proof_response_with_type<S>(
    body: &Value,
    label: S,
    proof_type_override: Option<&str>,
) -> Value
where
    S: AsRef<str>,
{
    let proof = format!(
        "0x{}",
        label
            .as_ref()
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    validate_hex_proof(&proof).expect("mock proof helper must emit a valid hex string");
    make_response(
        body,
        &MockResponseSpec {
            kind: MockResponseKind::Proof,
            proof_type: match proof_type_override {
                Some(value) => StringSourceSpec::fixed(value),
                None => StringSourceSpec::request(),
            },
            batch_id: BatchIdSourceSpec::request(),
            proof_payload: Some(ProofPayloadSpec {
                proof: Some(proof),
                input: None,
                quote: None,
                uuid: None,
                kzg_proof: None,
                extra_data: None,
            }),
            ..MockResponseSpec::default()
        },
    )
}

fn validate_hex_proof(value: &str) -> anyhow::Result<()> {
    let hex = value
        .strip_prefix("0x")
        .ok_or_else(|| anyhow::anyhow!("proof must start with 0x"))?;
    if hex.is_empty() || hex.len() % 2 != 0 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        bail!("proof must be a valid hex string");
    }
    Ok(())
}

fn resolve_proof_type(body: &Value, spec: &StringSourceSpec) -> String {
    match spec.source {
        ValueSourceKind::Request => proof_type(body).to_string(),
        ValueSourceKind::Fixed => spec
            .value
            .clone()
            .unwrap_or_else(|| proof_type(body).to_string()),
    }
}

fn resolve_batch_id(body: &Value, spec: &BatchIdSourceSpec) -> Option<u64> {
    match spec.source {
        BatchIdSourceKind::Request => proposal_batch_id(body),
        BatchIdSourceKind::Fixed => spec.value,
        BatchIdSourceKind::None => None,
    }
}

pub fn gateway_bind_from_args<I, S>(args: I) -> anyhow::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    let _ = args.next();

    let mut bind = None;
    while let Some(arg) = args.next() {
        let arg = arg.as_ref();
        match arg {
            "--bind" => {
                let value = args
                    .next()
                    .context("missing value for --bind")?
                    .as_ref()
                    .to_string();
                bind = Some(value);
            }
            value if !value.starts_with('-') && bind.is_none() => {
                bind = Some(value.to_string());
            }
            unexpected => bail!("unknown argument: {unexpected}"),
        }
    }

    Ok(bind.unwrap_or_else(|| "0.0.0.0:4000".to_string()))
}

#[cfg(test)]
mod tests {
    use super::gateway_bind_from_args;

    #[test]
    fn gateway_bind_defaults_to_public_address() {
        let bind = gateway_bind_from_args(["raiko-mock-gateway"]).unwrap();
        assert_eq!(bind, "0.0.0.0:4000");
    }

    #[test]
    fn gateway_bind_uses_explicit_bind_flag() {
        let bind = gateway_bind_from_args(["raiko-mock-gateway", "--bind", "0.0.0.0:4123"])
            .unwrap();
        assert_eq!(bind, "0.0.0.0:4123");
    }
}
