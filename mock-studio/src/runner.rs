use std::{
    env,
    fs::{self, File},
    io::Write,
    path::Path,
    process::Stdio,
    time::Duration,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use async_trait::async_trait;
use tokio::process::Command;

#[async_trait]
pub trait GatewayRunner: Send + Sync {
    async fn run(&self, rule_id: &str, rule_dir: &Path) -> anyhow::Result<String>;
}

#[derive(Clone)]
pub struct FakeRunner {
    base_url: String,
}

impl FakeRunner {
    pub fn success(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
        }
    }
}

#[async_trait]
impl GatewayRunner for FakeRunner {
    async fn run(&self, _rule_id: &str, rule_dir: &Path) -> anyhow::Result<String> {
        fs::write(rule_dir.join("build.log"), "fake build ok\n")?;
        fs::write(rule_dir.join("runtime.log"), "fake runtime ok\n")?;
        Ok(self.base_url.clone())
    }
}

pub struct LocalCargoRunner {
    health_timeout: Duration,
    health_poll_interval: Duration,
}

impl Default for LocalCargoRunner {
    fn default() -> Self {
        Self {
            health_timeout: Duration::from_secs(10),
            health_poll_interval: Duration::from_millis(200),
        }
    }
}

#[async_trait]
impl GatewayRunner for LocalCargoRunner {
    async fn run(&self, rule_id: &str, rule_dir: &Path) -> anyhow::Result<String> {
        let bind = resolve_gateway_bind(rule_id, env::var("MOCK_GATEWAY_PORT").ok().as_deref())?;
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        let build_log_path = rule_dir.join("build.log");
        let runtime_log_path = rule_dir.join("runtime.log");

        let build_output = Command::new("cargo")
            .arg("build")
            .arg("-p")
            .arg("raiko-mock-gateway")
            .current_dir(workspace_root)
            .env("MOCK_RULE_ID", rule_id)
            .env("MOCK_RULES_ROOT", rule_dir.parent().unwrap_or(rule_dir))
            .output()
            .await
            .with_context(|| format!("failed to build mock gateway for {rule_id}"))?;
        let mut build_log = Vec::new();
        build_log.extend_from_slice(&build_output.stdout);
        build_log.extend_from_slice(&build_output.stderr);
        fs::write(&build_log_path, build_log)?;
        if !build_output.status.success() {
            anyhow::bail!("mock gateway build failed for {rule_id}");
        }

        let binary_path = workspace_root
            .join("target")
            .join("debug")
            .join("raiko-mock-gateway");
        let runtime_log = File::create(&runtime_log_path)
            .with_context(|| format!("failed to create runtime log for {rule_id}"))?;
        let runtime_log_err = runtime_log
            .try_clone()
            .with_context(|| format!("failed to clone runtime log for {rule_id}"))?;

        let mut command = Command::new(binary_path);
        command
            .arg(&bind)
            .env("MOCK_RULE_ID", rule_id)
            .env("MOCK_RULES_ROOT", rule_dir.parent().unwrap_or(rule_dir))
            .stdin(Stdio::null())
            .stdout(Stdio::from(runtime_log))
            .stderr(Stdio::from(runtime_log_err));

        command
            .spawn()
            .with_context(|| format!("failed to spawn mock gateway for {rule_id}"))?;

        let base_url = format!("http://{bind}");
        wait_for_health(
            &base_url,
            self.health_timeout,
            self.health_poll_interval,
            &runtime_log_path,
        )
        .await?;

        Ok(base_url)
    }
}

fn allocate_local_port(rule_id: &str) -> u16 {
    let rule_offset = rule_id
        .trim_start_matches("ticket-")
        .parse::<u16>()
        .unwrap_or(1);
    let time_offset = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_millis() % 1000) as u16)
        .unwrap_or(0);
    20000 + time_offset + rule_offset
}

fn resolve_gateway_bind(rule_id: &str, configured_port: Option<&str>) -> anyhow::Result<String> {
    if let Some(port_text) = configured_port {
        let port = port_text
            .parse::<u16>()
            .with_context(|| format!("invalid MOCK_GATEWAY_PORT: {port_text}"))?;
        return Ok(format!("127.0.0.1:{port}"));
    }

    Ok(format!("127.0.0.1:{}", allocate_local_port(rule_id)))
}

async fn wait_for_health(
    base_url: &str,
    timeout: Duration,
    poll_interval: Duration,
    runtime_log_path: &Path,
) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let start = tokio::time::Instant::now();
    let health_url = format!("{base_url}/health");

    loop {
        if start.elapsed() > timeout {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(runtime_log_path)?;
            writeln!(file, "health check timed out for {health_url}")?;
            anyhow::bail!("mock gateway health check timed out");
        }

        match client.get(&health_url).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(_) | Err(_) => tokio::time::sleep(poll_interval).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_gateway_bind;

    #[test]
    fn resolve_gateway_bind_uses_configured_port_when_present() {
        let bind = resolve_gateway_bind("ticket-7", Some("24567")).unwrap();
        assert_eq!(bind, "127.0.0.1:24567");
    }

    #[test]
    fn resolve_gateway_bind_rejects_invalid_port() {
        let error = resolve_gateway_bind("ticket-7", Some("not-a-port"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid MOCK_GATEWAY_PORT"));
    }
}
