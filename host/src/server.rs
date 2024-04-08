use std::{fs::File, path::PathBuf, str::FromStr};

use hyper::{
    body::{Buf, HttpBody},
    header::HeaderValue,
    service::{make_service_fn, service_fn},
    Body, Method, Request, Response, Server, StatusCode,
};
use prometheus::{Encoder, TextEncoder};
use raiko_lib::input::GuestInput;
use tower::ServiceBuilder;
use tracing::info;

use crate::{
    error::HostError,
    execution::execute,
    get_config,
    request::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, JsonRpcResponseError, *},
    Opt,
};

/// Starts the proverd json-rpc server.
/// Note: the server may not immediately listening after returning the
/// `JoinHandle`.
#[allow(clippy::too_many_arguments)]
pub fn serve(opt: Opt) -> tokio::task::JoinHandle<()> {
    let addr = opt
        .address
        .parse::<std::net::SocketAddr>()
        .expect("valid socket address");

    tokio::spawn(async move {
        let handler = if let Some(cache) = opt.cache {
            Handler::new_with_cache(cache)
        } else {
            Handler::new()
        };

        let service = service_fn(move |req| {
            let handler = handler.clone();
            handler.handle_request(req)
        });

        let service = ServiceBuilder::new()
            .concurrency_limit(opt.concurrency_limit)
            .service(service);

        let service = make_service_fn(|_| {
            let service = service.clone();
            async move { Ok::<_, hyper::Error>(service) }
        });

        let server = Server::bind(&addr).serve(service);
        info!("Listening on http://{}", addr);
        server.await.expect("server should be serving");
    })
}

/// sets default headers for CORS requests
fn set_headers(headers: &mut hyper::HeaderMap, extended: bool) {
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));

    if extended {
        headers.insert(
            "access-control-allow-methods",
            HeaderValue::from_static("post, get, options"),
        );
        headers.insert(
            "access-control-allow-headers",
            HeaderValue::from_static("origin, content-type, accept, x-requested-with"),
        );
        headers.insert("access-control-max-age", HeaderValue::from_static("300"));
    }
}

#[derive(Clone)]
struct Handler {
    cache_dir: Option<PathBuf>,
}

impl Handler {
    pub fn new() -> Self {
        Self { cache_dir: None }
    }

    pub fn new_with_cache(dir: PathBuf) -> Self {
        Self {
            cache_dir: Some(dir),
        }
    }

    pub fn get(&self, block_no: u64, network: &str) -> Option<GuestInput> {
        let mut input: Option<GuestInput> = None;
        self.cache_dir
            .as_ref()
            .map(|dir| dir.join(format!("input-{}-{}", network, block_no)))
            .map(|path| File::open(path).map(|file| input = bincode::deserialize_from(file).ok()));
        input
    }

    pub fn set(&self, block_no: u64, network: &str, input: GuestInput) -> super::error::Result<()> {
        if let Some(dir) = self.cache_dir.as_ref() {
            let path = dir.join(format!("input-{}-{}", network, block_no));
            let file = File::create(path).map_err(HostError::Io)?;
            bincode::serialize_into(file, &input).map_err(|e| HostError::Anyhow(e.into()))?;
        }
        Ok(())
    }

    async fn handle_request(mut self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        {
            // limits the request size
            const MAX_BODY_SIZE: u64 = 1 << 20;
            let response_content_length = match req.body().size_hint().upper() {
                Some(v) => v,
                None => MAX_BODY_SIZE + 1,
            };

            if response_content_length > MAX_BODY_SIZE {
                let mut resp = Response::new(Body::from("request too large"));
                *resp.status_mut() = StatusCode::BAD_REQUEST;
                return Ok(resp);
            }
        }

        match (req.method(), req.uri().path()) {
            (&Method::GET, "/health") => {
                // nothing to report yet - healthy by default
                let mut resp = Response::default();
                set_headers(resp.headers_mut(), false);
                Ok(resp)
            }

            // json-rpc
            (&Method::POST, "/") => {
                let body_bytes = hyper::body::aggregate(req.into_body())
                    .await
                    .unwrap()
                    .reader();
                let json_req: Result<JsonRpcRequest<Vec<serde_json::Value>>, serde_json::Error> =
                    serde_json::from_reader(body_bytes);

                if let Err(err) = json_req {
                    let payload = serde_json::to_vec(&JsonRpcResponseError {
                        jsonrpc: "2.0".to_string(),
                        id: 0.into(),
                        error: JsonRpcError {
                            // parser error
                            code: -32700,
                            message: err.to_string(),
                        },
                    })
                    .unwrap();
                    let mut resp = Response::new(Body::from(payload));
                    set_headers(resp.headers_mut(), false);
                    return Ok(resp);
                }

                let json_req = json_req.unwrap();
                let result = self
                    .handle_method(json_req.method.as_str(), &json_req.params)
                    .await;
                let payload = match result {
                    Err(err) => {
                        serde_json::to_vec(&JsonRpcResponseError {
                            jsonrpc: "2.0".to_string(),
                            id: json_req.id,
                            error: JsonRpcError {
                                // internal server error
                                code: -32000,
                                message: err.to_string(),
                            },
                        })
                    }
                    Ok(val) => serde_json::to_vec(&JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: json_req.id,
                        result: Some(val),
                    }),
                };
                let mut resp = Response::new(Body::from(payload.unwrap()));
                set_headers(resp.headers_mut(), false);
                Ok(resp)
            }

            // serve CORS headers
            (&Method::OPTIONS, "/") => {
                let mut resp = Response::default();
                set_headers(resp.headers_mut(), true);
                Ok(resp)
            }

            // serve metrics
            (&Method::GET, "/metrics") => {
                let encoder = TextEncoder::new();
                let mut buffer = vec![];
                let mf = prometheus::gather();
                encoder.encode(&mf, &mut buffer).unwrap();
                let resp = Response::builder()
                    .header(hyper::header::CONTENT_TYPE, encoder.format_type())
                    .body(Body::from(buffer))
                    .unwrap();
                Ok(resp)
            }

            // everything else
            _ => {
                let mut not_found = Response::default();
                *not_found.status_mut() = StatusCode::NOT_FOUND;
                Ok(not_found)
            }
        }
    }

    async fn handle_method(
        &mut self,
        method: &str,
        params: &[serde_json::Value],
    ) -> Result<serde_json::Value, HostError> {
        match method {
            // Generate a proof for a block
            "proof" => {
                // Get the request data sent through json-rpc
                let request: serde_json::Value =
                    params.first().expect("params must not be empty").to_owned();

                // Use it to find cached input if any  build the config
                let config = get_config(Some(request)).unwrap();
                let block_no = config["block_no"].as_u64().expect("block_no not provided");
                let network = config["network"].as_str().expect("network not provided");
                let cached_input = self.get(block_no, network);

                // Run the selected prover
                let proof_type =
                    ProofType::from_str(config["proof_type"].as_str().unwrap()).unwrap();
                let (input, proof) = match proof_type {
                    ProofType::Native => {
                        execute::<super::execution::NativeDriver>(&config, cached_input).await
                    }
                    #[cfg(feature = "sp1")]
                    ProofType::Sp1 => execute::<sp1_prover::Sp1Prover>(&config, cached_input).await,
                    #[cfg(feature = "risc0")]
                    ProofType::Risc0 => {
                        execute::<risc0_prover::Risc0Prover>(&config, cached_input).await
                    }
                    #[cfg(feature = "sgx")]
                    ProofType::Sgx => execute::<sgx_prover::SgxProver>(&config, cached_input).await,
                    _ => unimplemented!("Prover {:?} not enabled!", proof_type),
                }?;
                // Cache the input
                self.set(block_no, network, input)?;
                Ok(proof)
            }
            _ => todo!(),
        }
    }
}
