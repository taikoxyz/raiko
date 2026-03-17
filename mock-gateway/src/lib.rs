pub mod generated;
pub mod router;
pub mod state;

pub use router::app;
pub use state::{AppState, MockContext};

use anyhow::{bail, Context};
use serde_json::{json, Value};

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

pub fn mock_proof_response(body: &Value, label: &str) -> Value {
    json!({
        "status": "ok",
        "proof_type": proof_type(body),
        "batch_id": proposal_batch_id(body),
        "data": {
            "proof": {
                "proof": format!("mock-proof:{label}"),
                "input": null,
                "quote": null,
                "uuid": null,
                "kzg_proof": null,
                "extra_data": null
            }
        }
    })
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
