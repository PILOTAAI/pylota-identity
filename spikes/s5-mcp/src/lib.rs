//! Spike S5's Worker (build plan M1): MCP over Streamable HTTP served from `fetch` with local JSON-RPC
//! types ([ADR 0009]), no `rmcp` and no tokio. Dual-era as [MCP §2] requires: an `initialize` request selects
//! the 2025 ("legacy") semantics; a request whose `params._meta` carries
//! `io.modelcontextprotocol/protocolVersion` is served statelessly under 2026-07-28 ("modern"). One tool,
//! `echo`. Checked with MCP Inspector and Claude Code; see `RESULT.md`.
//!
//! [ADR 0009]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/adr/0009-local-mcp-protocol-types.md
//! [MCP §2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/mcp.md
#![forbid(unsafe_code)]

use serde_json::{Value, json};

pub const SUPPORTED_VERSIONS: [&str; 3] = ["2026-07-28", "2025-11-25", "2025-06-18"];
const LEGACY: [&str; 2] = ["2025-11-25", "2025-06-18"];
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

/// An HTTP answer: status and an optional JSON body.
#[derive(Debug, PartialEq)]
pub struct Answer {
    pub status: u16,
    pub body: Option<Value>,
}

/// Handles one POST to the MCP endpoint. `header` looks a request header up, case-insensitively.
pub fn handle(body: &[u8], header: impl Fn(&str) -> Option<String>) -> Answer {
    let Ok(request) = serde_json::from_slice::<Value>(body) else {
        return error(400, Value::Null, -32700, "parse error");
    };
    if request.is_array() {
        return error(400, Value::Null, -32600, "batches are not supported");
    }
    let id = request.get("id").cloned();
    let method = request["method"].as_str().unwrap_or_default();
    let Some(id) = id else {
        // A notification: accepted, no body.
        return Answer {
            status: 202,
            body: None,
        };
    };
    let modern_version = request["params"]["_meta"][META_VERSION]
        .as_str()
        .map(str::to_owned);

    if method == "initialize" {
        let asked = request["params"]["protocolVersion"]
            .as_str()
            .unwrap_or_default();
        let version = if LEGACY.contains(&asked) {
            asked
        } else {
            LEGACY[0]
        };
        return ok(
            id,
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "pylota-identity-spike-s5", "version": "0.1.0" },
            }),
        );
    }

    let modern = match modern_version {
        Some(version) => {
            if !SUPPORTED_VERSIONS.contains(&version.as_str()) || LEGACY.contains(&version.as_str())
            {
                return error_data(
                    400,
                    id,
                    -32022,
                    "unsupported protocol version",
                    json!({ "supported": SUPPORTED_VERSIONS, "requested": version }),
                );
            }
            if header("mcp-protocol-version").as_deref() != Some(version.as_str())
                || header("mcp-method").as_deref() != Some(method)
            {
                return error(400, id, -32020, "header mismatch");
            }
            if method == "tools/call"
                && header("mcp-name").as_deref() != request["params"]["name"].as_str()
            {
                return error(400, id, -32020, "header mismatch");
            }
            true
        }
        None => {
            let version = header("mcp-protocol-version");
            if version.is_some_and(|v| !LEGACY.contains(&v.as_str())) {
                return error(400, id, -32022, "unsupported protocol version");
            }
            false
        }
    };

    let result = match method {
        "server/discover" if modern => json!({
            "supportedVersions": SUPPORTED_VERSIONS,
            "capabilities": { "tools": { "listChanged": false } },
            "_meta": { "io.modelcontextprotocol/serverInfo":
                       { "name": "pylota-identity-spike-s5", "version": "0.1.0" } },
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": [{
            "name": "echo",
            "description": "Returns the text it is given.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"],
            },
        }] }),
        "tools/call" => {
            if request["params"]["name"] != "echo" {
                return error(200, id, -32602, "unknown tool");
            }
            let text = request["params"]["arguments"]["text"]
                .as_str()
                .unwrap_or_default();
            json!({ "content": [{ "type": "text", "text": text }], "isError": false })
        }
        _ => return error(200, id, -32601, "method not found"),
    };
    let mut result = result;
    if modern {
        result["resultType"] = json!("complete");
    }
    ok(id, result)
}

fn ok(id: Value, result: Value) -> Answer {
    Answer {
        status: 200,
        body: Some(json!({ "jsonrpc": "2.0", "id": id, "result": result })),
    }
}

fn error(status: u16, id: Value, code: i64, message: &str) -> Answer {
    Answer {
        status,
        body: Some(
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
        ),
    }
}

fn error_data(status: u16, id: Value, code: i64, message: &str, data: Value) -> Answer {
    let mut answer = error(status, id, code, message);
    if let Some(body) = answer.body.as_mut() {
        body["error"]["data"] = data;
    }
    answer
}

#[cfg(target_arch = "wasm32")]
mod entry {
    use pylota_identity_platform::cf::glue::{
        CfPlatform, HttpRequestIn, HttpResponseOut, WorkerApp,
    };

    pub struct App;

    impl WorkerApp for App {
        fn new(_platform: CfPlatform) -> Self {
            App
        }

        async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
            if req.method != "POST" {
                return HttpResponseOut::Http {
                    status: 405,
                    headers: vec![("allow".to_owned(), "POST".to_owned())],
                    body: Vec::new(),
                };
            }
            let answer = super::handle(&req.body, |name| {
                req.headers
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name))
                    .map(|(_, v)| v.clone())
            });
            let (headers, body) = match answer.body {
                Some(body) => (
                    vec![("content-type".to_owned(), "application/json".to_owned())],
                    body.to_string().into_bytes(),
                ),
                None => (Vec::new(), Vec::new()),
            };
            HttpResponseOut::Http {
                status: answer.status,
                headers,
                body,
            }
        }
    }

    pylota_identity_platform::export_worker! {
        app: crate::entry::App,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_headers(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn legacy_initialize_negotiates() {
        let a = handle(
            br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
            no_headers,
        );
        assert_eq!(a.body.unwrap()["result"]["protocolVersion"], "2025-11-25");
    }

    #[test]
    fn notification_is_202() {
        let a = handle(
            br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            no_headers,
        );
        assert_eq!(
            a,
            Answer {
                status: 202,
                body: None
            }
        );
    }

    #[test]
    fn modern_call_checks_headers_and_marks_complete() {
        let body = br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{"text":"hi"},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#;
        let bad = handle(body, no_headers);
        assert_eq!(bad.status, 400);
        let good = handle(body, |h| match h {
            "mcp-protocol-version" => Some("2026-07-28".into()),
            "mcp-method" => Some("tools/call".into()),
            "mcp-name" => Some("echo".into()),
            _ => None,
        });
        let result = &good.body.unwrap()["result"];
        assert_eq!(result["content"][0]["text"], "hi");
        assert_eq!(result["resultType"], "complete");
    }
}
