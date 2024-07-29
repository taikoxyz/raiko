use axum::{response::IntoResponse, Json, Router};
use raiko_lib::prover::Proof;
use raiko_tasks::TaskStatus;
use serde::Serialize;
use utoipa::{OpenApi, ToSchema};
use utoipa_scalar::{Scalar, Servable};
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    server::api::v1::{self, GuestOutputDoc},
    ProverState,
};

mod proof;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Raiko Proverd Server API",
        version = "2.0",
        description = "Raiko Proverd Server API",
        contact(
            name = "API Support",
            url = "https://community.taiko.xyz",
            email = "info@taiko.xyz",
        ),
        license(
            name = "MIT",
            url = "https://github.com/taikoxyz/raiko/blob/main/LICENSE"
        ),
    ),
    components(
        schemas(
            raiko_core::interfaces::ProofRequestOpt,
            raiko_core::interfaces::ProverSpecificOpts,
            crate::interfaces::HostError,
            GuestOutputDoc,
            ProofResponse,
            TaskStatus,
            CancelStatus,
            PruneStatus,
            Proof,
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

#[derive(Debug, Serialize, ToSchema)]
#[serde(untagged)]
pub enum ProofResponse {
    Status {
        /// The status of the submitted task.
        status: TaskStatus,
    },
    Proof {
        /// The proof.
        proof: Proof,
    },
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Status {
    Ok { data: ProofResponse },
    Error { error: String, message: String },
}

impl From<Vec<u8>> for Status {
    fn from(proof: Vec<u8>) -> Self {
        Self::Ok {
            data: ProofResponse::Proof {
                proof: serde_json::from_slice(&proof).unwrap_or_default(),
            },
        }
    }
}

impl From<TaskStatus> for Status {
    fn from(status: TaskStatus) -> Self {
        Self::Ok {
            data: ProofResponse::Status { status },
        }
    }
}

impl IntoResponse for Status {
    fn into_response(self) -> axum::response::Response {
        Json(serde_json::to_value(self).unwrap()).into_response()
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "lowercase")]
/// Status of cancellation request.
/// Can be `ok` for a successful cancellation or `error` with message and error type for errors.
pub enum CancelStatus {
    /// Cancellation was successful.
    Ok,
    /// Cancellation failed.
    Error { error: String, message: String },
}

impl IntoResponse for CancelStatus {
    fn into_response(self) -> axum::response::Response {
        Json(serde_json::to_value(self).unwrap()).into_response()
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "lowercase")]
/// Status of prune request.
/// Can be `ok` for a successful prune or `error` with message and error type for errors.
pub enum PruneStatus {
    /// Prune was successful.
    Ok,
    /// Prune failed.
    Error { error: String, message: String },
}

impl IntoResponse for PruneStatus {
    fn into_response(self) -> axum::response::Response {
        Json(serde_json::to_value(self).unwrap()).into_response()
    }
}

#[must_use]
pub fn create_docs() -> utoipa::openapi::OpenApi {
    [
        v1::health::create_docs(),
        v1::metrics::create_docs(),
        proof::create_docs(),
    ]
    .into_iter()
    .fold(Docs::openapi(), |mut doc, sub_doc| {
        doc.merge(sub_doc);
        doc
    })
}

pub fn create_router() -> Router<ProverState> {
    let docs = create_docs();

    Router::new()
        // Only add the concurrency limit to the proof route. We want to still be able to call
        // healthchecks and metrics to have insight into the system.
        .nest("/proof", proof::create_router())
        .nest("/health", v1::health::create_router())
        .nest("/metrics", v1::metrics::create_router())
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", docs.clone()))
        .merge(Scalar::with_url("/scalar", docs))
}
