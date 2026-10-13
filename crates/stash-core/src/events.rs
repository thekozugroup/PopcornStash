//! In-process event bus feeding the `/api/events` SSE stream.

use serde::Serialize;
use tokio::sync::broadcast;

use crate::models::{Download, VpnStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryChange {
    Added,
    Updated,
    Removed,
}

/// Events pushed to connected browsers. The SSE event name is the variant name in snake_case.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    Download(Download),
    Library {
        r#type: LibraryChange,
        item_id: String,
    },
    Vpn(VpnStatus),
    Inbox {
        count: i64,
    },
    Job {
        kind: String,
        state: String,
        item_id: Option<String>,
    },
}

impl Event {
    /// SSE event name.
    pub fn name(&self) -> &'static str {
        match self {
            Event::Download(_) => "download",
            Event::Library { .. } => "library",
            Event::Vpn(_) => "vpn",
            Event::Inbox { .. } => "inbox",
            Event::Job { .. } => "job",
        }
    }

    /// SSE data payload (JSON).
    pub fn data(&self) -> serde_json::Value {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(mut map)) => map.remove("data").unwrap_or_default(),
            _ => serde_json::Value::Null,
        }
    }
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(512);
        Self { tx }
    }

    /// Publish; silently dropped when nobody listens.
    pub fn publish(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_shape() {
        let e = Event::Library {
            r#type: LibraryChange::Added,
            item_id: "x".into(),
        };
        assert_eq!(e.name(), "library");
        assert_eq!(
            e.data(),
            serde_json::json!({"type": "added", "item_id": "x"})
        );
    }
}
