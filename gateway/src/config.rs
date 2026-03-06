use clap::Parser;

#[derive(Debug, Clone, Parser)]
pub struct Config {
    #[arg(long, env = "SHASTA_GATEWAY_BIND", default_value = "0.0.0.0:8080")]
    pub bind: String,
    #[arg(long, env = "SHASTA_GATEWAY_BACKEND_REPLICAS")]
    pub backend_replicas: usize,
    #[arg(long, env = "SHASTA_GATEWAY_BACKEND_STATEFULSET", default_value = "raiko")]
    pub backend_statefulset: String,
    #[arg(
        long,
        env = "SHASTA_GATEWAY_BACKEND_HEADLESS_SERVICE",
        default_value = "raiko-headless"
    )]
    pub backend_headless_service: String,
    #[arg(
        long,
        env = "SHASTA_GATEWAY_BACKEND_SERVICE",
        default_value = "raiko-service"
    )]
    pub backend_service: String,
    #[arg(long, env = "SHASTA_GATEWAY_BACKEND_NAMESPACE", default_value = "default")]
    pub backend_namespace: String,
    #[arg(long, env = "SHASTA_GATEWAY_BACKEND_PORT", default_value_t = 8080)]
    pub backend_port: u16,
    #[arg(long, env = "SHASTA_GATEWAY_DEFAULT_NETWORK", default_value = "")]
    pub default_network: String,
    #[arg(long, env = "SHASTA_GATEWAY_DEFAULT_L1_NETWORK", default_value = "")]
    pub default_l1_network: String,
    #[arg(long, env = "SHASTA_GATEWAY_DEFAULT_PROOF_TYPE", default_value = "")]
    pub default_proof_type: String,
    #[arg(long, env = "SHASTA_GATEWAY_DEFAULT_PROVER", default_value = "")]
    pub default_prover: String,
    #[arg(long, env = "SHASTA_GATEWAY_DEFAULT_AGGREGATE", default_value_t = false)]
    pub default_aggregate: bool,
}

impl Config {
    pub fn backend_url(&self, index: usize) -> String {
        format!(
            "http://{}-{}.{}.{}.svc.cluster.local:{}",
            self.backend_statefulset,
            index,
            self.backend_headless_service,
            self.backend_namespace,
            self.backend_port,
        )
    }

    pub fn shared_backend_url(&self) -> String {
        format!(
            "http://{}.{}.svc.cluster.local:{}",
            self.backend_service, self.backend_namespace, self.backend_port,
        )
    }
}
