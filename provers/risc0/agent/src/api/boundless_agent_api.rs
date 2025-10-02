use utoipa::OpenApi;
use crate::{ElfType, DatabaseStats};
use super::types::{
    AsyncProofRequestData, AsyncProofResponse, DetailedStatusResponse,
    RequestListResponse, HealthResponse, DatabaseStatsResponse, DeleteAllResponse,
    ErrorResponse, ProofType, UploadImageResponse, ImageInfoResponse
};
use crate::api::handlers::{
    __path_health_check, __path_proof_handler, __path_get_async_proof_status,
    __path_list_async_requests, __path_get_database_stats, __path_delete_all_requests,
    __path_upload_image_handler, __path_image_info_handler
};
use crate::image_manager::ImageDetails;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Boundless Agent API",
        version = "1.0.0",
        description = r#"
REST API for Boundless Agent - Zero-knowledge proof generation via Boundless market

The Boundless Agent is a web service that acts as an intermediary between the Raiko server and the Boundless market for zero-knowledge proof generation. It provides a REST API for submitting proof requests, monitoring their progress, and retrieving completed proofs.

## Architecture
```
Raiko Server → Boundless Agent → Boundless Market
```

## Key Concepts
- **Asynchronous Processing**: All proof requests are processed asynchronously
- **Request Lifecycle**: Requests go through multiple states: preparing → submitted → in_progress → completed/failed
- **Proof Types**: Supports batch proofs, aggregation proofs, and ELF update proofs (coming soon)
- **Market Integration**: Automatically handles Boundless market submission, pricing, and prover assignment
        "#,
        contact(
            name = "Boundless Agent Support",
            url = "https://github.com/taikoxyz/raiko",
            email = ""
        ),
        license(
            name = "MIT",
            url = "https://github.com/taikoxyz/raiko/blob/main/LICENSE"
        )
    ),
    servers(
        (url = "http://localhost:9999", description = "Local development server"),
        (url = "{protocol}://{host}:{port}", description = "Configurable server", 
            variables(
                ("protocol" = (default = "http", enum_values("http", "https"))),
                ("host" = (default = "localhost")),
                ("port" = (default = "9999"))
            )
        )
    ),
    paths(
        health_check,
        proof_handler,
        get_async_proof_status,
        list_async_requests,
        get_database_stats,
        delete_all_requests,
        upload_image_handler,
        image_info_handler,
    ),
    components(schemas(
        AsyncProofRequestData,
        AsyncProofResponse,
        ProofType,
        ElfType,
        DetailedStatusResponse,
        RequestListResponse,
        HealthResponse,
        DatabaseStatsResponse,
        DatabaseStats,
        DeleteAllResponse,
        ErrorResponse,
        UploadImageResponse,
        ImageInfoResponse,
        ImageDetails,
    )),
    tags(
        (name = "Health", description = "Service health and status endpoints"),
        (name = "Proof", description = "Proof generation and submission endpoints"),
        (name = "Status", description = "Request status monitoring and tracking endpoints"),
        (name = "Maintenance", description = "Database and system maintenance endpoints"),
        (name = "Image Management", description = "ELF image upload and management endpoints")
    ),
    external_docs(
        url = "https://github.com/taikoxyz/raiko/docs/boundless_agent_api.md",
        description = "Detailed API Documentation and Integration Guide"
    )
)]
/// Boundless Agent OpenAPI Documentation
pub struct BoundlessAgentApiDoc;

/// Generate OpenAPI specification
pub fn create_docs() -> utoipa::openapi::OpenApi {
    BoundlessAgentApiDoc::openapi()
}