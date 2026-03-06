pub mod config;
pub mod router;
pub mod shasta;

pub use config::Config;
pub use router::{app, AppState};
pub use shasta::{
    backend_index, route_key_from_body, route_key_from_body_with_defaults, ShastaRouteDefaults,
    ShastaRouteKey,
};

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::{Request, StatusCode}};
    use serde_json::json;
    use tower::ServiceExt;

    fn shasta_body() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "l1_network": "ethereum",
            "network": "taiko",
            "proof_type": "native",
            "prover": "0x0000000000000000000000000000000000000000",
            "aggregate": false,
            "proposals": [
                {
                    "proposal_id": 101,
                    "l1_inclusion_block_number": 9001
                },
                {
                    "proposal_id": 102,
                    "l1_inclusion_block_number": 9002
                }
            ]
        }))
        .unwrap()
    }

    #[test]
    fn routing_derives_a_stable_shasta_route_key() {
        let route_key = route_key_from_body(&shasta_body()).unwrap();

        assert_eq!(
            route_key,
            ShastaRouteKey {
                l1_network: "ethereum".to_string(),
                network: "taiko".to_string(),
                proof_type: "native".to_string(),
                prover: "0x0000000000000000000000000000000000000000".to_string(),
                aggregate: false,
                proposal_id: vec![101, 102],
                l1_inclusion_block_number: vec![9001, 9002],
            }
        );
    }

    #[test]
    fn routing_hash_is_stable_for_identical_requests() {
        let route_key = route_key_from_body(&shasta_body()).unwrap();

        assert_eq!(backend_index(&route_key, 3), backend_index(&route_key, 3));
    }

    #[test]
    fn routing_hash_changes_when_key_fields_change() {
        let route_key_a = route_key_from_body(&shasta_body()).unwrap();
        let route_key_b = route_key_from_body(
            &serde_json::to_vec(&json!({
                "l1_network": "ethereum",
                "network": "taiko",
                "proof_type": "native",
                "prover": "0x0000000000000000000000000000000000000000",
                "aggregate": false,
                "proposals": [
                    {
                        "proposal_id": 201,
                        "l1_inclusion_block_number": 9001
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();

        assert_ne!(route_key_a, route_key_b);
        assert_ne!(backend_index(&route_key_a, 17), backend_index(&route_key_b, 17));
    }

    #[test]
    fn routing_accepts_requests_that_rely_on_backend_defaults() {
        let route_key = route_key_from_body(
            &serde_json::to_vec(&json!({
                "proof_type": "native",
                "proposals": [
                    {
                        "proposal_id": 101,
                        "l1_inclusion_block_number": 9001
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();

        assert_eq!(route_key.proof_type, "native");
        assert_eq!(route_key.network, "");
        assert_eq!(route_key.l1_network, "");
        assert_eq!(route_key.prover, "");
        assert!(!route_key.aggregate);
    }

    #[test]
    fn routing_uses_configured_defaults_when_fields_are_omitted() {
        let route_key = route_key_from_body_with_defaults(
            &serde_json::to_vec(&json!({
                "proposals": [
                    {
                        "proposal_id": 101,
                        "l1_inclusion_block_number": 9001
                    }
                ]
            }))
            .unwrap(),
            &ShastaRouteDefaults {
                l1_network: "ethereum".to_string(),
                network: "taiko".to_string(),
                proof_type: "native".to_string(),
                prover: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_string(),
                aggregate: false,
            },
        )
        .unwrap();

        assert_eq!(route_key.l1_network, "ethereum");
        assert_eq!(route_key.network, "taiko");
        assert_eq!(route_key.proof_type, "native");
        assert_eq!(route_key.prover, "0x70997970C51812dc3A010C7d01b50e0d17dc79C8");
        assert!(!route_key.aggregate);
    }

    #[tokio::test]
    async fn router_accepts_versioned_shasta_path() {
        let app = app(AppState::new(Config {
            bind: "127.0.0.1:8080".to_string(),
            backend_replicas: 1,
            backend_statefulset: "raiko".to_string(),
            backend_headless_service: "raiko-headless".to_string(),
            backend_service: "raiko-service".to_string(),
            backend_namespace: "default".to_string(),
            backend_port: 8080,
            default_network: "taiko".to_string(),
            default_l1_network: "ethereum".to_string(),
            default_proof_type: "native".to_string(),
            default_prover: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_string(),
            default_aggregate: false,
        }));

        let response = app
            .oneshot(
                Request::post("/v3/proof/batch/shasta")
                    .header("content-type", "application/json")
                    .body(Body::from(shasta_body()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn router_exposes_health_routes() {
        let app = app(AppState::new(Config {
            bind: "127.0.0.1:8080".to_string(),
            backend_replicas: 1,
            backend_statefulset: "raiko".to_string(),
            backend_headless_service: "raiko-headless".to_string(),
            backend_service: "raiko-service".to_string(),
            backend_namespace: "default".to_string(),
            backend_port: 8080,
            default_network: "taiko".to_string(),
            default_l1_network: "ethereum".to_string(),
            default_proof_type: "native".to_string(),
            default_prover: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_string(),
            default_aggregate: false,
        }));

        let response = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
