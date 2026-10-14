//! In-process event bus feeding the `/api/events` SSE stream.
//!
//! Events here carry what the server needs to decide who may see them (section 7, Live
//! events); the server rewrites each one per connection before anything is sent.

use serde::Serialize;
use tokio::sync::broadcast;

use crate::models::{Download, ItemFacts, VpnStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryChange {
    Added,
    Updated,
    Removed,
}

impl LibraryChange {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Updated => "updated",
            Self::Removed => "removed",
        }
    }
}

/// Events published inside the server. The SSE event name is [`Event::name`].
#[derive(Debug, Clone)]
pub enum Event {
    Download(Download),
    /// An item was added, changed or removed. `before` is what decided who could see it
    /// before the change, when the change may have moved it (None: the same as now, or
    /// nothing for an added item). What it is now is read by the server.
    Library {
        r#type: LibraryChange,
        item_id: String,
        before: Option<ItemFacts>,
    },
    /// Many items changed library at once: a library made from an existing folder took
    /// them in (`from` None, `to` the library), or a deleted library let them go (`from`
    /// the library, `to` None, the items are then in a folder that is not a library).
    Regrouped {
        item_ids: Vec<String>,
        from: Option<String>,
        to: Option<String>,
    },
    /// A library was created, changed, reordered or deleted. Carries nothing; every
    /// stream checks whether its own answer to `GET /api/libraries` changed.
    Libraries,
    Vpn(VpnStatus),
    /// The number of waiting items changed; each stream counts what its caller sees.
    Inbox {
        count: i64,
    },
    Job {
        kind: String,
        state: String,
        item_id: Option<String>,
    },
    /// Server only, never sent: this person's role or access may have changed.
    AccessChanged {
        user_id: String,
    },
    /// Server only, never sent: this person was deleted; their streams close.
    UserRemoved {
        user_id: String,
    },
}

impl Event {
    /// SSE event name.
    pub fn name(&self) -> &'static str {
        match self {
            Event::Download(_) => "download",
            Event::Library { .. } | Event::Regrouped { .. } => "library",
            Event::Libraries => "libraries",
            Event::Vpn(_) => "vpn",
            Event::Inbox { .. } => "inbox",
            Event::Job { .. } => "job",
            Event::AccessChanged { .. } | Event::UserRemoved { .. } => "internal",
        }
    }

    /// SSE data payload (JSON) as a full owner would get it.
    pub fn data(&self) -> serde_json::Value {
        match self {
            Event::Download(d) => serde_json::to_value(d).unwrap_or_default(),
            Event::Library {
                r#type, item_id, ..
            } => serde_json::json!({"type": r#type, "item_id": item_id}),
            Event::Regrouped { .. } | Event::Libraries => serde_json::json!({}),
            Event::Vpn(status) => serde_json::to_value(status).unwrap_or_default(),
            Event::Inbox { count } => serde_json::json!({ "count": count }),
            Event::Job {
                kind,
                state,
                item_id,
            } => serde_json::json!({"kind": kind, "state": state, "item_id": item_id}),
            Event::AccessChanged { .. } | Event::UserRemoved { .. } => serde_json::Value::Null,
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
        let (tx, _) = broadcast::channel(2048);
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
            before: None,
        };
        assert_eq!(e.name(), "library");
        assert_eq!(
            e.data(),
            serde_json::json!({"type": "added", "item_id": "x"})
        );
        assert_eq!(Event::Libraries.name(), "libraries");
        assert_eq!(Event::Libraries.data(), serde_json::json!({}));
    }
}
