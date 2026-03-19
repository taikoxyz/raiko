use std::{
    env,
    fs::{self, File},
    io::Write,
    net::UdpSocket,
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
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
    public_base_url: Option<String>,
    detected_public_host: Option<String>,
    active_child: Arc<Mutex<Option<tokio::process::Child>>>,
}

impl Default for LocalCargoRunner {
    fn default() -> Self {
        Self::new(None)
    }
}

impl LocalCargoRunner {
    pub fn new(public_base_url: Option<String>) -> Self {
        Self {
            health_timeout: Duration::from_secs(10),
            health_poll_interval: Duration::from_millis(200),
            public_base_url,
            detected_public_host: None,
            active_child: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(test)]
    fn with_detected_public_host(mut self, detected_public_host: Option<&str>) -> Self {
        self.detected_public_host = detected_public_host.map(ToString::to_string);
        self
    }

    async fn stop_active_gateway(&self) -> anyhow::Result<()> {
        let existing = self
            .active_child
            .lock()
            .expect("active gateway store poisoned")
            .take();
        if let Some(mut child) = existing {
            if child
                .try_wait()
                .context("failed to inspect existing mock gateway process")?
                .is_none()
            {
                child
                    .kill()
                    .await
                    .context("failed to stop existing mock gateway process")?;
            }
        }
        Ok(())
    }

    fn store_active_gateway(&self, child: tokio::process::Child) {
        *self
            .active_child
            .lock()
            .expect("active gateway store poisoned") = Some(child);
    }

    #[cfg(test)]
    fn set_active_child_for_tests(&self, child: Option<tokio::process::Child>) {
        *self
            .active_child
            .lock()
            .expect("active gateway store poisoned") = child;
    }
}

#[async_trait]
impl GatewayRunner for LocalCargoRunner {
    async fn run(&self, rule_id: &str, rule_dir: &Path) -> anyhow::Result<String> {
        let bind = resolve_gateway_bind(rule_id, env::var("MOCK_GATEWAY_PORT").ok().as_deref())?;
        let health_base_url = resolve_health_base_url(&bind)?;
        let public_base_url = resolve_public_base_url(
            &bind,
            self.public_base_url.as_deref(),
            self.detected_public_host
                .clone()
                .or_else(detect_public_host)
                .as_deref(),
        )?;
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
            .arg("--bind")
            .arg(&bind)
            .env("MOCK_RULE_ID", rule_id)
            .env("MOCK_RULES_ROOT", rule_dir.parent().unwrap_or(rule_dir))
            .stdin(Stdio::null())
            .stdout(Stdio::from(runtime_log))
            .stderr(Stdio::from(runtime_log_err));

        self.stop_active_gateway().await?;

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn mock gateway for {rule_id}"))?;

        let health_result = wait_for_health(
            &health_base_url,
            self.health_timeout,
            self.health_poll_interval,
            &runtime_log_path,
        )
        .await;
        if let Err(error) = health_result {
            let _ = child.kill().await;
            return Err(error);
        }

        self.store_active_gateway(child);

        Ok(public_base_url)
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
        return Ok(format!("0.0.0.0:{port}"));
    }

    Ok(format!("0.0.0.0:{}", allocate_local_port(rule_id)))
}

fn resolve_health_base_url(bind: &str) -> anyhow::Result<String> {
    let (_, port) = split_host_port(bind)?;
    Ok(format!("http://127.0.0.1:{port}"))
}

fn resolve_public_base_url(
    bind: &str,
    public_base_url: Option<&str>,
    detected_public_host: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(url) = public_base_url {
        return Ok(url.trim_end_matches('/').to_string());
    }

    let (_, port) = split_host_port(bind)?;
    let host = detected_public_host.unwrap_or("127.0.0.1");
    Ok(format!("http://{host}:{port}"))
}

fn split_host_port(bind: &str) -> anyhow::Result<(&str, u16)> {
    let (host, port_text) = bind
        .rsplit_once(':')
        .with_context(|| format!("invalid bind address: {bind}"))?;
    let port = port_text
        .parse::<u16>()
        .with_context(|| format!("invalid bind port: {bind}"))?;
    Ok((host, port))
}

fn detect_public_host() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:80").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    if ip.is_unspecified() || ip.is_loopback() {
        return None;
    }
    Some(ip.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioCliArgs {
    pub bind: String,
    pub public_base_url: Option<String>,
}

pub fn parse_studio_args<I, S>(args: I) -> anyhow::Result<StudioCliArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    let _ = args.next();

    let mut bind = None;
    let mut public_base_url = None;

    while let Some(arg) = args.next() {
        let arg = arg.as_ref();
        match arg {
            "--bind" => {
                bind = Some(
                    args.next()
                        .context("missing value for --bind")?
                        .as_ref()
                        .to_string(),
                );
            }
            "--public-base-url" => {
                public_base_url = Some(
                    args.next()
                        .context("missing value for --public-base-url")?
                        .as_ref()
                        .to_string(),
                );
            }
            value if !value.starts_with('-') && bind.is_none() => {
                bind = Some(value.to_string());
            }
            unexpected => anyhow::bail!("unknown argument: {unexpected}"),
        }
    }

    Ok(StudioCliArgs {
        bind: bind.unwrap_or_else(|| "0.0.0.0:9090".to_string()),
        public_base_url,
    })
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
    use super::{
        parse_studio_args, resolve_gateway_bind, resolve_public_base_url, LocalCargoRunner,
    };
    use std::process::Stdio;
    use tokio::process::Command;

    #[test]
    fn resolve_gateway_bind_uses_configured_port_when_present() {
        let bind = resolve_gateway_bind("ticket-7", Some("24567")).unwrap();
        assert_eq!(bind, "0.0.0.0:24567");
    }

    #[test]
    fn resolve_gateway_bind_defaults_to_public_host() {
        let bind = resolve_gateway_bind("ticket-7", None).unwrap();
        assert!(bind.starts_with("0.0.0.0:"));
    }

    #[test]
    fn resolve_gateway_bind_rejects_invalid_port() {
        let error = resolve_gateway_bind("ticket-7", Some("not-a-port"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid MOCK_GATEWAY_PORT"));
    }

    #[test]
    fn resolve_public_base_url_prefers_explicit_override() {
        let base_url = resolve_public_base_url("0.0.0.0:24567", Some("https://mock.example"), None)
            .unwrap();
        assert_eq!(base_url, "https://mock.example");
    }

    #[test]
    fn resolve_public_base_url_uses_detected_host_when_available() {
        let base_url =
            resolve_public_base_url("0.0.0.0:24567", None, Some("203.0.113.10")).unwrap();
        assert_eq!(base_url, "http://203.0.113.10:24567");
    }

    #[test]
    fn parse_studio_args_supports_bind_and_public_base_url_flags() {
        let args = parse_studio_args([
            "raiko-mock-studio",
            "--bind",
            "0.0.0.0:4010",
            "--public-base-url",
            "https://mock.example",
        ])
        .unwrap();
        assert_eq!(args.bind, "0.0.0.0:4010");
        assert_eq!(args.public_base_url.as_deref(), Some("https://mock.example"));
    }

    #[test]
    fn parse_studio_args_defaults_to_9090() {
        let args = parse_studio_args(["raiko-mock-studio"]).unwrap();
        assert_eq!(args.bind, "0.0.0.0:9090");
    }

    #[test]
    fn local_runner_uses_detected_host_for_public_url() {
        let runner = LocalCargoRunner::new(None).with_detected_public_host(Some("203.0.113.10"));
        let bind = resolve_gateway_bind("ticket-7", Some("24567")).unwrap();
        let base_url = resolve_public_base_url(
            &bind,
            runner.public_base_url.as_deref(),
            runner.detected_public_host.as_deref(),
        )
        .unwrap();
        assert_eq!(base_url, "http://203.0.113.10:24567");
    }

    #[tokio::test]
    async fn local_runner_replaces_existing_gateway_process() {
        let runner = LocalCargoRunner::default();
        let child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let old_pid = child.id().unwrap();

        runner.set_active_child_for_tests(Some(child));
        runner.stop_active_gateway().await.unwrap();

        let status = std::process::Command::new("kill")
            .arg("-0")
            .arg(old_pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(!status.success(), "old gateway process should be gone");
    }
}
