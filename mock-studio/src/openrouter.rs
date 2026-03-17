use std::env;

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use reqwest::StatusCode;
use serde_json::json;
use syn::parse_file;

use crate::{
    api_spec::{shasta_api_spec, ShastaApiSpec},
    models::{HandlerGeneration, MockSpec, NthResponse, SpecGeneration},
};

#[async_trait]
pub trait MockPlanner: Send + Sync {
    async fn plan(&self, requirement: &str) -> anyhow::Result<SpecGeneration>;
}

#[async_trait]
pub trait RestrictedHandlerGenerator: Send + Sync {
    async fn generate_handler(
        &self,
        requirement: &str,
        spec: &MockSpec,
    ) -> anyhow::Result<HandlerGeneration>;
}

#[derive(Clone)]
pub struct FakePlanner {
    spec: MockSpec,
}

impl FakePlanner {
    pub fn success() -> Self {
        Self {
            spec: MockSpec {
                summary: "Return an error on the fourth call".to_string(),
                default_task_status: "registered".to_string(),
                memory_required: true,
                request_key_fields: vec!["proposals[].proposal_id".to_string()],
                normal_request_policy: Some("first return registered".to_string()),
                aggregation_policy: Some("aggregation always errors".to_string()),
                proof_response_policy: Some(
                    "same normal request later returns a proof-shaped success response"
                        .to_string(),
                ),
                proof_type_override: Some("risc0".to_string()),
                nth_responses: vec![NthResponse {
                    n: 4,
                    kind: "error".to_string(),
                    error: Some("mock_error".to_string()),
                    message: Some("forced failure on 4th request".to_string()),
                }],
            },
        }
    }
}

#[async_trait]
impl MockPlanner for FakePlanner {
    async fn plan(&self, requirement: &str) -> anyhow::Result<SpecGeneration> {
        Ok(SpecGeneration {
            spec: self.spec.clone(),
            prompt: format!("fake planner prompt for requirement: {requirement}"),
            response: serde_json::to_string_pretty(&self.spec)?,
        })
    }
}

#[derive(Clone, Default)]
pub struct FakeHandlerGenerator;

impl FakeHandlerGenerator {
    pub fn success() -> Self {
        Self
    }
}

#[async_trait]
impl RestrictedHandlerGenerator for FakeHandlerGenerator {
    async fn generate_handler(
        &self,
        requirement: &str,
        spec: &MockSpec,
    ) -> anyhow::Result<HandlerGeneration> {
        let source = render_handler_from_spec(spec);
        Ok(HandlerGeneration {
            source: source.clone(),
            prompt: format!(
                "fake handler prompt for requirement: {requirement}, spec: {}",
                serde_json::to_string(spec)?
            ),
            response: serde_json::to_string_pretty(&serde_json::json!({ "source": source }))?,
        })
    }
}

#[derive(Clone)]
pub struct OpenRouterAgent {
    api_key: String,
    spec_model: String,
    handler_model: String,
    client: reqwest::Client,
}

impl OpenRouterAgent {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            api_key: env::var("OPENROUTER_API_KEY")
                .context("OPENROUTER_API_KEY is required for mock studio")?,
            spec_model: env::var("OPENROUTER_SPEC_MODEL")
                .or_else(|_| env::var("OPENROUTER_MODEL"))
                .unwrap_or_else(|_| "openai/gpt-4o-mini".to_string()),
            handler_model: env::var("OPENROUTER_HANDLER_MODEL")
                .or_else(|_| env::var("OPENROUTER_MODEL"))
                .unwrap_or_else(|_| "openai/gpt-4o-mini".to_string()),
            client: reqwest::Client::new(),
        })
    }

    async fn call_json(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> anyhow::Result<String> {
        let response = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": model,
                "response_format": { "type": "json_object" },
                "messages": [
                    {
                        "role": "system",
                        "content": system_prompt
                    },
                    {
                        "role": "user",
                        "content": user_prompt
                    }
                ]
            }))
            .send()
            .await
            .context("openrouter request failed")?;

        let status = response.status();
        let body = response.text().await.context("invalid openrouter body")?;
        extract_openrouter_message(status, &body)
    }
}

#[async_trait]
impl MockPlanner for OpenRouterAgent {
    async fn plan(&self, requirement: &str) -> anyhow::Result<SpecGeneration> {
        let prompt = build_planner_prompt(requirement, &shasta_api_spec())?;
        let content = self
            .call_json(
                &self.spec_model,
                "You are planning a Rust mock gateway. Return only JSON with keys summary, default_task_status, memory_required, request_key_fields, normal_request_policy, aggregation_policy, proof_response_policy, proof_type_override, nth_responses. proof_type_override must be absent or a simple string, and it only affects proof-shaped success responses. Keep the result constrained to /v3/proof/batch/shasta behavior.",
                &prompt,
            )
            .await?;

        Ok(SpecGeneration {
            spec: parse_mock_spec(&content).context("failed to parse mock spec")?,
            prompt,
            response: content,
        })
    }
}

#[async_trait]
impl RestrictedHandlerGenerator for OpenRouterAgent {
    async fn generate_handler(
        &self,
        requirement: &str,
        spec: &MockSpec,
    ) -> anyhow::Result<HandlerGeneration> {
        let prompt = build_handler_prompt(requirement, spec, &shasta_api_spec())?;
        let content = self
            .call_json(
                &self.handler_model,
                "You generate Rust code for a single restricted handler module. Return JSON only with key source. The Rust source must define exactly one public function named handle_shasta_request(ctx: &MockContext, body: &Value) -> Value. It must import `use serde_json::Value;`. It must import `use crate::{error_status, mock_proof_response, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext};`. It may call ctx.call_index(), ctx.request_key(body), ctx.has_seen_request(body), and ctx.mark_request_seen(body). Do not define routers, modules, helpers, mains, or tests. error_status always takes exactly two string arguments. ok_status always takes proof_type(body), proposal_batch_id(body), and a task status string. Use mock_proof_response_with_type(body, label, Some(\"fixed-type\")) only when Task Spec includes proof_type_override. Otherwise use mock_proof_response(body, label).",
                &prompt,
            )
            .await?;

        let payload: serde_json::Value =
            serde_json::from_str(&content).context("failed to parse handler generation json")?;
        let source = payload["source"]
            .as_str()
            .ok_or_else(|| anyhow!("handler generation missing source"))?;
        let source = sanitize_handler_source(source, spec);
        Ok(HandlerGeneration {
            source,
            prompt,
            response: content,
        })
    }
}

pub fn render_handler_from_spec(spec: &MockSpec) -> String {
    let mut nth_match_arms = String::new();
    for response in &spec.nth_responses {
        if response.kind == "error" {
            let error = response.error.as_deref().unwrap_or("mock_error");
            let message = response
                .message
                .as_deref()
                .unwrap_or("generated mock error");
            nth_match_arms.push_str(&format!(
                "        {} => return error_status({error:?}, {message:?}),\n",
                response.n
            ));
        }
    }

    let aggregate_branch = if spec.aggregation_policy.is_some() {
        "    if body.get(\"aggregate\").and_then(Value::as_bool).unwrap_or(false) {\n        return error_status(\"mock_aggregation_error\", \"aggregation is not available in this mock\");\n    }\n\n"
    } else {
        ""
    };

    let seen_branch = if spec.memory_required && should_emit_proof_response(spec) {
        format!(
            "    if ctx.has_seen_request(body) {{\n        return {};\n    }}\n\n    ctx.mark_request_seen(body);\n\n",
            render_proof_response_call(spec, "repeat-request")
        )
    } else if spec.memory_required {
        "    ctx.mark_request_seen(body);\n\n".to_string()
    } else {
        String::new()
    };

    format!(
        "use serde_json::Value;\n\nuse crate::{{error_status, mock_proof_response, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext}};\n\npub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value {{\n    match ctx.call_index() {{\n{nth_match_arms}        _ => {{}}\n    }}\n\n{aggregate_branch}{seen_branch}    ok_status({}, proposal_batch_id(body), {:?})\n}}\n",
        render_default_proof_type_expression(spec),
        spec.default_task_status
    )
}

fn should_emit_proof_response(spec: &MockSpec) -> bool {
    spec.proof_response_policy.is_some() || spec.proof_type_override.is_some()
}

fn render_proof_response_call(spec: &MockSpec, label: &str) -> String {
    match spec.proof_type_override.as_deref() {
        Some(proof_type_override) => format!(
            "mock_proof_response_with_type(body, {label:?}, Some({proof_type_override:?}))"
        ),
        None => format!("mock_proof_response(body, {label:?})"),
    }
}

fn render_default_proof_type_expression(spec: &MockSpec) -> String {
    match spec.proof_type_override.as_deref() {
        Some(proof_type_override) => format!("{proof_type_override:?}"),
        None => "proof_type(body)".to_string(),
    }
}

fn validate_handler_source(source: &str) -> anyhow::Result<()> {
    if !source.contains("pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value") {
        anyhow::bail!(
            "generated handler must define the exact public signature for handle_shasta_request"
        );
    }

    for forbidden in ["fn main", "Router", "mod ", "#[tokio::main]"] {
        if source.contains(forbidden) {
            anyhow::bail!("generated handler contains forbidden token: {forbidden}");
        }
    }

    if !source.contains(
        "use crate::{error_status, mock_proof_response, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext};",
    ) {
        anyhow::bail!("generated handler must import the allowed helper set");
    }

    if !source.contains("use serde_json::Value;") {
        anyhow::bail!("generated handler must import serde_json::Value");
    }

    let has_valid_error = contains_call_with_min_commas(source, "error_status(", 1);
    let has_valid_ok = contains_call_with_min_commas(source, "ok_status(", 2);
    let has_valid_mock_proof = contains_call_with_min_commas(source, "mock_proof_response(", 1);
    let has_valid_mock_proof_override =
        contains_call_with_min_commas(source, "mock_proof_response_with_type(", 2);
    if !has_valid_error && !has_valid_ok && !has_valid_mock_proof && !has_valid_mock_proof_override
    {
        anyhow::bail!("generated handler must call at least one allowed helper");
    }

    if source.contains("ok_status(")
        && (!source.contains("proof_type(body)") || !source.contains("proposal_batch_id(body)"))
    {
        anyhow::bail!("generated handler must call ok_status with proof_type(body) and proposal_batch_id(body)");
    }

    validate_rust_syntax(source)?;

    Ok(())
}

fn sanitize_handler_source(source: &str, spec: &MockSpec) -> String {
    match validate_handler_source(source).and_then(|_| validate_handler_matches_spec(source, spec))
    {
        Ok(()) => source.to_string(),
        Err(_) => render_handler_from_spec(spec),
    }
}

fn validate_rust_syntax(source: &str) -> anyhow::Result<()> {
    parse_file(source).context("generated handler must be valid Rust syntax")?;
    Ok(())
}

fn validate_handler_matches_spec(source: &str, spec: &MockSpec) -> anyhow::Result<()> {
    if let Some(proof_type_override) = spec.proof_type_override.as_deref() {
        let expected_ok = format!("ok_status({proof_type_override:?}, proposal_batch_id(body),");
        let expected_proof = format!("Some({proof_type_override:?})");
        if !source.contains(&expected_ok) || !source.contains(&expected_proof) {
            anyhow::bail!("generated handler does not honor proof_type_override");
        }
    }

    Ok(())
}

fn build_planner_prompt(requirement: &str, api_spec: &ShastaApiSpec) -> anyhow::Result<String> {
    Ok(format!(
        "Requirement:\n{requirement}\n\nAPI Spec:\n{}\n\nReturn JSON only. Required keys: summary, default_task_status, memory_required, request_key_fields, normal_request_policy, aggregation_policy, proof_response_policy, proof_type_override, nth_responses. nth_responses is an array of objects with keys n, kind, error, message. If the mock should force proof_type, set top-level proof_type_override to that exact string instead of burying it inside nested policy objects. default_task_status must be a snake_case task status string.",
        serde_json::to_string_pretty(api_spec)?
    ))
}

fn parse_mock_spec(content: &str) -> anyhow::Result<MockSpec> {
    let payload: serde_json::Value =
        serde_json::from_str(content).context("invalid planner json")?;

    let summary = payload
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Generated mock spec")
        .to_string();
    let default_task_status = payload
        .get("default_task_status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("registered")
        .to_string();
    let memory_required = parse_memory_required(payload.get("memory_required"));
    let request_key_fields = parse_request_key_fields(payload.get("request_key_fields"));
    let normal_request_policy = optional_string(payload.get("normal_request_policy"));
    let aggregation_policy = optional_string(payload.get("aggregation_policy"));
    let proof_response_policy = optional_string(payload.get("proof_response_policy"));
    let proof_type_override = optional_string(payload.get("proof_type_override"))
        .or_else(|| payload.get("normal_request_policy").and_then(find_nested_proof_type))
        .or_else(|| payload.get("proof_response_policy").and_then(find_nested_proof_type));
    let nth_responses = parse_nth_responses(payload.get("nth_responses"))?;

    Ok(MockSpec {
        summary,
        default_task_status,
        memory_required,
        request_key_fields,
        normal_request_policy,
        aggregation_policy,
        proof_response_policy,
        proof_type_override,
        nth_responses,
    })
}

fn parse_memory_required(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::Bool(flag)) => *flag,
        Some(serde_json::Value::String(text)) => {
            let normalized = text.to_ascii_lowercase();
            normalized.contains("true")
                || normalized.contains("memory")
                || normalized.contains("remember")
                || normalized.contains("seen request")
        }
        _ => false,
    }
}

fn parse_request_key_fields(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        Some(serde_json::Value::String(text)) => text
            .split([',', '\n'])
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn optional_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value {
        Some(serde_json::Value::String(text)) if !text.trim().is_empty() => {
            Some(text.trim().to_string())
        }
        Some(other) if !other.is_null() => Some(other.to_string()),
        _ => None,
    }
}

fn parse_nth_responses(value: Option<&serde_json::Value>) -> anyhow::Result<Vec<NthResponse>> {
    let Some(serde_json::Value::Array(entries)) = value else {
        return Ok(Vec::new());
    };

    let mut parsed = Vec::new();
    for entry in entries {
        let Some(object) = entry.as_object() else {
            continue;
        };

        let Some(n) = object.get("n").and_then(parse_u64_loose) else {
            continue;
        };

        let kind = object
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("error")
            .to_string();
        let error = optional_string(object.get("error"));
        let message = optional_string(object.get("message"));

        parsed.push(NthResponse {
            n,
            kind,
            error,
            message,
        });
    }

    Ok(parsed)
}

fn parse_u64_loose(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn find_nested_proof_type(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(text)) = map.get("proof_type") {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            map.values().find_map(find_nested_proof_type)
        }
        serde_json::Value::Array(values) => values.iter().find_map(find_nested_proof_type),
        _ => None,
    }
}

fn build_handler_prompt(
    requirement: &str,
    spec: &MockSpec,
    api_spec: &ShastaApiSpec,
) -> anyhow::Result<String> {
    Ok(format!(
        "Task Requirement:\n{requirement}\n\nTask Spec:\n{}\n\nAPI Spec:\n{}\n\nMemory Contract:\n- ctx.call_index()\n- ctx.request_key(body)\n- ctx.has_seen_request(body)\n- ctx.mark_request_seen(body)\n\nGenerate Rust source for the restricted handler module. Return JSON only as {{\"source\": \"...\"}}.",
        serde_json::to_string_pretty(spec)?,
        serde_json::to_string_pretty(api_spec)?
    ))
}

fn extract_openrouter_message(status: StatusCode, body: &str) -> anyhow::Result<String> {
    let payload: serde_json::Value =
        serde_json::from_str(body).context("invalid openrouter json")?;

    if !status.is_success() {
        let message = payload
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(body);
        anyhow::bail!("openrouter returned {status}: {message}");
    }

    payload["choices"][0]["message"]["content"]
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("openrouter response missing message content: {body}"))
}

fn contains_call_with_min_commas(source: &str, function_name: &str, min_commas: usize) -> bool {
    let mut remainder = source;
    while let Some(start) = remainder.find(function_name) {
        let after_name = &remainder[start + function_name.len()..];
        if let Some(end) = after_name.find(')') {
            let args = &after_name[..end];
            if args.matches(',').count() >= min_commas {
                return true;
            }
            remainder = &after_name[end + 1..];
            continue;
        }
        break;
    }
    false
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;
    use serde_json::json;

    use crate::{
        api_spec::shasta_api_spec,
        models::{MockSpec, NthResponse},
    };

    use super::{
        build_handler_prompt, extract_openrouter_message, parse_mock_spec, render_handler_from_spec,
        sanitize_handler_source, validate_handler_source,
    };

    #[test]
    fn validate_handler_source_rejects_missing_allowed_helpers() {
        let source = r#"
use serde_json::Value;

pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value {
    match ctx.call_index() {
        4 => serde_json::json!({"status": "error"}),
        _ => body.clone(),
    }
}
"#;

        let error = validate_handler_source(source).unwrap_err().to_string();
        assert!(error.contains("allowed helper"));
    }

    #[test]
    fn validate_handler_source_rejects_missing_value_import() {
        let source = r#"
use crate::{error_status, mock_proof_response, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext};

pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value {
    match ctx.call_index() {
        4 => error_status("mock_error", "forced failure"),
        _ => ok_status(proof_type(body), proposal_batch_id(body), "registered"),
    }
}
"#;

        let error = validate_handler_source(source).unwrap_err().to_string();
        assert!(error.contains("serde_json::Value"));
    }

    #[test]
    fn validate_handler_source_rejects_invalid_rust_syntax() {
        let source = r#"
use serde_json::Value;
use crate::{error_status, mock_proof_response, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext};

pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value {
    if body["aggregate].as_bool().unwrap_or(false) {
        return error_status("mock_aggregation_error", "aggregation is not available in this mock");
    }
    ok_status(proof_type(body), proposal_batch_id(body), "registered")
}
"#;

        let error = validate_handler_source(source).unwrap_err().to_string();
        assert!(error.contains("valid Rust syntax"));
    }

    #[test]
    fn extract_openrouter_message_surfaces_http_error_body() {
        let body = r#"{"error":{"message":"No auth credentials found"}}"#;

        let error = extract_openrouter_message(StatusCode::UNAUTHORIZED, body)
            .unwrap_err()
            .to_string();

        assert!(error.contains("openrouter returned 401 Unauthorized"));
        assert!(error.contains("No auth credentials found"));
    }

    #[test]
    fn shasta_api_spec_mentions_aggregation_and_memory_contract() {
        let spec = shasta_api_spec();
        let rendered = serde_json::to_string(&spec).unwrap();

        assert!(rendered.contains("aggregate"));
        assert!(rendered.contains("has_seen_request"));
        assert!(rendered.contains("mark_request_seen"));
    }

    #[test]
    fn handler_prompt_includes_api_spec_context() {
        let task_spec = MockSpec {
            summary: "remember and return proof later".to_string(),
            default_task_status: "registered".to_string(),
            nth_responses: vec![NthResponse {
                n: 4,
                kind: "error".to_string(),
                error: Some("mock_error".to_string()),
                message: Some("forced failure".to_string()),
            }],
            memory_required: true,
            request_key_fields: vec![
                "aggregate".to_string(),
                "proposals[].proposal_id".to_string(),
            ],
            normal_request_policy: Some("first registered, then proof".to_string()),
            aggregation_policy: Some("aggregation always errors".to_string()),
            proof_response_policy: Some("return a mock proof envelope".to_string()),
            proof_type_override: Some("risc0".to_string()),
        };

        let prompt = build_handler_prompt(
            "normal request registers first, then returns proof; aggregation errors",
            &task_spec,
            &shasta_api_spec(),
        )
        .unwrap();

        assert!(prompt.contains("API Spec"));
        assert!(prompt.contains("Memory Contract"));
        assert!(prompt.contains("has_seen_request"));
        assert!(prompt.contains("aggregate"));
        assert!(prompt.contains("first registered, then proof"));
    }

    #[test]
    fn parse_mock_spec_normalizes_string_memory_required() {
        let content = json!({
            "summary": "stateful mock",
            "default_task_status": "registered",
            "memory_required": "This mock will maintain a memory of seen requests based on a unique key derived from the request body.",
            "request_key_fields": "aggregate, proposals[].proposal_id, proof_type",
            "normal_request_policy": "first registered, then proof",
            "aggregation_policy": "aggregation always errors",
            "proof_response_policy": "return a mock proof envelope",
            "nth_responses": []
        })
        .to_string();

        let spec = parse_mock_spec(&content).unwrap();

        assert!(spec.memory_required);
        assert_eq!(
            spec.request_key_fields,
            vec![
                "aggregate".to_string(),
                "proposals[].proposal_id".to_string(),
                "proof_type".to_string()
            ]
        );
    }

    #[test]
    fn parse_mock_spec_accepts_proof_type_override_string() {
        let content = json!({
            "summary": "stateful mock",
            "default_task_status": "registered",
            "proof_type_override": "risc0",
            "memory_required": true,
            "request_key_fields": ["proposals[].proposal_id"],
            "nth_responses": []
        })
        .to_string();

        let spec = parse_mock_spec(&content).unwrap();

        assert_eq!(spec.default_task_status, "registered");
        assert_eq!(spec.proof_type_override.as_deref(), Some("risc0"));
    }

    #[test]
    fn parse_mock_spec_derives_proof_type_override_from_nested_policy() {
        let content = json!({
            "summary": "stateful mock",
            "default_task_status": "registered",
            "normal_request_policy": {
                "on_first_request": {
                    "status": "ok",
                    "data": {
                        "status": "registered",
                        "proof_type": "sp1"
                    }
                },
                "on_second_request": {
                    "proof_type": "sp1"
                }
            },
            "proof_response_policy": {
                "success_response": "mock_proof_response keeps request proof_type"
            },
            "nth_responses": []
        })
        .to_string();

        let spec = parse_mock_spec(&content).unwrap();

        assert_eq!(spec.proof_type_override.as_deref(), Some("sp1"));
    }

    #[test]
    fn parse_mock_spec_ignores_invalid_nth_response_indices() {
        let content = json!({
            "summary": "aggregation-aware mock",
            "default_task_status": "registered",
            "memory_required": true,
            "nth_responses": [
                {
                    "n": "aggregate",
                    "kind": "error",
                    "error": "bad",
                    "message": "ignore me"
                },
                {
                    "n": 4,
                    "kind": "error",
                    "error": "mock_error",
                    "message": "forced failure"
                }
            ]
        })
        .to_string();

        let spec = parse_mock_spec(&content).unwrap();

        assert_eq!(spec.nth_responses.len(), 1);
        assert_eq!(spec.nth_responses[0].n, 4);
    }

    #[test]
    fn render_handler_from_spec_uses_proof_type_override_helper() {
        let spec = MockSpec {
            summary: "override proof type".to_string(),
            default_task_status: "registered".to_string(),
            memory_required: true,
            request_key_fields: vec!["proposals[].proposal_id".to_string()],
            normal_request_policy: Some("register then proof".to_string()),
            aggregation_policy: None,
            proof_response_policy: Some("return proof on repeat".to_string()),
            proof_type_override: Some("risc0".to_string()),
            nth_responses: Vec::new(),
        };

        let source = render_handler_from_spec(&spec);

        assert!(source.contains("mock_proof_response_with_type"));
        assert!(source.contains("Some(\"risc0\")"));
        assert!(source.contains("ok_status(\"risc0\""));
    }

    #[test]
    fn sanitize_handler_source_falls_back_to_rendered_handler_on_invalid_syntax() {
        let spec = MockSpec {
            summary: "override proof type".to_string(),
            default_task_status: "registered".to_string(),
            memory_required: true,
            request_key_fields: vec!["proposals[].proposal_id".to_string()],
            normal_request_policy: Some("register then proof".to_string()),
            aggregation_policy: Some("aggregation always errors".to_string()),
            proof_response_policy: Some("return proof on repeat".to_string()),
            proof_type_override: Some("risc0".to_string()),
            nth_responses: Vec::new(),
        };
        let invalid_source = r#"
use serde_json::Value;
use crate::{error_status, mock_proof_response, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext};

pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value {
    if body["aggregate].as_bool().unwrap_or(false) {
        return error_status("mock_aggregation_error", "aggregation is not available in this mock");
    }
    ok_status(proof_type(body), proposal_batch_id(body), "registered")
}
"#;

        let sanitized = sanitize_handler_source(invalid_source, &spec);

        assert!(sanitized.contains("body.get(\"aggregate\")"));
        assert!(sanitized.contains("mock_proof_response_with_type"));
    }

    #[test]
    fn sanitize_handler_source_falls_back_when_override_is_ignored() {
        let spec = MockSpec {
            summary: "override proof type".to_string(),
            default_task_status: "registered".to_string(),
            memory_required: true,
            request_key_fields: vec!["proposals[].proposal_id".to_string()],
            normal_request_policy: Some("register then proof".to_string()),
            aggregation_policy: Some("aggregation always errors".to_string()),
            proof_response_policy: Some("return proof on repeat".to_string()),
            proof_type_override: Some("risc0".to_string()),
            nth_responses: Vec::new(),
        };
        let source = r#"
use serde_json::Value;
use crate::{error_status, mock_proof_response, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext};

pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value {
    if body.get("aggregate") == Some(&Value::Bool(true)) {
        return error_status("mock_aggregation_error", "aggregation is not available in this mock");
    }
    let request_key = ctx.request_key(body);
    if ctx.has_seen_request(body) {
        return mock_proof_response_with_type(body, request_key, Some("risc0"));
    } else {
        ctx.mark_request_seen(body);
        return ok_status(proof_type(body), proposal_batch_id(body), "registered");
    }
}
"#;

        let sanitized = sanitize_handler_source(source, &spec);

        assert!(sanitized.contains("ok_status(\"risc0\""));
        assert!(sanitized.contains("mock_proof_response_with_type(body, \"repeat-request\", Some(\"risc0\"))"));
    }
}
