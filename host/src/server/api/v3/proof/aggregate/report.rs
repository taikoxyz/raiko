use axum::{extract::State, routing::get, Json, Router};
use raiko_tasks::AggregationTaskReport;
use utoipa::OpenApi;

use crate::interfaces::HostResult;
use raiko_reqactor::Gateway;

#[utoipa::path(post, path = "/proof/aggregate/report",
    tag = "Proving",
    responses (
        (status = 200, description = "Successfully retrieved a report of all aggregation tasks", body = AggregationTaskReport)
    )
)]
// #[debug_handler(state = Gateway)]
/// List all aggregation tasks.
///
/// Retrieve a list of aggregation task reports.
async fn report_handler<P: raiko_reqpool::Pool + 'static>(
    State(_gateway): State<Gateway<P>>,
) -> HostResult<Json<AggregationTaskReport>> {
    todo!()
}

#[derive(OpenApi)]
#[openapi(paths(report_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router<P: raiko_reqpool::Pool + 'static>() -> Router<Gateway<P>> {
    Router::new().route("/", get(report_handler::<P>))
}
