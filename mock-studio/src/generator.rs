use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde_json::json;

use crate::models::{GeneratedIndexEntry, HandlerGeneration, MockSpec, RunReceipt, SpecGeneration};

#[derive(Clone)]
pub struct Generator {
    generated_root: PathBuf,
}

impl Generator {
    pub fn new(generated_root: PathBuf) -> Self {
        Self { generated_root }
    }

    pub fn default_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("mock-gateway")
            .join("generated")
    }

    pub fn write_rule_files(
        &self,
        rule_id: &str,
        requirement: &str,
        spec: &MockSpec,
        handler_source: &str,
    ) -> anyhow::Result<()> {
        let rule_dir = self.generated_root.join(rule_id);
        fs::create_dir_all(&rule_dir)
            .with_context(|| format!("failed to create {}", rule_dir.display()))?;

        fs::write(rule_dir.join("conversation.md"), requirement)
            .with_context(|| format!("failed to write conversation for {rule_id}"))?;
        fs::write(
            rule_dir.join("meta.json"),
            serde_json::to_vec_pretty(&json!({
                "rule_id": rule_id,
                "summary": spec.summary
            }))?,
        )
        .with_context(|| format!("failed to write meta for {rule_id}"))?;
        fs::write(rule_dir.join("spec.json"), serde_json::to_vec_pretty(spec)?)
            .with_context(|| format!("failed to write spec for {rule_id}"))?;
        fs::write(rule_dir.join("ticket.rs"), handler_source)
            .with_context(|| format!("failed to write ticket.rs for {rule_id}"))?;

        Ok(())
    }

    pub fn rule_dir(&self, rule_id: &str) -> PathBuf {
        self.generated_root.join(rule_id)
    }

    pub fn write_llm_artifacts(
        &self,
        rule_id: &str,
        spec_generation: &SpecGeneration,
        handler_generation: &HandlerGeneration,
    ) -> anyhow::Result<()> {
        let llm_dir = self.rule_dir(rule_id).join("llm");
        fs::create_dir_all(&llm_dir)?;
        fs::write(llm_dir.join("spec_prompt.md"), &spec_generation.prompt)?;
        fs::write(
            llm_dir.join("spec_response.json"),
            &spec_generation.response,
        )?;
        fs::write(
            llm_dir.join("handler_prompt.md"),
            &handler_generation.prompt,
        )?;
        fs::write(
            llm_dir.join("handler_response.json"),
            &handler_generation.response,
        )?;
        Ok(())
    }

    pub fn write_receipt(&self, rule_id: &str, receipt: &RunReceipt) -> anyhow::Result<()> {
        fs::write(
            self.rule_dir(rule_id).join("receipt.json"),
            serde_json::to_vec_pretty(receipt)?,
        )?;
        Ok(())
    }

    pub fn update_index(
        &self,
        rule_id: &str,
        spec: &MockSpec,
        status: &str,
        base_url: Option<&str>,
    ) -> anyhow::Result<()> {
        fs::create_dir_all(&self.generated_root)?;
        let index_path = self.generated_root.join("index.json");
        let mut entries: Vec<GeneratedIndexEntry> = if index_path.exists() {
            serde_json::from_slice(&fs::read(&index_path)?)?
        } else {
            Vec::new()
        };

        entries.retain(|entry| entry.rule_id != rule_id);
        entries.push(GeneratedIndexEntry {
            rule_id: rule_id.to_string(),
            summary: spec.summary.clone(),
            status: status.to_string(),
            base_url: base_url.unwrap_or_default().to_string(),
        });

        fs::write(index_path, serde_json::to_vec_pretty(&entries)?)?;
        Ok(())
    }
}
