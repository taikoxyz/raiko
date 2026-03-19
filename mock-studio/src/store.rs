use std::{
    sync::{Arc, Mutex},
};

use crate::models::TicketRecord;

#[derive(Clone, Default)]
pub struct TicketStore {
    inner: Arc<Mutex<Vec<TicketRecord>>>,
}

impl TicketStore {
    pub fn from_records(records: Vec<TicketRecord>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(records)),
        }
    }

    pub fn upsert(&self, ticket: TicketRecord) {
        let mut tickets = self.inner.lock().expect("ticket store poisoned");
        if let Some(existing) = tickets
            .iter_mut()
            .find(|existing| existing.ticket_id == ticket.ticket_id)
        {
            *existing = ticket;
        } else {
            tickets.push(ticket);
        }
    }

    pub fn get(&self, ticket_id: &str) -> Option<TicketRecord> {
        self.inner
            .lock()
            .expect("ticket store poisoned")
            .iter()
            .find(|ticket| ticket.ticket_id == ticket_id)
            .cloned()
    }

    pub fn list(&self) -> Vec<TicketRecord> {
        self.inner.lock().expect("ticket store poisoned").clone()
    }
}
