#![allow(incomplete_features)]
use clap::Parser;
use raiko_host::{interfaces::HostResult, server::serve, Opts};
use raiko_lib::consts::SupportedChainSpecs;
use raiko_reqpool::RedisPoolConfig;
use std::path::PathBuf;
use tracing::{debug, info};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{Builder, Rotation},
};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> HostResult<()> {
    dotenv::dotenv().ok();
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .init();
    let opts = parse_opts()?;
    let chain_specs = parse_chain_specs(&opts);

    // Start pool
    let pool = raiko_reqpool::RedisPool::open(RedisPoolConfig {
        redis_url: opts.redis_url.clone(),
        redis_ttl: opts.redis_ttl,
    })
    .map_err(|e| anyhow::anyhow!(e))?;

    // Start Actor
    let (controller, pause_tx) =
        raiko_reqactor::Actor::start(pool.clone(), chain_specs.clone()).await;
    let gateway = raiko_reqactor::Gateway::new(
        pool,
        opts.proof_request_opt.clone(),
        chain_specs.clone(),
        controller,
        pause_tx,
    );

    let _guard = subscribe_log(&opts.log_path, &opts.log_level, opts.max_log);
    debug!("Start config:\n{:#?}", opts.proof_request_opt);
    debug!("Args:\n{:#?}", opts);
    info!("Supported chains: {:?}", chain_specs);

    let address = opts.address.as_str();
    let concurrency = opts.concurrency_limit;
    let jwt_secret = opts.jwt_secret.clone();
    serve(gateway, address, concurrency, jwt_secret).await?;
    Ok(())
}

fn parse_opts() -> HostResult<Opts> {
    // Read the command line arguments;
    let mut opts = Opts::parse();
    // Read env supported options.
    opts.merge_from_env();
    // Read the config file.
    opts.merge_from_file()?;

    Ok(opts)
}

fn parse_chain_specs(opts: &Opts) -> SupportedChainSpecs {
    if let Some(cs_path) = &opts.chain_spec_path {
        SupportedChainSpecs::merge_from_file(cs_path.clone()).expect("Failed to parse chain specs")
    } else {
        SupportedChainSpecs::default()
    }
}

fn subscribe_log(
    log_path: &Option<PathBuf>,
    log_level: &String,
    max_log: usize,
) -> Option<WorkerGuard> {
    let subscriber_builder = FmtSubscriber::builder()
        .with_env_filter(log_level)
        .with_test_writer();
    match log_path {
        Some(ref log_path) => {
            let file_appender = Builder::new()
                .rotation(Rotation::DAILY)
                .filename_prefix("raiko.log")
                .max_log_files(max_log)
                .build(log_path)
                .expect("initializing rolling file appender failed");
            let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
            let subscriber = subscriber_builder.json().with_writer(non_blocking).finish();
            tracing::subscriber::set_global_default(subscriber).unwrap();
            Some(guard)
        }
        None => {
            let subscriber = subscriber_builder.finish();
            tracing::subscriber::set_global_default(subscriber).unwrap();
            None
        }
    }
}
