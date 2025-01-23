use axum::{response::IntoResponse, Json};
use raiko_core::interfaces::ProofRequest;
use serde::{Deserialize, Serialize};

// Import the reusing interfaces and types
use super::Status;

#[derive(Debug, Deserialize, Serialize)]
pub struct RequestAndStatus {
    pub request: ProofRequest,
    pub status: Status,
}

impl IntoResponse for RequestAndStatus {
    fn into_response(self) -> axum::response::Response {
        Json(serde_json::to_value(self).unwrap()).into_response()
    }
}

pub struct RequestAndStatusVec(Vec<RequestAndStatus>);

impl From<Vec<RequestAndStatus>> for RequestAndStatusVec {
    fn from(value: Vec<RequestAndStatus>) -> Self {
        Self(value)
    }
}

impl IntoResponse for RequestAndStatusVec {
    fn into_response(self) -> axum::response::Response {
        Json(serde_json::to_value(self.0).unwrap()).into_response()
    }
}
