// use std::collections::HashMap;

// use chrono::Utc;

// use crate::{
//     request::{RequestEntity, RequestKey, Status, StatusWithContext},
//     traits::{Pool, PoolWithTrace},
// };

// #[derive(Debug, Clone)]
// pub struct MemoryPool {
//     /// The live requests in the pool
//     pending: HashMap<RequestKey, (RequestEntity, StatusWithContext)>,
//     /// The trace of requests
//     trace: Vec<(RequestKey, RequestEntity, StatusWithContext)>,
// }

// impl Pool for MemoryPool {
//     type Config = ();

//     fn new(_config: Self::Config) -> Self {
//         Self {
//             lives: HashMap::new(),
//             trace: Vec::new(),
//         }
//     }

//     fn add(&mut self, request_key: RequestKey, request_entity: RequestEntity) {
//         let status = StatusWithContext::new(Status::Registered, Utc::now());

//         let old = self.lives.insert(
//             request_key.clone(),
//             (request_entity.clone(), status.clone()),
//         );

//         if let Some((_, old_status)) = old {
//             tracing::error!(
//                 "MemoryPool.add: request key already exists, {request_key:?}, old status: {old_status:?}"
//             );
//         } else {
//             tracing::info!("MemoryPool.add, {request_key:?}, status: {status:?}");
//         }

//         self.trace.push((request_key, request_entity, status));
//     }

//     fn remove(&mut self, request_key: &RequestKey) {
//         match self.lives.remove(request_key) {
//             Some((_, status)) => {
//                 tracing::info!("MemoryPool.remove, {request_key:?}, status: {status:?}");
//             }
//             None => {
//                 tracing::error!("MemoryPool.remove: request key not found, {request_key:?}");
//             }
//         }
//     }

//     fn get(&self, request_key: &RequestKey) -> Option<(RequestEntity, StatusWithContext)> {
//         self.lives.get(request_key).cloned()
//     }

//     fn get_status(&self, request_key: &RequestKey) -> Option<StatusWithContext> {
//         self.lives
//             .get(request_key)
//             .map(|(_, status)| status.clone())
//     }

//     fn update_status(&mut self, request_key: &RequestKey, status: StatusWithContext) {
//         match self.lives.remove(request_key) {
//             Some((entity, old_status)) => {
//                 tracing::info!(
//                     "MemoryPool.update_status, {request_key:?}, old status: {old_status:?}, new status: {status:?}"
//                 );
//                 self.lives
//                     .insert(request_key.clone(), (entity.clone(), status.clone()));
//                 self.trace.push((request_key.clone(), entity, status));
//             }
//             None => {
//                 tracing::error!(
//                     "MemoryPool.update_status: request key not found, discard it, {request_key:?}"
//                 );
//             }
//         }
//     }
// }

// impl PoolWithTrace for MemoryPool {
//     fn get_all_live(&self) -> Vec<(RequestKey, RequestEntity, StatusWithContext)> {
//         self.lives
//             .iter()
//             .map(|(k, v)| (k.clone(), v.0.clone(), v.1.clone()))
//             .collect()
//     }

//     fn get_all_trace(&self) -> Vec<(RequestKey, RequestEntity, StatusWithContext)> {
//         self.trace.clone()
//     }

//     fn trace(
//         &self,
//         request_key: &RequestKey,
//     ) -> (
//         Option<(RequestEntity, StatusWithContext)>,
//         Vec<(RequestKey, RequestEntity, StatusWithContext)>,
//     ) {
//         let live = self.lives.get(request_key).cloned();
//         let traces = self
//             .trace
//             .iter()
//             .filter(|(k, _, _)| k == request_key)
//             .cloned()
//             .collect();
//         (live, traces)
//     }
// }
