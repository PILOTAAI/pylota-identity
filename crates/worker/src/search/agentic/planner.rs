//! The planner's model calls ([Search § 11.4]) and its six read-only tools ([Search § 11.5]): the
//! request bodies, the timeout race, and the parse of a Chat Completions reply.
//!
//! [Search § 11.4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md
//! [Search § 11.5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use pylota_identity_platform::ai::{AiOptions, is_rate_limited};
use pylota_identity_platform::ratelimit::RlBucket;
use pylota_identity_platform::{Ai as _, Clock as _, Platform, RateLimiter as _};
use serde_json::{Value, json};

use crate::search::join;

/// The tool definitions, as the design lists them (`planner::tests::tools_match_design`).
pub const TOOLS: &str = r#"[
  { "type": "function", "function": {
      "name": "search",
      "description": "Search the mailbox. q uses the Pylota Identity query language (operators such as from:, ref:, after:, has:attachment, quoted phrases, OR, -). Returns hits with message IDs, snippets and facets.",
      "strict": true,
      "parameters": { "type": "object", "additionalProperties": false, "required": ["q"],
        "properties": {
          "q": { "type": "string", "minLength": 1, "maxLength": 500 },
          "mode": { "type": "string", "enum": ["keyword", "semantic", "hybrid"], "default": "hybrid" },
          "limit": { "type": "integer", "minimum": 1, "maximum": 20, "default": 8 },
          "group_by": { "type": "string", "enum": ["message", "thread"], "default": "message" } } } } },
  { "type": "function", "function": {
      "name": "read_thread",
      "description": "Read the messages of one thread you saw in a result, oldest first, quotes removed.",
      "strict": true,
      "parameters": { "type": "object", "additionalProperties": false, "required": ["thread_id"],
        "properties": {
          "thread_id": { "type": "string", "pattern": "^thr_[0-9A-HJKMNP-TV-Z]{26}$" },
          "max_messages": { "type": "integer", "minimum": 1, "maximum": 20, "default": 10 } } } } },
  { "type": "function", "function": {
      "name": "read_message",
      "description": "Read one message you saw in a result.",
      "strict": true,
      "parameters": { "type": "object", "additionalProperties": false, "required": ["message_id"],
        "properties": {
          "message_id": { "type": "string", "pattern": "^msg_[0-9A-HJKMNP-TV-Z]{26}$" },
          "include_quoted": { "type": "boolean", "default": false } } } } },
  { "type": "function", "function": {
      "name": "read_attachment_text",
      "description": "Read extracted text from pages of an attachment of a message you saw.",
      "strict": true,
      "parameters": { "type": "object", "additionalProperties": false, "required": ["message_id", "attachment_id"],
        "properties": {
          "message_id": { "type": "string", "pattern": "^msg_[0-9A-HJKMNP-TV-Z]{26}$" },
          "attachment_id": { "type": "string", "pattern": "^att_[0-9A-HJKMNP-TV-Z]{26}$" },
          "pages": { "type": "string", "pattern": "^[0-9]{1,3}(-[0-9]{1,3})?$", "default": "1-3" } } } } },
  { "type": "function", "function": {
      "name": "find_related",
      "description": "Find messages in other threads that are about the same thing as a message you saw.",
      "strict": true,
      "parameters": { "type": "object", "additionalProperties": false, "required": ["message_id"],
        "properties": {
          "message_id": { "type": "string", "pattern": "^msg_[0-9A-HJKMNP-TV-Z]{26}$" },
          "limit": { "type": "integer", "minimum": 1, "maximum": 10, "default": 5 } } } } },
  { "type": "function", "function": {
      "name": "contacts",
      "description": "Look up people and organisations this mailbox has exchanged mail with, by name, address or domain prefix.",
      "strict": true,
      "parameters": { "type": "object", "additionalProperties": false, "required": ["q"],
        "properties": {
          "q": { "type": "string", "minLength": 1, "maxLength": 100 },
          "limit": { "type": "integer", "minimum": 1, "maximum": 20, "default": 10 } } } } }
]"#;

pub fn tools() -> Value {
    serde_json::from_str(TOOLS).expect("the tool table parses")
}

/// One tool call the model asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The JSON-encoded arguments, as sent.
    pub arguments: String,
}

/// A model reply: its text and tool calls, and the assistant message to append to the conversation.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub message: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelError {
    /// `RL_AI` refused, or Workers AI answered that it is rate limited.
    RateLimited,
    /// An error, a timeout, or a reply that is not a chat completion.
    Failed,
}

/// The reply of a Chat Completions answer.
pub fn parse(out: &Value) -> Option<Reply> {
    let message = out["choices"].get(0)?.get("message")?.clone();
    let content = message["content"].as_str().map(str::to_owned);
    let tool_calls = message["tool_calls"]
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .map(|c| ToolCall {
                    id: c["id"].as_str().unwrap_or("").to_owned(),
                    name: c["function"]["name"].as_str().unwrap_or("").to_owned(),
                    arguments: match &c["function"]["arguments"] {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    },
                })
                .collect()
        })
        .unwrap_or_default();
    let mut assistant = json!({ "role": "assistant", "content": message["content"].clone() });
    if let Some(calls) = message.get("tool_calls").filter(|c| c.is_array()) {
        assistant["tool_calls"] = calls.clone();
    }
    Some(Reply {
        content,
        tool_calls,
        message: assistant,
    })
}

/// One model call: `RL_AI` asked first for the tenant, then `AI.run` raced against `timeout_ms`.
pub async fn call<P: Platform>(
    p: &P,
    tenant_id: &str,
    input: &Value,
    timeout_ms: u32,
) -> Result<Reply, ModelError> {
    if !p
        .limits()
        .allow(RlBucket::Ai, tenant_id)
        .await
        .unwrap_or(false)
    {
        crate::metrics::count(p, "ai_deferred_total", Some(tenant_id), 1);
        return Err(ModelError::RateLimited);
    }
    let model = p.config().agent_model.clone();
    let opts = AiOptions::content(p.config().ai_gateway.clone());
    let run = p.ai().run::<Value, Value>(&model, input, &opts);
    match join::before(run, p.clock().sleep(timeout_ms)).await {
        Some(Ok(out)) => parse(&out).ok_or(ModelError::Failed),
        Some(Err(e)) if is_rate_limited(&e) => {
            crate::metrics::count(p, "ai_rate_limited_total", Some(tenant_id), 1);
            Err(ModelError::RateLimited)
        }
        _ => Err(ModelError::Failed),
    }
}

/// A planning call's body.
pub fn plan_input(messages: &[Value]) -> Value {
    json!({
        "messages": messages,
        "tools": tools(),
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "temperature": 0.2,
        "max_completion_tokens": 1024,
        "chat_template_kwargs": { "enable_thinking": false },
    })
}

/// The answer schema ([Search § 11.7]).
pub fn answer_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false,
        "required": ["status", "sentences", "confidence", "not_found"],
        "properties": {
            "status": { "type": "string", "enum": ["answered", "insufficient_evidence"] },
            "sentences": { "type": "array", "maxItems": 12,
                "items": { "type": "object", "additionalProperties": false, "required": ["text", "citations"],
                    "properties": {
                        "text": { "type": "string", "minLength": 1, "maxLength": 600 },
                        "citations": { "type": "array", "maxItems": 5,
                            "items": { "type": "string", "pattern": "^msg_[0-9A-HJKMNP-TV-Z]{26}$" } } } } },
            "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
            "not_found": { "type": "array", "maxItems": 8, "items": { "type": "string", "maxLength": 200 } }
        }
    })
}

/// The answer call's body: the conversation, the final user message, no tools.
pub fn answer_input(messages: &[Value]) -> Value {
    let mut m = messages.to_vec();
    m.push(json!({ "role": "user", "content": super::prompts::ANSWER_NOW }));
    json!({
        "messages": m,
        "tools": tools(),
        "tool_choice": "none",
        "temperature": 0.2,
        "max_completion_tokens": 1500,
        "chat_template_kwargs": { "enable_thinking": false },
        "response_format": { "type": "json_schema",
            "json_schema": { "name": "agentic_answer", "strict": true, "schema": answer_schema() } },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_match_design() {
        let doc = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/src/project/design/search.md"),
        )
        .unwrap();
        assert!(doc.contains(TOOLS));
        let t = tools();
        let names: Vec<&str> = t
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "search",
                "read_thread",
                "read_message",
                "read_attachment_text",
                "find_related",
                "contacts"
            ]
        );
        for x in t.as_array().unwrap() {
            assert_eq!(x["function"]["parameters"]["additionalProperties"], false);
        }
    }

    #[test]
    fn parses_chat_completions() {
        let out = json!({ "choices": [ { "message": { "role": "assistant", "content": null,
            "tool_calls": [ { "id": "c1", "type": "function",
                "function": { "name": "search", "arguments": "{\"q\":\"claim\"}" } } ] },
            "finish_reason": "tool_calls" } ] });
        let r = parse(&out).unwrap();
        assert_eq!(r.tool_calls[0].name, "search");
        assert_eq!(r.tool_calls[0].arguments, "{\"q\":\"claim\"}");
        assert_eq!(r.message["tool_calls"][0]["id"], "c1");
        assert!(parse(&json!({ "result": 1 })).is_none());
        let text = parse(&json!({ "choices": [ { "message": { "content": "READY" } } ] })).unwrap();
        assert_eq!(text.content.as_deref(), Some("READY"));
        assert!(text.tool_calls.is_empty());
    }
}
