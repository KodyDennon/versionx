//! JSON-RPC 2.0 wire protocol — strict subset used between the CLI client
//! and the `versiond` server.
//!
//! ### Framing
//! Each message is a JSON object preceded by a 4-byte big-endian length
//! prefix. Max payload size is [`MAX_MESSAGE_BYTES`] (2 MiB) — anything
//! larger is rejected before allocation.
//!
//! ### Messages
//! - Requests have `method` + optional `params` + `id`.
//! - Responses have exactly one of `result` / `error` + matching `id`.
//! - Notifications have `method` + optional `params` + **no** `id`.
//!
//! We don't support batching — the 0.3 spec doesn't need it and it
//! complicates cancellation. If a use-case appears later, add it.

use serde::{Deserialize, Serialize};

pub const JSONRPC_VERSION: &str = "2.0";

/// Upper bound on a single frame. 2 MiB is generous for our payloads
/// (biggest realistic one is a full workspace `list` with ~1k components).
/// Anything bigger almost certainly means a runaway producer or malicious
/// peer; we disconnect rather than try to handle it.
pub const MAX_MESSAGE_BYTES: usize = 2 * 1024 * 1024;

/// Anything that can cross the wire. The `untagged` repr means serde
/// picks the matching variant from structural fields — `id` + `method`
/// ⇒ Request, `id` + `result|error` ⇒ Response, `method` without `id`
/// ⇒ Notification.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    Request(Request),
    Response(Response),
    Notification(Notification),
}

/// A JSON-RPC 2.0 request. `id` is always a string (UUID v7) in our
/// usage — the spec allows numbers/null, but string-only keeps dispatch
/// logic simple.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: JsonRpcVersion,
    pub id: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub params: serde_json::Value,
}

impl Request {
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            id: uuid::Uuid::now_v7().to_string(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: JsonRpcVersion,
    pub id: String,
    #[serde(flatten)]
    pub payload: ResponsePayload,
}

impl Response {
    pub fn success(id: impl Into<String>, result: serde_json::Value) -> Self {
        Self { jsonrpc: JsonRpcVersion, id: id.into(), payload: ResponsePayload::Result { result } }
    }

    pub fn error(id: impl Into<String>, error: ErrorObject) -> Self {
        Self { jsonrpc: JsonRpcVersion, id: id.into(), payload: ResponsePayload::Error { error } }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponsePayload {
    Result { result: serde_json::Value },
    Error { error: ErrorObject },
}

/// A server-sent push, no response expected. Used for progress events,
/// file-change notifications, and log lines streamed to a subscriber.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: JsonRpcVersion,
    pub method: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub params: serde_json::Value,
}

impl Notification {
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self { jsonrpc: JsonRpcVersion, method: method.into(), params }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorObject {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ErrorObject {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), data: None }
    }

    #[must_use]
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }

    // Standard JSON-RPC 2.0 error codes.
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    // Application-level error codes (reserved range -32000..=-32099).
    pub const SHUTTING_DOWN: i32 = -32001;
    pub const BUSY: i32 = -32002;
    pub const WORKSPACE_FAILED: i32 = -32003;
}

/// Marker type that always (de)serializes as the literal string `"2.0"`.
/// Keeps wire format correct without a `version: String` field that could
/// drift.
#[derive(Copy, Clone, Debug, Default)]
pub struct JsonRpcVersion;

impl Serialize for JsonRpcVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(JSONRPC_VERSION)
    }
}

impl<'de> Deserialize<'de> for JsonRpcVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s == JSONRPC_VERSION {
            Ok(Self)
        } else {
            Err(serde::de::Error::custom(format!(
                "expected jsonrpc version {JSONRPC_VERSION}, got {s}"
            )))
        }
    }
}

// --------- Method / notification method names ---------------------------

/// Canonical method names. Keep in one place so client + server stay in sync.
pub mod methods {
    pub const PING: &str = "ping";
    pub const SERVER_INFO: &str = "server.info";
    pub const SHUTDOWN: &str = "server.shutdown";
    pub const WORKSPACE_LIST: &str = "workspace.list";
    pub const WORKSPACE_STATUS: &str = "workspace.status";
    pub const WORKSPACE_GRAPH: &str = "workspace.graph";
    pub const BUMP_PROPOSE: &str = "bump.propose";
    pub const SUBSCRIBE: &str = "subscribe";
    pub const UNSUBSCRIBE: &str = "unsubscribe";
}

/// Notification channel names.
pub mod notifications {
    pub const WORKSPACE_CHANGED: &str = "workspace.changed";
    pub const PROGRESS: &str = "progress";
    pub const LOG: &str = "log";
    pub const SHUTTING_DOWN: &str = "server.shutting_down";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip() {
        let req = Request::new("ping", serde_json::json!({"x": 1}));
        let ser = serde_json::to_string(&req).unwrap();
        assert!(ser.contains("\"jsonrpc\":\"2.0\""));
        let back: Request = serde_json::from_str(&ser).unwrap();
        assert_eq!(back.method, "ping");
        assert_eq!(back.id, req.id);
    }

    #[test]
    fn success_response_has_result_not_error() {
        let r = Response::success("abc", serde_json::json!({"pong": true}));
        let ser = serde_json::to_string(&r).unwrap();
        assert!(ser.contains("\"result\""));
        assert!(!ser.contains("\"error\""));
    }

    #[test]
    fn error_response_has_error_not_result() {
        let r = Response::error("abc", ErrorObject::new(ErrorObject::METHOD_NOT_FOUND, "no such"));
        let ser = serde_json::to_string(&r).unwrap();
        assert!(ser.contains("\"error\""));
        assert!(!ser.contains("\"result\""));
    }

    #[test]
    fn notification_has_no_id() {
        let n = Notification::new("progress", serde_json::json!({"pct": 50}));
        let ser = serde_json::to_string(&n).unwrap();
        assert!(!ser.contains("\"id\""));
    }

    #[test]
    fn rejects_wrong_version() {
        let bad = r#"{"jsonrpc":"1.0","id":"1","method":"ping"}"#;
        let err = serde_json::from_str::<Request>(bad).unwrap_err();
        assert!(err.to_string().contains("jsonrpc version"));
    }

    #[test]
    fn untagged_message_routes_to_right_variant() {
        let req_json = r#"{"jsonrpc":"2.0","id":"1","method":"ping"}"#;
        let msg: Message = serde_json::from_str(req_json).unwrap();
        assert!(matches!(msg, Message::Request(_)));

        let notif_json = r#"{"jsonrpc":"2.0","method":"progress","params":{}}"#;
        let msg: Message = serde_json::from_str(notif_json).unwrap();
        assert!(matches!(msg, Message::Notification(_)));

        let resp_json = r#"{"jsonrpc":"2.0","id":"1","result":{"pong":true}}"#;
        let msg: Message = serde_json::from_str(resp_json).unwrap();
        assert!(matches!(msg, Message::Response(_)));
    }
}
