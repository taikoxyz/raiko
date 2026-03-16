use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::models::TicketRecord;

#[derive(Clone, Default)]
pub struct TicketStore {
    inner: Arc<Mutex<HashMap<String, TicketRecord>>>,
}

impl TicketStore {
    pub fn upsert(&self, ticket: TicketRecord) {
        self.inner
            .lock()
            .expect("ticket store poisoned")
            .insert(ticket.ticket_id.clone(), ticket);
    }

    pub fn get(&self, ticket_id: &str) -> Option<TicketRecord> {
        self.inner
            .lock()
            .expect("ticket store poisoned")
            .get(ticket_id)
            .cloned()
    }
}
