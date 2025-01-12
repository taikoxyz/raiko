use crate::request::{RequestEntity, RequestKey, StatusWithContext};

pub type PoolResult<T> = Result<T, String>;

/// Pool maintains the requests and their statuses
pub trait Pool: Send + Sync + Clone {
    /// Add a new request to the pool
    fn add(
        &mut self,
        request_key: RequestKey,
        request_entity: RequestEntity,
        status: StatusWithContext,
    ) -> PoolResult<()>;

    /// Remove a request from the pool, return the number of requests removed
    fn remove(&mut self, request_key: &RequestKey) -> PoolResult<usize>;

    /// Get a request and status from the pool
    fn get(
        &mut self,
        request_key: &RequestKey,
    ) -> PoolResult<Option<(RequestEntity, StatusWithContext)>>;

    /// Get the status of a request
    fn get_status(&mut self, request_key: &RequestKey) -> PoolResult<Option<StatusWithContext>>;

    /// Update the status of a request, return the old status
    fn update_status(
        &mut self,
        request_key: RequestKey,
        status: StatusWithContext,
    ) -> PoolResult<StatusWithContext>;
}

/// A pool extension that supports tracing
pub trait PoolWithTrace: Pool {
    /// Get all trace of requests, with the given max depth.
    fn trace_all(&self, max_depth: usize) -> Vec<(RequestKey, RequestEntity, StatusWithContext)>;

    /// Get the live entity and trace of a request
    fn trace(
        &self,
        request_key: &RequestKey,
    ) -> Vec<(RequestKey, RequestEntity, StatusWithContext)>;
}
