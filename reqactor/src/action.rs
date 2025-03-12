use crate::{RequestEntity, RequestKey};
use raiko_reqpool::impl_display_using_json_pretty;
use serde::{Deserialize, Serialize};

/// The action message sent from **external** to the actor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    Prove {
        request_key: RequestKey,
        request_entity: RequestEntity,
    },
    Cancel {
        request_key: RequestKey,
    },
}

impl Action {
    /// Get the request key of the action.
    pub fn request_key(&self) -> &RequestKey {
        match self {
            Action::Prove { request_key, .. } => request_key,
            Action::Cancel { request_key, .. } => request_key,
        }
    }

    /// Whether the action is high priority.
    ///
    /// Currently, only aggregation requests are considered high priority.
    pub fn is_high_priority(&self) -> bool {
        matches!(self, Action::Prove { request_key, .. } if matches!(request_key, RequestKey::Aggregation(_)))
    }
}

impl raiko_metrics::ToLabel for &Action {
    fn to_label(&self) -> &'static str {
        match self {
            Action::Prove { .. } => "prove",
            Action::Cancel { .. } => "cancel",
        }
    }
}

impl_display_using_json_pretty!(Action);
