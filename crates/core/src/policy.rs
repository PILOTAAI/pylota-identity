//! Send policy ([Outbound › Policy pipeline]). M2 holds the loop limits that inbound classification
//! shares with the send path, and the idempotency fingerprint; the pipeline itself grows with the send
//! milestones.
//!
//! [Outbound › Policy pipeline]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use serde_json::Value;

/// `X-Pylota-Identity-Hop` at or above this makes inbound mail `automated`, and refuses a send
/// (`409 loop_detected`, N13).
pub const MAX_HOP: u32 = 10;

/// Sends per thread, and messages from one address, in 24 hours before step 10b refuses (N35).
pub const MAX_REPLIES_PER_DAY: u32 = 25;

/// The default `policy.auto_reply.max_automatic_exchanges` (D6).
pub const DEFAULT_MAX_AUTOMATIC_EXCHANGES: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendOp {
    Send,
    Reply,
    ReplyAll,
    Forward,
    CalendarReply,
}

impl SendOp {
    pub const fn as_str(self) -> &'static str {
        match self {
            SendOp::Send => "send",
            SendOp::Reply => "reply",
            SendOp::ReplyAll => "reply_all",
            SendOp::Forward => "forward",
            SendOp::CalendarReply => "calendar_reply",
        }
    }
}

/// Canonical JSON: keys sorted by their UTF-8 bytes at every level, no insignificant whitespace,
/// `serde_json`'s minimal string escapes and shortest numbers, arrays in order.
pub fn canonical_json(v: &Value) -> String {
    let mut out = String::new();
    write_canonical(v, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*k).clone()).to_string());
                out.push(':');
                write_canonical(&map[*k], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// `hex(sha256(canonical_json({"body": body, "operation": op, "target": target})))`; `target` is the
/// identity ID for `send`, `{identity_id}/{message_id}` otherwise.
pub fn send_fingerprint(operation: SendOp, target: &str, body: &Value) -> String {
    let v = serde_json::json!({ "body": body, "operation": operation.as_str(), "target": target });
    crate::ses::sha256_hex(canonical_json(&v).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_order_and_whitespace() {
        let a: Value =
            serde_json::from_str(r#"{ "b": [3, {"z":1,"a":"é\n"}], "a": 1.50, "B": null }"#)
                .unwrap();
        assert_eq!(
            canonical_json(&a),
            r#"{"B":null,"a":1.5,"b":[3,{"a":"é\n","z":1}]}"#
        );
        let b: Value =
            serde_json::from_str(r#"{"a":1.5,"B":null,"b":[3,{"a":"é\n","z":1}]}"#).unwrap();
        let f = |v: &Value| send_fingerprint(SendOp::Send, "idn_1", v);
        assert_eq!(f(&a), f(&b));
        assert_ne!(f(&a), send_fingerprint(SendOp::Reply, "idn_1", &a));
        assert_ne!(f(&a), f(&json!({"a": 1.5})));
        assert_eq!(f(&a).len(), 64);
    }
}
