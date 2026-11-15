//! Application port implementations for the WebSocket transport.

use std::sync::Arc;

use corvus_application::ports::{BrowserEvent, BrowserRegistry, BrowserSession};
use corvus_proto::{Envelope, ErrorBody};

use crate::{BrowserConnection, Inbound, Registry};

impl BrowserSession for BrowserConnection {
    fn id(&self) -> &str {
        BrowserConnection::id(self)
    }

    fn capabilities(&self) -> Vec<String> {
        BrowserConnection::capabilities(self).to_vec()
    }

    fn supports(&self, capability: &str) -> bool {
        BrowserConnection::supports(self, capability)
    }

    fn is_alive(&self) -> bool {
        BrowserConnection::is_alive(self)
    }

    fn send(&self, envelope: Envelope) -> Result<(), ErrorBody> {
        BrowserConnection::send(self, envelope).map_err(|error| error.error_body())
    }
}

impl BrowserRegistry for Registry {
    fn list(&self) -> Vec<Arc<dyn BrowserSession>> {
        Registry::list(self)
            .into_iter()
            .filter_map(|info| self.get(&info.id))
            .map(|connection| connection as Arc<dyn BrowserSession>)
            .collect()
    }

    fn get(&self, id: &str) -> Option<Arc<dyn BrowserSession>> {
        Registry::get(self, id).map(|connection| connection as Arc<dyn BrowserSession>)
    }
}

impl From<Inbound> for BrowserEvent {
    fn from(event: Inbound) -> Self {
        match event {
            Inbound::Message {
                browser, envelope, ..
            } => BrowserEvent::Message { browser, envelope },
            Inbound::Closed { browser, .. } => BrowserEvent::Closed { browser },
        }
    }
}
