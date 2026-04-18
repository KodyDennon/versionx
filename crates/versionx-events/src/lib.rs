//! Typed event bus shared across Versionx subsystems.
//!
//! Every subsystem emits `Event`s through an [`EventSender`]. Subscribers
//! (tracing layer, daemon RPC streamer, state DB writer, MCP progress
//! notifier) read from an [`EventReceiver`] obtained via [`EventBus::subscribe`].
//!
//! The bus is built on `tokio::sync::broadcast` — lossy under slow-subscriber
//! conditions, which is what we want: never block the producer. Subscribers
//! that can't keep up skip events and receive [`tokio::sync::broadcast::error::RecvError::Lagged`].
//!
//! See `docs/spec/01-architecture-overview.md §7` for the event categories
//! and the broader observability story.

#![deny(unsafe_code)]

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

/// Default broadcast channel capacity. Chosen so that a busy sync op
/// (thousands of adapter.exec events) doesn't overflow in the first second.
pub const DEFAULT_CAPACITY: usize = 4096;

/// Severity of an emitted event. Mirrors `tracing::Level`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    /// Whether this event should be visible at typical operator verbosity.
    #[must_use]
    pub const fn is_operator_visible(self) -> bool {
        matches!(self, Self::Info | Self::Warn | Self::Error)
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        })
    }
}

/// A single structured event emitted on the bus.
///
/// The [`kind`](Self::kind) field uses dot-separated categories
/// (`adapter.exec.stdout`, `release.publish.complete`, `policy.violation`).
/// See `docs/spec/01-architecture-overview.md §7` for the canonical list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    /// Stable unique identifier (UUID v7, so it's lexicographically sortable
    /// by emission time).
    pub id: Uuid,
    /// Wall-clock time the event was emitted.
    pub timestamp: DateTime<Utc>,
    /// Dot-separated category (`adapter.exec.stdout` etc.).
    pub kind: String,
    /// Severity.
    pub level: Level,
    /// Short human-readable message. Always present. JSON output consumers
    /// should prefer [`data`](Self::data) for structured fields.
    pub message: String,
    /// Structured payload. Shape depends on [`kind`](Self::kind).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub data: serde_json::Value,
}

impl Event {
    /// Build an event with the current timestamp and a fresh UUID v7.
    #[must_use]
    pub fn new(kind: impl Into<String>, level: Level, message: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            kind: kind.into(),
            level,
            message: message.into(),
            data: serde_json::Value::Null,
        }
    }

    /// Attach a structured payload, consuming self. Panics if serialization
    /// fails, which it shouldn't for the types callers typically pass.
    #[must_use]
    pub fn with_data<T: Serialize>(mut self, data: &T) -> Self {
        self.data = serde_json::to_value(data).unwrap_or(serde_json::Value::Null);
        self
    }
}

/// Producer side of the event bus. Cheap to clone.
#[derive(Clone)]
pub struct EventSender {
    inner: Arc<broadcast::Sender<Event>>,
}

impl EventSender {
    /// Emit an event. Returns the number of active subscribers (zero if
    /// there are none — events still flow through `tracing`).
    pub fn emit(&self, event: Event) -> usize {
        // Mirror to `tracing` so operators with RUST_LOG / VERSIONX_LOG see
        // everything even without a live subscriber.
        match event.level {
            Level::Trace => tracing::trace!(kind = %event.kind, message = %event.message),
            Level::Debug => tracing::debug!(kind = %event.kind, message = %event.message),
            Level::Info => tracing::info!(kind = %event.kind, message = %event.message),
            Level::Warn => tracing::warn!(kind = %event.kind, message = %event.message),
            Level::Error => tracing::error!(kind = %event.kind, message = %event.message),
        }
        self.inner.send(event).unwrap_or(0)
    }

    /// Convenience: emit an info-level event.
    pub fn info(&self, kind: impl Into<String>, message: impl Into<String>) -> usize {
        self.emit(Event::new(kind, Level::Info, message))
    }

    /// Convenience: emit a warning.
    pub fn warn(&self, kind: impl Into<String>, message: impl Into<String>) -> usize {
        self.emit(Event::new(kind, Level::Warn, message))
    }

    /// Current subscriber count.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.inner.receiver_count()
    }
}

impl fmt::Debug for EventSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventSender").field("subscribers", &self.subscriber_count()).finish()
    }
}

/// Subscriber side of the bus. Each `EventReceiver` sees every event emitted
/// after it was created (or since it was last drained, up to channel capacity).
pub type EventReceiver = broadcast::Receiver<Event>;

/// The event bus handle. Owns the underlying broadcast channel.
#[derive(Clone, Debug)]
pub struct EventBus {
    sender: EventSender,
}

impl EventBus {
    /// Create a new bus with the default capacity.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Create a new bus with a custom broadcast capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { sender: EventSender { inner: Arc::new(tx) } }
    }

    /// Get a cheap clone of the sender side. Pass this into subsystems.
    #[must_use]
    pub fn sender(&self) -> EventSender {
        self.sender.clone()
    }

    /// Subscribe a new receiver. Only receives events emitted after this call.
    #[must_use]
    pub fn subscribe(&self) -> EventReceiver {
        self.sender.inner.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Structured error type used by subsystems when they need to emit an event
/// describing a failure. Crate-level errors use `thiserror` directly; this
/// one mirrors [`Event`] closely so it can be serialized alongside.
#[derive(Debug, thiserror::Error)]
#[error("versionx-events error: {0}")]
pub struct EventError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribe_then_emit_delivers() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.sender().info("test.kind", "hello");
        let evt = rx.recv().await.expect("event");
        assert_eq!(evt.kind, "test.kind");
        assert_eq!(evt.message, "hello");
        assert_eq!(evt.level, Level::Info);
    }

    #[tokio::test]
    async fn events_emitted_before_subscribe_are_missed() {
        let bus = EventBus::new();
        bus.sender().info("early", "lost");
        let mut rx = bus.subscribe();
        bus.sender().info("late", "kept");
        let evt = rx.recv().await.unwrap();
        assert_eq!(evt.kind, "late");
    }

    #[tokio::test]
    async fn multiple_subscribers_each_see_every_event() {
        let bus = EventBus::new();
        let mut r1 = bus.subscribe();
        let mut r2 = bus.subscribe();
        bus.sender().warn("kind", "msg");
        let e1 = r1.recv().await.unwrap();
        let e2 = r2.recv().await.unwrap();
        assert_eq!(e1.id, e2.id);
        assert_eq!(e1.level, Level::Warn);
    }

    #[test]
    fn event_serializes_to_json_round_trip() {
        let original = Event::new("adapter.exec", Level::Info, "npm install")
            .with_data(&serde_json::json!({"cwd": "/tmp", "jobs": 4}));
        let json = serde_json::to_string(&original).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, original.kind);
        assert_eq!(back.data["cwd"], "/tmp");
    }

    #[test]
    fn emit_with_zero_subscribers_does_not_panic() {
        let bus = EventBus::new();
        assert_eq!(bus.sender().info("noone.listening", "ok"), 0);
    }
}
