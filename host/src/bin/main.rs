#![allow(incomplete_features)]
use chrono::Utc;
use raiko_host::{
    interfaces::HostResult, parse_ballot, parse_chain_specs, parse_opts, server::auth::ApiKeyStore,
    server::serve,
};
use raiko_reqpool::RedisPoolConfig;
use std::fs::create_dir_all;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> HostResult<()> {
    dotenv::dotenv().ok();

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
        opts.queue_limit,
    )
    .await;

    let _guard = subscribe_log(&opts.log_path, &opts.log_level, opts.max_log);
    debug!("Start config:\n{:#?}", default_request_config);
    debug!("Args:\n{:#?}", opts);
    info!("Supported chains: {:?}", chain_specs);

    let address = opts.address.as_str();
    let concurrency = opts.concurrency_limit;
    let jwt_secret = opts.jwt_secret.clone();
    let api_key_store = Some(Arc::new(ApiKeyStore::new(opts.api_keys)));
    serve(actor, address, concurrency, jwt_secret, api_key_store).await?;
    Ok(())
}

use tracing_subscriber::Layer;

pub fn subscribe_log(
    log_path: &Option<PathBuf>,
    log_level: &str,
    _max_log: usize,
) -> Option<WorkerGuard> {
    // back compatible with env_logger
    // tracing_log::LogTracer::init().expect("log->tracing bridge init failed");

    // Build main filter
    let env_filter = EnvFilter::try_new(log_level).unwrap_or_else(|_| EnvFilter::new(log_level));

    // stdout for info/debug/everything
    let stdout_layer = fmt::layer()
        .with_writer(std::io::stdout) // output to console
        .with_ansi(false)
        .with_filter(env_filter);

    // access log for billing usage
    if let Some(dir) = log_path {
        // ensure directory exists
        if let Err(e) = create_dir_all(dir) {
            eprintln!("Failed to create log dir: {e}");
            return None;
        }

        // get current year and month
        let now = Utc::now();
        let filename = format!("billing-{}.log", now.format("%Y-%m"));
        let file_path = dir.join(filename);

        // open file + construct non-blocking writer

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .unwrap_or_else(|e| panic!("Failed to open log file {:?}: {}", file_path, e));
        let (non_blocking, guard) = tracing_appender::non_blocking(file);

        // build billing file log layer
        let file_layer = fmt::layer()
            .with_writer(non_blocking)
            .with_ansi(false)
            .with_filter(EnvFilter::new(format!("billing={}", log_level)));

        tracing_subscriber::registry()
            .with(stdout_layer)
            .with(file_layer)
            .try_init()
            .unwrap_or_else(|e| {
                eprintln!("Failed to initialize tracing subscriber: {e}");
                std::process::exit(1);
            });

        Some(guard)
    } else {
        // only stdout
        tracing_subscriber::registry()
            .with(stdout_layer)
            .try_init()
            .unwrap_or_else(|e| {
                eprintln!("Failed to initialize tracing subscriber: {e}");
                std::process::exit(1);
            });
        None
    }
}
