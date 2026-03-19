pub mod api_spec;
pub mod app;
pub mod generator;
pub mod models;
pub mod openrouter;
pub mod runner;
pub mod store;

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use anyhow::Context;
use async_trait::async_trait;

pub use app::app;
pub use models::{GatewayProxyRequest, MockSpec, NthResponse, StudioUiState, TicketRecord};
pub use openrouter::{FakeHandlerGenerator, OpenRouterAgent};
pub use runner::{FakeRunner, GatewayRunner, LocalCargoRunner};

use crate::{
    generator::Generator,
    models::{GatewayRuntimeStatus, StudioUiState as UiStateModel, TicketStatus},
    openrouter::{MockPlanner, RestrictedHandlerGenerator},
    store::TicketStore,
};

const DEFAULT_REQUIREMENT_TEXT: &str =
    "Generate a mock for /v3/proof/batch/shasta.";
const DEFAULT_GATEWAY_REQUEST_TEMPLATE: &str = r#"{
  "aggregate": false,
  "proof_type": "native",
  "proposals": [
    {
      "proposal_id": 101,
      "l1_inclusion_block_number": 9001
    }
  ]
}"#;

#[async_trait]
pub trait GatewayForwarder: Send + Sync {
    async fn forward_shasta_request(
        &self,
        base_url: &str,
        body: &str,
    ) -> anyhow::Result<String>;
}

#[derive(Clone, Default)]
pub struct ReqwestGatewayForwarder {
    client: reqwest::Client,
}

#[async_trait]
impl GatewayForwarder for ReqwestGatewayForwarder {
    async fn forward_shasta_request(
        &self,
        base_url: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        let base_url = normalize_gateway_target(base_url);
        let response = self
            .client
            .post(format!("{base_url}/v3/proof/batch/shasta"))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .context("gateway request failed")?;
        let text = response.text().await.context("gateway response was invalid")?;
        Ok(text)
    }
}

#[derive(Clone)]
pub struct AppState {
    pub(crate) service: Arc<StudioService>,
}

impl AppState {
    pub fn for_tests(
        planner: FakePlanner,
        handler_generator: FakeHandlerGenerator,
        runner: FakeRunner,
    ) -> Self {
        let unique_suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let generator = Generator::new(std::env::temp_dir().join(format!(
            "raiko-mock-studio-{}-{unique_suffix}",
            std::process::id()
        )));
        Self {
            service: Arc::new(StudioService::new(
                Arc::new(planner),
                Arc::new(handler_generator),
                Arc::new(runner),
                Arc::new(ReqwestGatewayForwarder::default()),
                generator,
            )),
        }
    }

    pub fn for_tests_in(
        generated_root: std::path::PathBuf,
        planner: FakePlanner,
        handler_generator: FakeHandlerGenerator,
        runner: FakeRunner,
    ) -> Self {
        let generator = Generator::new(generated_root);
        Self {
            service: Arc::new(StudioService::new(
                Arc::new(planner),
                Arc::new(handler_generator),
                Arc::new(runner),
                Arc::new(ReqwestGatewayForwarder::default()),
                generator,
            )),
        }
    }

    pub fn for_real_runner_tests_in(
        generated_root: std::path::PathBuf,
        planner: FakePlanner,
        handler_generator: FakeHandlerGenerator,
        runner: LocalCargoRunner,
    ) -> Self {
        let generator = Generator::new(generated_root);
        Self {
            service: Arc::new(StudioService::new(
                Arc::new(planner),
                Arc::new(handler_generator),
                Arc::new(runner),
                Arc::new(ReqwestGatewayForwarder::default()),
                generator,
            )),
        }
    }

    pub fn for_tests_with_gateway_forwarder(
        generated_root: std::path::PathBuf,
        planner: FakePlanner,
        handler_generator: FakeHandlerGenerator,
        runner: FakeRunner,
        gateway_forwarder: Arc<dyn GatewayForwarder>,
    ) -> Self {
        let generator = Generator::new(generated_root);
        Self {
            service: Arc::new(StudioService::new(
                Arc::new(planner),
                Arc::new(handler_generator),
                Arc::new(runner),
                gateway_forwarder,
                generator,
            )),
        }
    }

    pub fn new(
        planner: Arc<dyn MockPlanner>,
        handler_generator: Arc<dyn RestrictedHandlerGenerator>,
        runner: Arc<dyn GatewayRunner>,
    ) -> Self {
        Self {
            service: Arc::new(StudioService::new(
                planner,
                handler_generator,
                runner,
                Arc::new(ReqwestGatewayForwarder::default()),
                Generator::new(Generator::default_root()),
            )),
        }
    }
}

pub struct StudioService {
    planner: Arc<dyn MockPlanner>,
    handler_generator: Arc<dyn RestrictedHandlerGenerator>,
    runner: Arc<dyn GatewayRunner>,
    gateway_forwarder: Arc<dyn GatewayForwarder>,
    generator: Generator,
    store: TicketStore,
    counter: AtomicU64,
    active_ticket_id: Mutex<Option<String>>,
}

impl StudioService {
    fn new(
        planner: Arc<dyn MockPlanner>,
        handler_generator: Arc<dyn RestrictedHandlerGenerator>,
        runner: Arc<dyn GatewayRunner>,
        gateway_forwarder: Arc<dyn GatewayForwarder>,
        generator: Generator,
    ) -> Self {
        let restored_tickets = generator.restore_ticket_records();
        let next_counter_seed = generator.max_ticket_number();
        Self {
            planner,
            handler_generator,
            runner,
            gateway_forwarder,
            generator,
            store: TicketStore::from_records(restored_tickets),
            counter: AtomicU64::new(next_counter_seed),
            active_ticket_id: Mutex::new(None),
        }
    }

    pub async fn submit_ticket(&self, requirement: &str) -> TicketRecord {
        let ticket_number = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let ticket_id = format!("ticket-{ticket_number}");
        let mut record = TicketRecord::pending(ticket_id.clone(), requirement.to_string());
        self.store.upsert(record.clone());

        let result = async {
            let mut spec_generation = self
                .planner
                .plan(requirement)
                .await
                .context("planner failed to produce a mock spec")?;
            normalize_spec_for_requirement(requirement, &mut spec_generation.spec);
            let handler_generation = self
                .handler_generator
                .generate_handler(requirement, &spec_generation.spec)
                .await
                .context("handler generator failed to produce restricted rust")?;
            self.generator.write_rule_files(
                &ticket_id,
                requirement,
                &spec_generation.spec,
                &handler_generation.source,
            )?;
            self.generator.write_llm_artifacts(
                &ticket_id,
                &spec_generation,
                &handler_generation,
            )?;
            self.generator
                .update_index(&ticket_id, &spec_generation.spec, "building", None)?;
            let rule_dir = self.generator.rule_dir(&ticket_id);
            let base_url = self.runner.run(&ticket_id, &rule_dir).await?;
            self.generator.update_index(
                &ticket_id,
                &spec_generation.spec,
                "running",
                Some(&base_url),
            )?;
            self.generator.write_receipt(
                &ticket_id,
                &crate::models::RunReceipt {
                    status: "running".to_string(),
                    base_url: Some(base_url.clone()),
                    error: None,
                    handler_mode: Some(handler_generation.handler_mode.clone()),
                    handler_validation_error: handler_generation.validation_error.clone(),
                },
            )?;
            anyhow::Ok((spec_generation.spec, base_url, handler_generation))
        }
        .await;

        match result {
            Ok((spec, base_url, handler_generation)) => {
                record = TicketRecord {
                    ticket_id,
                    rule_id: record.rule_id,
                    requirement: record.requirement,
                    summary: spec.summary,
                    status: TicketStatus::Running,
                    base_url: Some(base_url),
                    error: None,
                    gateway_runtime: GatewayRuntimeStatus::Online,
                    handler_mode: handler_generation.handler_mode,
                    handler_validation_error: handler_generation.validation_error,
                };
                *self.active_ticket_id.lock().expect("active ticket store poisoned") =
                    Some(record.ticket_id.clone());
            }
            Err(error) => {
                let error_message = format!("{error:#}");
                record.status = TicketStatus::Failed;
                record.error = Some(error_message.clone());
                record.gateway_runtime = GatewayRuntimeStatus::Offline;
                let _ = self.generator.write_receipt(
                    &ticket_id,
                    &crate::models::RunReceipt {
                        status: "failed".to_string(),
                        base_url: None,
                        error: Some(error_message),
                        handler_mode: Some(record.handler_mode.clone()),
                        handler_validation_error: record.handler_validation_error.clone(),
                    },
                );
            }
        }

        self.store.upsert(record.clone());
        record
    }

    pub fn get_ticket(&self, ticket_id: &str) -> TicketRecord {
        let mut ticket = self
            .store
            .get(ticket_id)
            .unwrap_or_else(|| TicketRecord::failed_lookup(ticket_id));
        ticket.gateway_runtime = self.runtime_for_ticket(&ticket.ticket_id);
        ticket
    }

    pub fn list_tickets(&self) -> Vec<TicketRecord> {
        self.store
            .list()
            .into_iter()
            .map(|mut ticket| {
                ticket.gateway_runtime = self.runtime_for_ticket(&ticket.ticket_id);
                ticket
            })
            .collect()
    }

    pub fn ui_state(&self) -> UiStateModel {
        UiStateModel {
            tickets: self.list_tickets(),
            default_requirement: DEFAULT_REQUIREMENT_TEXT.to_string(),
            gateway_request_template: DEFAULT_GATEWAY_REQUEST_TEMPLATE.to_string(),
            preferred_gateway_target: preferred_gateway_target_from_env().unwrap_or_default(),
        }
    }

    pub async fn proxy_gateway_request(
        &self,
        ticket_id: &str,
        target: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<String> {
        let ticket = self
            .store
            .get(ticket_id)
            .ok_or_else(|| anyhow::anyhow!("ticket not found"))?;
        let base_url = Some(target.trim())
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or(ticket.base_url)
            .filter(|_| matches!(ticket.status, TicketStatus::Running))
            .ok_or_else(|| anyhow::anyhow!("ticket is not running"))?;
        let body = serde_json::to_string(body).context("failed to serialize gateway request")?;

        self.gateway_forwarder
            .forward_shasta_request(&base_url, &body)
            .await
    }
}

impl StudioService {
    fn runtime_for_ticket(&self, ticket_id: &str) -> GatewayRuntimeStatus {
        match self
            .active_ticket_id
            .lock()
            .expect("active ticket store poisoned")
            .as_deref()
        {
            Some(active_ticket_id) if active_ticket_id == ticket_id => GatewayRuntimeStatus::Online,
            _ => GatewayRuntimeStatus::Offline,
        }
    }
}

fn preferred_gateway_target_from_env() -> Option<String> {
    std::env::var("PUBLIC_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_gateway_target(target: &str) -> String {
    let trimmed = target.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    }
}

pub use openrouter::FakePlanner;

fn normalize_spec_for_requirement(requirement: &str, spec: &mut MockSpec) {
    let normalized = requirement.to_ascii_lowercase();
    if !mentions_repeated_request_behavior(&normalized) {
        spec.memory_required = false;
        spec.nth_responses.clear();
    }

    if !mentions_aggregate_behavior(&normalized) {
        spec.aggregation_policy = None;
    }
}

fn mentions_repeated_request_behavior(requirement: &str) -> bool {
    let markers = [
        "same request",
        "same proposal",
        "same proposal_id",
        "repeat request",
        "repeated request",
        "on the 2nd",
        "on the 3rd",
        "on the 4th",
        "on the 5th",
        "1st",
        "2nd",
        "3rd",
        "4th",
        "5th",
        "nth",
        "first call",
        "second call",
        "third call",
        "fourth call",
        "fifth call",
        "each proposal_id",
        "per request",
    ];
    markers.iter().any(|marker| requirement.contains(marker))
}

fn mentions_aggregate_behavior(requirement: &str) -> bool {
    requirement.contains("aggregate")
}

#[cfg(test)]
mod tests {
    use super::normalize_spec_for_requirement;
    use crate::models::{MockSpec, NthResponse};

    #[test]
    fn normalize_spec_for_requirement_clears_spurious_repeat_state_when_intent_is_single_shot() {
        let mut spec = MockSpec {
            summary: "single shot proof".to_string(),
            default_task_status: "success".to_string(),
            memory_required: true,
            request_key_fields: vec!["proposals[].proposal_id".to_string()],
            normal_request_policy: Some("normal request returns sp1".to_string()),
            aggregation_policy: Some("aggregate errors".to_string()),
            proof_response_policy: Some("proof response".to_string()),
            proof_type_override: Some("sp1".to_string()),
            nth_responses: vec![NthResponse {
                n: 2,
                kind: "error".to_string(),
                error: Some("mock_error".to_string()),
                message: Some("unexpected repeat failure".to_string()),
            }],
        };

        normalize_spec_for_requirement(
            "Generate a mock for /v3/proof/batch/shasta. return normal response with proof_type is sp1 if aggregate is false, return error if aggregate is true",
            &mut spec,
        );

        assert!(!spec.memory_required);
        assert!(spec.nth_responses.is_empty());
        assert!(spec.aggregation_policy.is_some());
    }
}
