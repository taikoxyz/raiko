use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde_json::json;

use crate::models::{
    GatewayRuntimeStatus, GeneratedIndexEntry, HandlerGeneration, MockSpec, RunReceipt,
    SpecGeneration, TicketRecord, TicketStatus,
};

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
        fs::write(
            llm_dir.join("handler_generation_meta.json"),
            serde_json::to_vec_pretty(&json!({
                "handler_mode": handler_generation.handler_mode,
                "validation_error": handler_generation.validation_error,
            }))?,
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

    pub fn restore_ticket_records(&self) -> Vec<TicketRecord> {
        let entries = match fs::read_dir(&self.generated_root) {
            Ok(entries) => entries,
            Err(_) => return Vec::new(),
        };

        let mut records = entries
            .filter_map(Result::ok)
            .filter_map(|entry| self.restore_ticket_record(&entry.path()).ok())
            .collect::<Vec<_>>();
        records.sort_by_key(|record| extract_ticket_number(&record.ticket_id));
        records
    }

    pub fn max_ticket_number(&self) -> u64 {
        self.restore_ticket_records()
            .iter()
            .filter_map(|record| extract_ticket_number(&record.ticket_id))
            .max()
            .unwrap_or(0)
    }

    fn restore_ticket_record(&self, rule_dir: &Path) -> anyhow::Result<TicketRecord> {
        if !rule_dir.is_dir() {
            anyhow::bail!("not a rule directory");
        }

        let Some(rule_id) = rule_dir.file_name().and_then(|name| name.to_str()) else {
            anyhow::bail!("invalid rule directory name");
        };
        if extract_ticket_number(rule_id).is_none() {
            anyhow::bail!("not a ticket directory");
        }

        let requirement = fs::read_to_string(rule_dir.join("conversation.md")).unwrap_or_default();
        let summary = read_summary(rule_dir).unwrap_or_else(|| "pending".to_string());
        let receipt = fs::read(rule_dir.join("receipt.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<RunReceipt>(&bytes).ok());
        let status_text = receipt
            .as_ref()
            .map(|receipt| receipt.status.as_str())
            .unwrap_or("pending");
        let base_url = receipt.as_ref().and_then(|receipt| receipt.base_url.clone());
        let error = receipt.as_ref().and_then(|receipt| receipt.error.clone());
        let handler_mode = receipt
            .as_ref()
            .and_then(|receipt| receipt.handler_mode.clone())
            .unwrap_or_else(|| "llm".to_string());
        let handler_validation_error = receipt
            .as_ref()
            .and_then(|receipt| receipt.handler_validation_error.clone());

        Ok(TicketRecord {
            ticket_id: rule_id.to_string(),
            rule_id: rule_id.to_string(),
            requirement,
            summary,
            status: parse_ticket_status(status_text),
            base_url,
            error,
            gateway_runtime: GatewayRuntimeStatus::Offline,
            handler_mode,
            handler_validation_error,
        })
    }
}

fn read_summary(rule_dir: &Path) -> Option<String> {
    let meta = fs::read(rule_dir.join("meta.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())?;
    meta.get("summary")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn parse_ticket_status(status: &str) -> TicketStatus {
    match status {
        "running" => TicketStatus::Running,
        "failed" => TicketStatus::Failed,
        _ => TicketStatus::Pending,
    }
}

fn extract_ticket_number(ticket_id: &str) -> Option<u64> {
    ticket_id.strip_prefix("ticket-")?.parse::<u64>().ok()
}
