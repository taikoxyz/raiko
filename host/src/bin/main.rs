#![allow(incomplete_features)]
use chrono::Utc;
use raiko_host::{
    interfaces::HostResult,
    parse_ballot, parse_chain_specs, parse_opts,
    server::auth::ApiKeyStore,
    server::logging::{LogFormat, RequestLoggingConfig},
    server::serve,
};
use raiko_reqpool::RedisPoolConfig;
use std::fs::create_dir_all;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{Builder, Rotation},
};
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> HostResult<()> {
    dotenv::dotenv().ok();
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .init();
    let opts = parse_opts()?;
    let chain_specs = parse_chain_specs(&opts);
    let ballot = parse_ballot(&opts);
    let default_request_config = opts.proof_request_opt.clone();
    let max_proving_concurrency = opts.concurrency_limit;
    let pool = raiko_reqpool::Pool::open(RedisPoolConfig {
        redis_url: opts.redis_url.clone(),
        redis_ttl: opts.redis_ttl,
        enable_redis_pool: opts.enable_redis_pool,
    })
    .map_err(|e| anyhow::anyhow!(e))?;
    let actor = raiko_reqactor::start_actor(
        pool,
        ballot,
        chain_specs.clone(),
        default_request_config.clone(),
        max_proving_concurrency,
    )
    .await;

    let _guard = subscribe_log(&opts.log_path, &opts.log_level, opts.max_log);
    debug!("Start config:\n{:#?}", default_request_config);
    debug!("Args:\n{:#?}", opts);
    info!("Supported chains: {:?}", chain_specs);

    let address = opts.address.as_str();
    let concurrency = opts.concurrency_limit;
    let jwt_secret = opts.jwt_secret.clone();

    let request_logging_config = if opts.enable_request_logging {
        let log_format = match opts.request_log_format.as_str() {
            "csv" => LogFormat::Csv,
            _ => LogFormat::Json,
        };

        Some(RequestLoggingConfig {
            enabled: true,
            log_file_path: opts.request_log_path.clone(),
            log_format,
            retention_days: 30,
            include_headers: vec!["user-agent".to_string()],
            exclude_paths: vec!["/health".to_string(), "/metrics".to_string()],
        })
    } else {
        None
    };

    let api_key_store = Some(Arc::new(ApiKeyStore::new(opts.api_keys)));
    serve(
        actor,
        address,
        concurrency,
        jwt_secret,
        request_logging_config,
        api_key_store,
    )
    .await?;
    Ok(())
}

use tracing_subscriber::Layer;

pub fn subscribe_log(
    log_path: &Option<PathBuf>,
    log_level: &str,
    max_log: usize,
) -> Option<WorkerGuard> {
    // 构建主过滤器
    let env_filter = EnvFilter::try_new(log_level).unwrap_or_else(|_| EnvFilter::new("info"));

    // stdout for info/debug/everything
    let stdout_layer = fmt::layer()
        .with_writer(std::io::stdout) // 输出到控制台
        .with_ansi(true)
        .with_filter(env_filter);

    // access log for billing usage
    if let Some(dir) = log_path {
        // 确保目录存在
        if let Err(e) = create_dir_all(dir) {
            eprintln!("Failed to create log dir: {e}");
            return None;
        }

        // 获取当前年月
        let now = Utc::now();
        let filename = format!("billing-{}.log", now.format("%Y-%m"));
        let file_path = dir.join(filename);

        // 打开文件 + 构造 non-blocking writer
        let file = File::create(&file_path)
            .unwrap_or_else(|e| panic!("Failed to create log file {:?}: {}", file_path, e));
        let (non_blocking, guard) = tracing_appender::non_blocking(file);

        // 构建 JSON layer
        let file_layer = fmt::layer()
            .json()
            .with_writer(non_blocking)
            .with_ansi(false)
            .with_filter(EnvFilter::new("billing=debug"));

        tracing_subscriber::registry()
            .with(stdout_layer)
            .with(file_layer)
            .init();

        Some(guard)
    } else {
        // 只有 stdout
        tracing_subscriber::registry().with(stdout_layer).init();
        None
    }
}
