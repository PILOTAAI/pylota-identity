//! The JSON-RPC envelope and the MCP messages the server answers, as local `serde` types following
//! `schema.ts` of `2026-07-28` and `2025-11-25` ([ADR 0009]: `rmcp` compiles tokio into the Worker, so
//! it is only a native dev-dependency, used by the round-trip test that keeps these types honest).
//!
//! [ADR 0009]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/adr/0009-local-mcp-protocol-types.md

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The `"2.0"` of every JSON-RPC message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TwoPointZero;

impl Serialize for TwoPointZero {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str("2.0")
    }
}

impl<'de> Deserialize<'de> for TwoPointZero {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        if v == "2.0" {
            Ok(TwoPointZero)
        } else {
            Err(serde::de::Error::custom("jsonrpc must be \"2.0\""))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}

/// A request or a notification (no `id`), as received.
#[derive(Clone, Debug, Deserialize)]
pub struct Incoming {
    pub jsonrpc: TwoPointZero,
    #[serde(default)]
    pub id: Option<RequestId>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: TwoPointZero,
    pub id: RequestId,
    pub result: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub jsonrpc: TwoPointZero,
    /// `None` is serialised as `null`; [`JsonRpcError::without_id`] leaves the member out.
    pub id: Option<RequestId>,
    pub error: ErrorObject,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC and MCP error codes (MCP § 2.4).
pub mod code {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    pub const SERVER: i64 = -32000;
    pub const HEADER_MISMATCH: i64 = -32020;
    pub const UNSUPPORTED_VERSION: i64 = -32022;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Implementation {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListChanged {
    pub list_changed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub tools: ListChanged,
    pub prompts: ListChanged,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: Implementation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverResult {
    pub supported_versions: Vec<String>,
    pub capabilities: ServerCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    pub ttl_ms: u64,
    pub cache_scope: String,
    #[serde(rename = "_meta")]
    pub meta: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub read_only_hint: Option<bool>,
    pub destructive_hint: Option<bool>,
    pub idempotent_hint: Option<bool>,
    pub open_world_hint: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub title: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub annotations: ToolAnnotations,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    Text { text: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<Content>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    pub is_error: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prompt {
    pub name: String,
    pub title: String,
    pub description: String,
    pub arguments: Vec<PromptArgument>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PromptMessage {
    pub role: String,
    pub content: Content,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GetPromptResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,
}

impl JsonRpcError {
    pub fn new(id: Option<RequestId>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: TwoPointZero,
            id,
            error: ErrorObject {
                code,
                message: message.into(),
                data: None,
            },
        }
    }

    pub fn data(mut self, data: Value) -> Self {
        self.error.data = Some(data);
        self
    }

    /// The error with no `id` member at all (a refused `Origin`, MCP § 2.1).
    pub fn without_id(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.remove("id");
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_shapes() {
        let r: Incoming =
            serde_json::from_value(json!({ "jsonrpc": "2.0", "id": 7, "method": "ping" })).unwrap();
        assert_eq!(r.id, Some(RequestId::Number(7)));
        assert!(
            serde_json::from_value::<Incoming>(json!({ "jsonrpc": "1.0", "method": "ping" }))
                .is_err()
        );
        let e = JsonRpcError::new(None, code::SERVER, "Forbidden");
        assert_eq!(
            e.without_id(),
            json!({ "jsonrpc": "2.0", "error": { "code": -32000, "message": "Forbidden" } })
        );
        assert_eq!(serde_json::to_value(&e).unwrap()["id"], Value::Null);
        let t = CallToolResult {
            content: vec![Content::Text { text: "{}".into() }],
            structured_content: Some(json!({})),
            is_error: false,
        };
        assert_eq!(
            serde_json::to_value(t).unwrap(),
            json!({ "content": [{ "type": "text", "text": "{}" }], "structuredContent": {}, "isError": false })
        );
    }
}
