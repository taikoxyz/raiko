pub mod api_spec;
pub mod app;
pub mod generator;
pub mod models;
pub mod openrouter;
pub mod runner;
pub mod store;

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use anyhow::Context;

pub use app::app;
pub use models::{MockSpec, NthResponse, TicketRecord};
pub use openrouter::{FakeHandlerGenerator, OpenRouterAgent};
pub use runner::{FakeRunner, GatewayRunner, LocalCargoRunner};

use crate::{
    generator::Generator,
    models::TicketStatus,
    openrouter::{MockPlanner, RestrictedHandlerGenerator},
    store::TicketStore,
};

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
        let generator = Generator::new(
            std::env::temp_dir().join(format!("raiko-mock-studio-{}", std::process::id())),
        );
        Self {
            service: Arc::new(StudioService::new(
                Arc::new(planner),
                Arc::new(handler_generator),
                Arc::new(runner),
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
                Generator::new(Generator::default_root()),
            )),
        }
    }
}

pub struct StudioService {
    planner: Arc<dyn MockPlanner>,
    handler_generator: Arc<dyn RestrictedHandlerGenerator>,
    runner: Arc<dyn GatewayRunner>,
    generator: Generator,
    store: TicketStore,
    counter: AtomicU64,
}

impl StudioService {
    fn new(
        planner: Arc<dyn MockPlanner>,
        handler_generator: Arc<dyn RestrictedHandlerGenerator>,
        runner: Arc<dyn GatewayRunner>,
        generator: Generator,
    ) -> Self {
        Self {
            planner,
            handler_generator,
            runner,
            generator,
            store: TicketStore::default(),
            counter: AtomicU64::new(0),
        }
    }

    pub async fn submit_ticket(&self, requirement: &str) -> TicketRecord {
        let ticket_number = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let ticket_id = format!("ticket-{ticket_number}");
        let mut record = TicketRecord::pending(ticket_id.clone(), requirement.to_string());
        self.store.upsert(record.clone());

        let result = async {
            let spec_generation = self
                .planner
                .plan(requirement)
                .await
                .context("planner failed to produce a mock spec")?;
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
                },
            )?;
            anyhow::Ok((spec_generation.spec, base_url))
        }
        .await;

        match result {
            Ok((spec, base_url)) => {
                record = TicketRecord {
                    ticket_id,
                    rule_id: record.rule_id,
                    requirement: record.requirement,
                    summary: spec.summary,
                    status: TicketStatus::Running,
                    base_url: Some(base_url),
                    error: None,
                };
            }
            Err(error) => {
                let error_message = format!("{error:#}");
                record.status = TicketStatus::Failed;
                record.error = Some(error_message.clone());
                let _ = self.generator.write_receipt(
                    &ticket_id,
                    &crate::models::RunReceipt {
                        status: "failed".to_string(),
                        base_url: None,
                        error: Some(error_message),
                    },
                );
            }
        }

        self.store.upsert(record.clone());
        record
    }

    pub fn get_ticket(&self, ticket_id: &str) -> TicketRecord {
        self.store
            .get(ticket_id)
            .unwrap_or_else(|| TicketRecord::failed_lookup(ticket_id))
    }
}

pub use openrouter::FakePlanner;
