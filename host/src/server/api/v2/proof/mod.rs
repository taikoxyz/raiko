use axum::Router;
use utoipa::openapi;

use crate::ProverState;

mod get;
mod status;
mod submit;

pub fn create_docs() -> openapi::OpenApi {
    [status::create_docs(), submit::create_docs()]
        .into_iter()
        .fold(get::create_docs(), |mut doc, sub_doc| {
            doc.merge(sub_doc);
            doc
        })
}

pub fn create_router() -> Router<ProverState> {
    Router::new()
        .nest("", get::create_router())
        .nest("", status::create_router())
        .nest("", submit::create_router())
}
