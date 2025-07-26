use axum::Router;
use raiko_lib::prover::Proof;
use raiko_tasks::TaskStatus;
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};
use utoipa_swagger_ui::SwaggerUi;

use crate::server::api::v1::{self, GuestOutputDoc};
use raiko_reqactor::Actor;

// re-export v2 types
pub use crate::server::api::v2::CancelStatus;
pub use crate::server::api::v2::ProofResponse;
pub use crate::server::api::v2::PruneStatus;
pub use crate::server::api::v2::Status;

mod proof;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Raiko Proverd Server API",
        version = "3.0",
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

pub fn create_router() -> Router<Actor> {
    let docs = create_docs();

    Router::new()
        // Only add the concurrency limit to the proof route. We want to still be able to call
        // healthchecks and metrics to have insight into the system.
        .nest("/proof", proof::create_router())
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", docs.clone()))
        .merge(Scalar::with_url("/scalar", docs))
}
