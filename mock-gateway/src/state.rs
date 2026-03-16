use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use serde_json::{json, Value};

#[derive(Clone)]
pub struct AppState {
    call_count: Arc<AtomicU64>,
    seen_requests: Arc<Mutex<Vec<String>>>,
}

impl AppState {
    pub fn new_context(&self) -> MockContext {
        MockContext {
            state: self.clone(),
            call_index: self.call_count.fetch_add(1, Ordering::SeqCst) + 1,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            call_count: Arc::new(AtomicU64::new(0)),
            seen_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

pub struct MockContext {
    state: AppState,
    call_index: u64,
}

impl MockContext {
    pub fn call_index(&self) -> u64 {
        self.call_index
    }

    pub fn request_key(&self, body: &Value) -> String {
        json!({
            "aggregate": body.get("aggregate").and_then(Value::as_bool).unwrap_or(false),
            "proof_type": body.get("proof_type").and_then(Value::as_str).unwrap_or("native"),
            "proposal_ids": body
                .get("proposals")
                .and_then(Value::as_array)
                .map(|proposals| {
                    proposals
                        .iter()
                        .filter_map(|proposal| proposal.get("proposal_id").and_then(Value::as_u64))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .to_string()
    }

    pub fn has_seen_request(&self, body: &Value) -> bool {
        let key = self.request_key(body);
        self.state
            .seen_requests
            .lock()
            .expect("seen request store poisoned")
            .contains(&key)
    }

    pub fn mark_request_seen(&self, body: &Value) {
        let key = self.request_key(body);
        let mut seen = self
            .state
            .seen_requests
            .lock()
            .expect("seen request store poisoned");
        if !seen.contains(&key) {
            seen.push(key);
        }
    }
}
