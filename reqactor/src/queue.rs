use std::collections::{HashSet, VecDeque};

use raiko_reqpool::{RequestEntity, RequestKey};

/// Queue of requests to be processed
#[derive(Debug)]
pub struct Queue {
    /// High priority pending requests
    high_pending: VecDeque<(RequestKey, RequestEntity)>,
    /// Low priority pending requests
    low_pending: VecDeque<(RequestKey, RequestEntity)>,
    /// Requests that are currently being worked on
    working_in_progress: HashSet<RequestKey>,
    /// Requests that have been pushed to the queue or are in-flight
    queued_keys: HashSet<RequestKey>,
}

impl Queue {
    pub fn new() -> Self {
        Self {
            high_pending: VecDeque::new(),
            low_pending: VecDeque::new(),
            working_in_progress: HashSet::new(),
            queued_keys: HashSet::new(),
        }
    }

    pub fn contains(&self, request_key: &RequestKey) -> bool {
        self.queued_keys.contains(request_key)
    }

    pub fn add_pending(&mut self, request_key: RequestKey, request_entity: RequestEntity) {
        if self.queued_keys.insert(request_key.clone()) {
            let is_high_priority = matches!(request_key, RequestKey::Aggregation(_));
            if is_high_priority {
                self.high_pending.push_back((request_key, request_entity));
            } else {
                self.low_pending.push_back((request_key, request_entity));
            }
        }
    }

    /// Attempts to move a request from either the high or low priority queue into the in-flight set
    /// and starts processing it. High priority requests are processed first.
    pub fn try_next(&mut self) -> Option<(RequestKey, RequestEntity)> {
        // Try high priority queue first
        let (request_key, request_entity) = self
            .high_pending
            .pop_front()
            .or_else(|| self.low_pending.pop_front())?;

        self.working_in_progress.insert(request_key.clone());
        Some((request_key, request_entity))
    }

    pub fn complete(&mut self, request_key: RequestKey) {
        self.working_in_progress.remove(&request_key);
        self.queued_keys.remove(&request_key);
    }
}
