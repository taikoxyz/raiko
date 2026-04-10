use axum::Router;

mod batch;
mod list;
mod prune;
mod report;
mod shasta_handler;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    [
        shasta_handler::create_docs(),
        report::create_docs(),
        list::create_docs(),
        prune::create_docs(),
    ]
    .into_iter()
    .fold(utoipa::openapi::OpenApi::default(), |mut docs, curr| {
        docs.merge(curr);
        docs
    })
}

pub fn create_router() -> Router<raiko_reqactor::Actor> {
    Router::new()
        .nest("/batch/shasta", shasta_handler::create_router())
        .nest("/report", report::create_router())
        .nest("/list", list::create_router())
        .nest("/prune", prune::create_router())
}
