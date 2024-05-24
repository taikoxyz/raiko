#![allow(incomplete_features)]
use std::path::PathBuf;

use raiko_host::{interfaces::error::HostResult, server::serve, ProverState};
use tracing::info;
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{Builder, Rotation},
};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> HostResult<()> {
    env_logger::init();
    let state = ProverState::init()?;
    let _guard = subscribe_log(
        &state.opts.log_path,
        &state.opts.log_level,
        state.opts.max_log,
    );

    info!("Supported chains: {:?}", state.chain_specs);
    info!("Start config:\n{:#?}", state.opts.proof_request_opt);
    info!("Args:\n{:#?}", state.opts);

    serve(state).await?;
    Ok(())
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
