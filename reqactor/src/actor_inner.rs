use raiko_reqpool::Pool;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::{Action, RequestKey};

pub struct ActorInner {
    pool: Pool,

    high_queue: VecDeque<RequestKey>,
    low_queue: VecDeque<RequestKey>,
    in_flight: HashSet<RequestKey>,
    actions: HashMap<RequestKey, Action>,
}

impl ActorInner {
    pub fn new(pool: Pool, max_concurrency: usize) -> Self {
        Self {
            pool,
            high_queue: VecDeque::new(),
            low_queue: VecDeque::new(),
            in_flight: HashSet::new(),
            actions: HashMap::new(),
        }
    }

    pub fn contains(&self, action: &Action) -> bool {
        let request_key = action.request_key();
        self.low_queue.contains(&request_key)
            || self.high_queue.contains(&request_key)
            || self.in_flight.contains(&request_key)
    }

    pub fn push(&mut self, action: Action) {
        let request_key = action.request_key();
        let is_high_priority = matches!(request_key, RequestKey::Aggregation(_));
        if is_high_priority {
            self.high_queue.push_back(request_key.clone());
        } else {
            self.low_queue.push_back(request_key.clone());
        }

        self.actions.insert(request_key.clone(), action);
    }

    pub fn pop(&mut self) -> Option<Action> {
        let action_opt = if let Some(request_key) = self.high_queue.pop_front() {
            self.actions.remove(&request_key)
        } else if let Some(request_key) = self.low_queue.pop_front() {
            self.actions.remove(&request_key)
        } else {
            None
        };

        if let Some(action) = &action_opt {
            self.in_flight.insert(action.request_key().clone());
        }

        action_opt
    }

    pub fn remove(&mut self, action: &Action) {
        let request_key = action.request_key();
        self.in_flight.remove(&request_key);
    }
}
