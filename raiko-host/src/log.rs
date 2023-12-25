use std::path::PathBuf;

pub fn init_tracing(
    max_log_days: usize,
    log_path: Option<PathBuf>,
    filename_prefix: &str,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    const DEFAULT_FILTER: &str = "info";
    // try to load filter from `RUST_LOG` or use reasonably verbose defaults
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| DEFAULT_FILTER.into());
    let subscriber_builder = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_test_writer();
    match log_path {
        Some(ref log_path) => {
            let file_appender = tracing_appender::rolling::Builder::new()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix(filename_prefix)
                .max_log_files(max_log_days)
                .build(log_path)
                .expect("initializing rolling file appender failed");
            let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
            let subscriber = subscriber_builder.json().with_writer(non_blocking).finish();
            tracing::subscriber::set_global_default(subscriber).unwrap();
            Some(_guard)
        }
        None => {
            let subscriber = subscriber_builder.finish();
            tracing::subscriber::set_global_default(subscriber).unwrap();
            None
        }
    }
}
