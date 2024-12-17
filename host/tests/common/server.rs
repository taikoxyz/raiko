use crate::common::Client;
use raiko_host::{server::serve, Opts, ProverState};
use rand::Rng;
use tokio_util::sync::CancellationToken;

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
}

impl TestServerBuilder {
    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    pub fn build(self) -> TestServerHandle {
        let port = self
            .port
            .unwrap_or(rand::thread_rng().gen_range(1024..65535));
        let address = format!("127.0.0.1:{port}");

        // TODO
        // opts.config_path
        // opts.chain_spec_path
        let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_string());
        let opts = Opts {
            address,
            log_level,

            concurrency_limit: 16,
            ..Default::default()
        };
        let state = ProverState::init_with_opts(opts).expect("Failed to initialize prover state");
        let token = CancellationToken::new();

        // Run the server in a separate thread with the ability to cancel it when our testing is done.
        let (state_, token_) = (state.clone(), token.clone());
        tokio::spawn(async move {
            println!("Starting server on port {}", port);
            tokio::select! {
                _ = token_.cancelled() => {
                    println!("Test done");
                }
                result = serve(state_) => {
                    match result {
                        Ok(()) => {
                            assert!(false, "Unexpected server shutdown");
                        }
                        Err(error) => {
                            assert!(false, "Server failed due to: {error:?}");
                        }
                    };
                }
            }
        });

        TestServerHandle {
            state,
            _token: token,
        }
    }
}

/// Handle for a test server.
///
/// Note that we don't need to cancel the server explicitly, as it will be cancelled when the token is dropped.
pub struct TestServerHandle {
    state: ProverState,
    _token: CancellationToken,
}

impl TestServerHandle {
    pub fn get_client(&self) -> Client {
        Client::new(format!("http://{}", self.state.opts.address))
    }
}
