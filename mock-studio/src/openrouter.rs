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

const PLANNER_SYSTEM_PROMPT: &str = "You are planning a constrained Rust mock gateway for exactly one route: /v3/proof/batch/shasta. Return JSON only with these top-level keys: summary, default_task_status, memory_required, request_key_fields, normal_request_policy, aggregation_policy, proof_response_policy, proof_type_override, nth_responses. Platform invariants: only implement behavior for /v3/proof/batch/shasta; do not change the outer JSON response envelope shape; do not invent new routes, helpers, or response families; proof-shaped success responses must remain compatible with the existing proof response envelope; if a fixed proof_type is required, set top-level proof_type_override to that exact string; do not hide proof_type overrides only inside nested policy text. State and counting rules: if the user intent refers to repeated behavior for the same request, the same proposal_id, the same request body, or nth behavior per logical request, then memory_required must be true; per-request repeated behavior must be modeled using request_key_fields and per-request memory, not global process-wide call count; nth_responses describes ordered behavior for the same logical request when the requirement implies repeated calls to that request; do not use aggregate requests as a default error path unless the user intent explicitly asks for aggregate-specific error behavior. Aggregation rules: treat aggregate requests as a separate branch only when the user intent requires distinct aggregate behavior; if the user intent does not mention aggregate behavior, do not add extra aggregate-only behavior on your own. Output rules: keep the plan deterministic and minimal; default_task_status must be a snake_case task status string; nth_responses entries may only use keys n, kind, error, message; prefer simple explicit behavior over vague prose.";

const HANDLER_SYSTEM_PROMPT: &str = "You generate Rust code for one restricted handler module. Return JSON only with key source. The Rust source must define exactly pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value. Required imports: use serde_json::Value; use crate::{error_status, mock_proof_response, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext};. Hard rules: only implement /v3/proof/batch/shasta; do not change the outer JSON response envelope shape; do not define routers, modules, helpers, mains, or tests; the generated handler must import serde_json::Value; the generated handler must import only the allowed crate helper set; the generated handler must call at least one allowed helper; the generated handler must be valid Rust syntax; error_status always takes exactly two string arguments; ok_status always takes a proof type expression, proposal_batch_id(body), and a task status string; mock_proof_response_with_type must use the exact proof_type_override value from Task Spec when one exists; never invent placeholder proof types such as fixed-type; when using mock_proof_response or mock_proof_response_with_type, the label argument must use a non-empty descriptive string label and must never be an empty string. State rules: if Task Spec implies repeated behavior for the same logical request, use per-request memory with ctx.request_key(body), ctx.has_seen_request(body), and ctx.mark_request_seen(body); do not use ctx.call_index() to implement nth behavior for the same request; ctx.call_index() is only global process-wide state and must not be used as a substitute for per-request memory. Aggregation rules: branch on aggregate requests only if Task Spec explicitly describes aggregate-specific behavior; do not add aggregate-only error behavior unless Task Spec explicitly requires it. Implementation rules: use ok_status only for registered or work-in-progress style success responses; use proof helpers only for proof-shaped success responses; keep the handler deterministic and minimal.";

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
            handler_mode: "renderer".to_string(),
            validation_error: None,
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
                PLANNER_SYSTEM_PROMPT,
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
                HANDLER_SYSTEM_PROMPT,
                &prompt,
            )
            .await?;

        let payload: serde_json::Value =
            serde_json::from_str(&content).context("failed to parse handler generation json")?;
        let source = payload["source"]
            .as_str()
            .ok_or_else(|| anyhow!("handler generation missing source"))?;
        let sanitized = sanitize_handler_source(source, spec);
        Ok(HandlerGeneration {
            source: sanitized.source,
            prompt,
            response: content,
            handler_mode: sanitized.handler_mode,
            validation_error: sanitized.validation_error,
        })
    }
}

struct SanitizedHandlerSource {
    source: String,
    handler_mode: String,
    validation_error: Option<String>,
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

    let default_response = render_default_response(spec);

    format!(
        "use serde_json::Value;\n\nuse crate::{{error_status, mock_proof_response, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext}};\n\npub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value {{\n    match ctx.call_index() {{\n{nth_match_arms}        _ => {{}}\n    }}\n\n{aggregate_branch}{seen_branch}    {default_response}\n}}\n"
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

fn render_default_response(spec: &MockSpec) -> String {
    if should_emit_proof_response(spec) && !spec.memory_required {
        return render_proof_response_call(spec, "default-proof");
    }

    format!(
        "ok_status({}, proposal_batch_id(body), {:?})",
        render_default_proof_type_expression(spec),
        spec.default_task_status
    )
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

    validate_allowed_crate_imports(source)?;

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

    validate_rust_syntax(source)?;
    validate_proof_helper_labels(source)?;

    Ok(())
}

fn validate_proof_helper_labels(source: &str) -> anyhow::Result<()> {
    for function_name in ["mock_proof_response(", "mock_proof_response_with_type("] {
        let mut remainder = source;
        while let Some(start) = remainder.find(function_name) {
            let after_name = &remainder[start + function_name.len()..];
            let (args, end) = extract_call_arguments(after_name)
                .ok_or_else(|| anyhow!("generated handler contains malformed proof helper call"))?;
            let parsed_args = split_top_level_args(args);
            if let Some(label) = parsed_args.get(1).map(|value| value.trim()) {
                if label == "\"\"" {
                    anyhow::bail!(
                        "generated handler must not call proof helpers with an empty label"
                    );
                }
            }
            remainder = &after_name[end + 1..];
        }
    }

    Ok(())
}

fn extract_call_arguments(source: &str) -> Option<(&str, usize)> {
    let mut depth = 1usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in source.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&source[..idx], idx));
                }
            }
            _ => {}
        }
    }

    None
}

fn split_top_level_args(source: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in source.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                args.push(source[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    if start <= source.len() {
        args.push(source[start..].trim());
    }

    args
}

fn validate_allowed_crate_imports(source: &str) -> anyhow::Result<()> {
    let allowed = [
        "error_status",
        "mock_proof_response",
        "mock_proof_response_with_type",
        "ok_status",
        "proof_type",
        "proposal_batch_id",
        "MockContext",
    ];

    let Some(start) = source.find("use crate::{") else {
        anyhow::bail!("generated handler must import the allowed helper set");
    };
    let after_start = &source[start + "use crate::{".len()..];
    let Some(end) = after_start.find("};") else {
        anyhow::bail!("generated handler must import the allowed helper set");
    };

    let imports = after_start[..end]
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();

    if imports.is_empty() || !imports.contains(&"MockContext") {
        anyhow::bail!("generated handler must import the allowed helper set");
    }

    for item in imports {
        if !allowed.contains(&item) {
            anyhow::bail!("generated handler imports unsupported crate item: {item}");
        }
    }

    Ok(())
}

fn sanitize_handler_source(source: &str, spec: &MockSpec) -> SanitizedHandlerSource {
    match validate_handler_source(source).and_then(|_| validate_handler_matches_spec(source, spec))
    {
        Ok(()) => SanitizedHandlerSource {
            source: source.to_string(),
            handler_mode: "llm".to_string(),
            validation_error: None,
        },
        Err(error) => SanitizedHandlerSource {
            source: render_handler_from_spec(spec),
            handler_mode: "fallback_renderer".to_string(),
            validation_error: Some(error.to_string()),
        },
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
        if source.contains("ok_status(") && !source.contains(&expected_ok) {
            anyhow::bail!("generated handler does not honor proof_type_override in ok_status");
        }
        if source.contains("mock_proof_response_with_type(") && !source.contains(&expected_proof) {
            anyhow::bail!(
                "generated handler does not honor proof_type_override in proof response"
            );
        }
        if !source.contains("ok_status(")
            && !source.contains("mock_proof_response_with_type(")
            && !source.contains(&expected_proof)
        {
            anyhow::bail!("generated handler does not honor proof_type_override");
        }
    }

    Ok(())
}

fn build_planner_prompt(requirement: &str, api_spec: &ShastaApiSpec) -> anyhow::Result<String> {
    Ok(format!(
        "User Intent:\n{requirement}\n\nPlatform Invariants:\n- Only implement behavior for /v3/proof/batch/shasta.\n- Do not change the outer JSON envelope shape.\n- Aggregate requests are a separate branch from normal proof requests.\n- Repeated-request behavior must be reasoned about per request_key for the same non-aggregate request.\n- proof-shaped success responses must use the constrained proof helpers.\n- If a fixed proof_type is needed, it must be represented as a top-level exact proof_type_override string.\n- Proof payloads must remain valid hex-string based mock responses.\n\nAPI Spec:\n{}\n\nOutput Contract:\n- Return JSON only.\n- Required keys: summary, default_task_status, memory_required, request_key_fields, normal_request_policy, aggregation_policy, proof_response_policy, proof_type_override, nth_responses.\n- nth_responses is an array of objects with keys n, kind, error, message.\n- default_task_status must be a snake_case task status string.",
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
        "User Intent:\n{requirement}\n\nTask Spec:\n{}\n\nPlatform Invariants:\n- Only implement /v3/proof/batch/shasta.\n- Do not change the outer JSON envelope shape.\n- Aggregate requests must branch before normal proof flow.\n- Repeated-request behavior is per request_key, not global process-wide call count.\n- Use exact proof_type_override values from Task Spec when fixed proof_type behavior is required.\n- proof-shaped success responses must use the constrained proof helpers.\n- Import serde_json::Value and only the allowed crate helper set.\n- Call at least one allowed helper.\n- The Rust source must be syntactically valid.\n- Proof helper labels must be non-empty descriptive strings.\n\nAPI Spec:\n{}\n\nMemory Contract:\n- ctx.request_key(body)\n- ctx.has_seen_request(body)\n- ctx.mark_request_seen(body)\n- ctx.call_index() exists but must not be used for per-request-key nth-response behavior.\n\nGenerate Rust source for the restricted handler module. Return JSON only as {{\"source\": \"...\"}}.",
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
        build_handler_prompt, build_planner_prompt, extract_openrouter_message,
        parse_mock_spec, render_handler_from_spec, sanitize_handler_source,
        validate_handler_source, HANDLER_SYSTEM_PROMPT, PLANNER_SYSTEM_PROMPT,
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
    fn validate_handler_source_accepts_subset_of_allowed_helper_imports() {
        let source = r#"
use serde_json::Value;
use crate::{error_status, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext};

pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value {
    if ctx.has_seen_request(body) {
        return mock_proof_response_with_type(body, "label", Some("sp1"));
    }
    ok_status(proof_type(body), proposal_batch_id(body), "registered")
}
"#;

        validate_handler_source(source).unwrap();
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
    fn shasta_api_spec_exposes_key_field_types_for_llm_context() {
        let spec = shasta_api_spec();
        let rendered = serde_json::to_string(&spec).unwrap();

        assert!(rendered.contains("request_field_schemas"));
        assert!(rendered.contains("\"path\":\"aggregate\""));
        assert!(rendered.contains("\"type_name\":\"bool\""));
        assert!(rendered.contains("\"path\":\"proof_type\""));
        assert!(rendered.contains("\"type_name\":\"string\""));
        assert!(rendered.contains("\"path\":\"proposals[].proposal_id\""));
        assert!(rendered.contains("\"type_name\":\"u64\""));
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
        assert!(prompt.contains("User Intent"));
        assert!(prompt.contains("Platform Invariants"));
        assert!(prompt.contains("per request_key"));
        assert!(prompt.contains("Import serde_json::Value and only the allowed crate helper set"));
        assert!(prompt.contains("Call at least one allowed helper"));
        assert!(prompt.contains("The Rust source must be syntactically valid"));
        assert!(prompt.contains("Proof helper labels must be non-empty descriptive strings"));
        assert!(!prompt.contains("Task Requirement"));
    }

    #[test]
    fn planner_prompt_separates_user_intent_from_platform_invariants() {
        let prompt = build_planner_prompt(
            "2nd request returns sp1 proof; later requests error",
            &shasta_api_spec(),
        )
        .unwrap();

        assert!(prompt.contains("User Intent"));
        assert!(prompt.contains("Platform Invariants"));
        assert!(prompt.contains("Do not change the outer JSON envelope shape"));
        assert!(prompt.contains("per request_key"));
    }

    #[test]
    fn system_prompts_embed_invariants_and_avoid_placeholder_proof_types() {
        assert!(PLANNER_SYSTEM_PROMPT.contains("outer JSON response envelope shape"));
        assert!(PLANNER_SYSTEM_PROMPT.contains("per-request repeated behavior"));
        assert!(PLANNER_SYSTEM_PROMPT.contains("memory_required must be true"));
        assert!(PLANNER_SYSTEM_PROMPT.contains("do not add extra aggregate-only behavior"));
        assert!(HANDLER_SYSTEM_PROMPT.contains("must not be used as a substitute for per-request memory"));
        assert!(HANDLER_SYSTEM_PROMPT.contains("do not add aggregate-only error behavior unless Task Spec explicitly requires it"));
        assert!(HANDLER_SYSTEM_PROMPT.contains("never invent placeholder proof types"));
        assert!(HANDLER_SYSTEM_PROMPT.contains("must import serde_json::Value"));
        assert!(HANDLER_SYSTEM_PROMPT.contains("must import only the allowed crate helper set"));
        assert!(HANDLER_SYSTEM_PROMPT.contains("must call at least one allowed helper"));
        assert!(HANDLER_SYSTEM_PROMPT.contains("must be valid Rust syntax"));
        assert!(HANDLER_SYSTEM_PROMPT.contains("must use a non-empty descriptive string label"));
        assert!(!HANDLER_SYSTEM_PROMPT.contains("fixed-type\")) only"));
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
    fn render_handler_from_spec_emits_single_shot_proof_response_without_memory() {
        let spec = MockSpec {
            summary: "single proof response".to_string(),
            default_task_status: "ok".to_string(),
            memory_required: false,
            request_key_fields: vec!["proposals[].proposal_id".to_string()],
            normal_request_policy: Some("always return a proof-shaped response".to_string()),
            aggregation_policy: Some("aggregate always errors".to_string()),
            proof_response_policy: Some("return proof immediately".to_string()),
            proof_type_override: Some("risc0".to_string()),
            nth_responses: Vec::new(),
        };

        let source = render_handler_from_spec(&spec);

        assert!(source.contains("mock_proof_response_with_type(body, \"default-proof\", Some(\"risc0\"))"));
        assert!(!source.contains("ok_status(\"risc0\""));
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

        assert_eq!(sanitized.handler_mode, "fallback_renderer");
        assert!(sanitized
            .validation_error
            .as_deref()
            .unwrap()
            .contains("valid Rust syntax"));
        assert!(sanitized.source.contains("body.get(\"aggregate\")"));
        assert!(sanitized.source.contains("mock_proof_response_with_type"));
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

        assert_eq!(sanitized.handler_mode, "fallback_renderer");
        assert!(sanitized
            .validation_error
            .as_deref()
            .unwrap()
            .contains("proof_type_override"));
        assert!(sanitized.source.contains("ok_status(\"risc0\""));
        assert!(sanitized
            .source
            .contains("mock_proof_response_with_type(body, \"repeat-request\", Some(\"risc0\"))"));
    }

    #[test]
    fn sanitize_handler_source_accepts_proof_only_handler_with_proof_type_override() {
        let spec = MockSpec {
            summary: "proof only override".to_string(),
            default_task_status: "registered".to_string(),
            memory_required: true,
            request_key_fields: vec!["proposals[].proposal_id".to_string()],
            normal_request_policy: Some("aggregate false returns sp1 proof".to_string()),
            aggregation_policy: Some("aggregate true errors".to_string()),
            proof_response_policy: Some("proof response uses sp1".to_string()),
            proof_type_override: Some("sp1".to_string()),
            nth_responses: Vec::new(),
        };
        let source = r#"
use serde_json::Value;
use crate::{error_status, mock_proof_response, mock_proof_response_with_type, ok_status, proof_type, proposal_batch_id, MockContext};

pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value {
    let aggregate = body.get("aggregate").and_then(Value::as_bool).unwrap_or(false);
    let label = "ShastaBatchProofResponse";

    if aggregate {
        return error_status("InvalidRequest", "Aggregation requests are not supported in this context");
    }

    let request_key = ctx.request_key(body);
    if ctx.has_seen_request(body) {
        return mock_proof_response_with_type(body, label, Some("sp1"));
    }
    ctx.mark_request_seen(body);

    mock_proof_response_with_type(body, label, Some("sp1"))
}
"#;

        let sanitized = sanitize_handler_source(source, &spec);

        assert_eq!(sanitized.handler_mode, "llm");
        assert!(sanitized.validation_error.is_none());
        assert!(sanitized.source.contains("Some(\"sp1\")"));
    }

    #[test]
    fn sanitize_handler_source_falls_back_on_empty_proof_label() {
        let spec = MockSpec {
            summary: "proof helper label must not be empty".to_string(),
            default_task_status: "ok".to_string(),
            memory_required: false,
            request_key_fields: vec!["proposals[].proposal_id".to_string()],
            normal_request_policy: Some("always return a proof-shaped response".to_string()),
            aggregation_policy: Some("aggregate always errors".to_string()),
            proof_response_policy: Some("return proof immediately".to_string()),
            proof_type_override: Some("risc0".to_string()),
            nth_responses: Vec::new(),
        };
        let source = r#"
use serde_json::Value;
use crate::{error_status, mock_proof_response_with_type, MockContext};

pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value {
    if body.get("aggregate").and_then(Value::as_bool).unwrap_or(false) {
        return error_status("Aggregate requests not supported", "Please submit a normal proof request.");
    }

    mock_proof_response_with_type(body, "", Some("risc0"))
}
"#;

        let sanitized = sanitize_handler_source(source, &spec);

        assert_eq!(sanitized.handler_mode, "fallback_renderer");
        assert!(sanitized
            .validation_error
            .as_deref()
            .unwrap()
            .contains("must not call proof helpers with an empty label"));
        assert!(sanitized
            .source
            .contains("mock_proof_response_with_type(body, \"default-proof\", Some(\"risc0\"))"));
    }

    #[test]
    #[ignore = "example contract for a future per-request nth renderer"]
    fn render_handler_contract_example_for_per_request_nth_error() {
        let spec = MockSpec {
            summary: "same request registers first, returns proof next, then errors".to_string(),
            default_task_status: "registered".to_string(),
            memory_required: true,
            request_key_fields: vec![
                "proposals[].proposal_id".to_string(),
                "proof_type".to_string(),
            ],
            normal_request_policy: Some("1st registered, 2nd proof, 3rd error for the same request".to_string()),
            aggregation_policy: None,
            proof_response_policy: Some("repeat request returns proof-shaped success".to_string()),
            proof_type_override: Some("sp1".to_string()),
            nth_responses: vec![NthResponse {
                n: 3,
                kind: "error".to_string(),
                error: Some("mock_error".to_string()),
                message: Some("third call fails for the same request".to_string()),
            }],
        };

        let source = render_handler_from_spec(&spec);

        // This is the target contract we actually want from the renderer:
        // per-request memory, not global call_index.
        assert!(source.contains("ctx.request_key(body)"));
        assert!(source.contains("ctx.has_seen_request(body)"));
        assert!(source.contains("ctx.mark_request_seen(body)"));
        assert!(source.contains("mock_proof_response_with_type"));
        assert!(source.contains("Some(\"sp1\")"));
        assert!(source.contains("error_status(\"mock_error\", \"third call fails for the same request\")"));
        assert!(!source.contains("ctx.call_index()"));
    }
}
