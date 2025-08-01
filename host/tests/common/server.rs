use crate::common::Client;
use raiko_ballot::Ballot;
use raiko_host::{
    parse_chain_specs,
    server::{auth::ApiKeyStore, serve},
    Opts,
};
use raiko_reqactor::start_actor;
use raiko_reqpool::memory_pool;
use rand::Rng;
use std::sync::Arc;

/// Builder for a test server.
///
/// This builder only supports setting a few parameters for testing.
///
/// Examples:
/// ```
/// let server = TestServerBuilder::default()
///     .port(8080)
///     .build();
/// ```
#[derive(Default, Debug)]
pub struct TestServerBuilder {
    port: Option<u16>,
    redis_url: Option<String>,
}

impl TestServerBuilder {
    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    pub async fn build(self) -> TestServerHandle {
        let port = self
            .port
            .unwrap_or(rand::thread_rng().gen_range(1024..65535));
        let redis_url = self.redis_url.unwrap_or(port.to_string());
        let address = format!("127.0.0.1:{port}");
        let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_string());

        let opts = Opts {
            address: address.clone(),
            log_level,
            concurrency_limit: 16,
            ..Default::default()
        };
        let chain_specs = parse_chain_specs(&opts);
        let default_request_config = opts.proof_request_opt.clone();
        let max_proving_concurrency = opts.concurrency_limit;

        let pool = memory_pool(redis_url);
        let ballot = Ballot::default();
        let actor = start_actor(
            pool,
            ballot,
            chain_specs.clone(),
            default_request_config.clone(),
            max_proving_concurrency,
            1000, // max_queue_size
        )
        .await;

        let address_clone = address.clone();
        tokio::spawn(async move {
            let _ = serve(
                actor,
                &address_clone,
                max_proving_concurrency,
                None,
                Some(Arc::new(ApiKeyStore::new("".to_string()))),
            )
            .await;
        });

        TestServerHandle { address }
    }
}

/// Handle for a test server.
///
/// Note that we don't need to cancel the server explicitly, as it will be cancelled when the token is dropped.
pub struct TestServerHandle {
    address: String,
}

impl TestServerHandle {
    pub fn get_client(&self) -> Client {
        Client::new(format!("http://{}", self.address))
    }
}
