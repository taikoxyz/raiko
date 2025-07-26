use axum::{response::IntoResponse, Router};
use raiko_lib::input::GuestOutput;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower::ServiceBuilder;
use utoipa::{OpenApi, ToSchema};
use utoipa_scalar::{Scalar, Servable};
use utoipa_swagger_ui::SwaggerUi;

use crate::interfaces::HostError;
use raiko_reqactor::Actor;

pub mod health;
pub mod metrics;
pub mod proof;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Raiko Proverd Server API",
        version = "1.0",
        description = "Raiko Proverd Server API",
        contact(
            name = "API Support",
            url = "https://community.taiko.xyz",
            email = "info@taiko.xyz",
        ),
        license(
            name = "MIT",
            url = "https://github.com/taikoxyz/raiko/blob/taiko/unstable/LICENSE"
        ),
    ),
    components(
        schemas(
            raiko_core::interfaces::ProofRequestOpt,
            raiko_core::interfaces::ProverSpecificOpts,
            crate::interfaces::HostError,
            GuestOutputDoc,
            ProofResponse,
            Status,
        )
    ),
    tags(
        (name = "Proving", description = "Routes that handle proving requests"),
        (name = "Health", description = "Routes that report the server health status"),
        (name = "Metrics", description = "Routes that give detailed insight into the server")
    )
)]
/// The root API struct which is generated from the `OpenApi` derive macro.
pub struct Docs;

#[derive(Debug, Serialize, ToSchema, Deserialize)]
/// The response body of a proof request.
pub struct ProofResponse {
    #[schema(value_type = Option<GuestOutputDoc>)]
    /// The output of the prover.
    pub output: Option<GuestOutput>,
    /// The proof.
    pub proof: Option<String>,
    /// The quote.
    pub quote: Option<String>,
}

impl ProofResponse {
    pub fn to_response(&self) -> Value {
        serde_json::json!({
            "status": "ok",
            "data": self
        })
    }
}

impl IntoResponse for ProofResponse {
    fn into_response(self) -> axum::response::Response {
        axum::Json(self.to_response()).into_response()
    }
}

impl TryFrom<Value> for ProofResponse {
    type Error = HostError;

    fn try_from(proof: Value) -> Result<Self, Self::Error> {
        serde_json::from_value(proof).map_err(|err| HostError::Conversion(err.to_string()))
    }
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "lowercase")]
#[allow(dead_code)]
pub enum Status {
    Ok { data: ProofResponse },
    Error { error: String, message: String },
}

#[derive(Debug, Serialize, ToSchema)]
#[allow(dead_code)]
#[schema(example = json!({"header": [0, 0, 0, 0], "hash":"0x0...0"}))]
/// The output of the prover when the proof generation was successful.
pub struct GuestOutputDoc {
    /// Header bytes.
    header: Vec<u8>,
    /// Instance hash.
    hash: String,
}

#[must_use]
pub fn create_docs() -> utoipa::openapi::OpenApi {
    [
        health::create_docs(),
        metrics::create_docs(),
        proof::create_docs(),
    ]
    .into_iter()
    .fold(Docs::openapi(), |mut doc, sub_doc| {
        doc.merge(sub_doc);
        doc
    })
}

pub fn create_router(concurrency_limit: usize) -> Router<Actor> {
    let docs = create_docs();

    Router::new()
        // Only add the concurrency limit to the proof route. We want to still be able to call
        // healthchecks and metrics to have insight into the system.
        .nest(
            "/proof",
            proof::create_router()
                .layer(ServiceBuilder::new().concurrency_limit(concurrency_limit)),
        )
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", docs.clone()))
        .merge(Scalar::with_url("/scalar", docs))
}
