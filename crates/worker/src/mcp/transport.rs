//! Request handling at `/mcp` (MCP § 2): the HTTP method, `Origin`, size and type, the JSON-RPC
//! parse, authentication and `RL_API` once per request, the era, the mirrored headers of the modern
//! era, and the methods. No session is ever minted and no server-to-client stream is offered.

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::app::HttpRequestIn;
use pylota_identity_platform::{Clock as _, Platform};
use serde_json::{Map, Value, json};

use super::schemas::{
    DiscoverResult, Implementation, Incoming, InitializeResult, JsonRpcError, JsonRpcResponse,
    ListChanged, RequestId, ServerCapabilities, code,
};
use super::{LEGACY, MAX_BODY, MODERN, SUPPORTED_VERSIONS, prompts, tools};
use crate::app::App;
use crate::auth::{self, KeyScope};
use crate::ctx::Reply;
use crate::errors::{ApiError, envelope};

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Era {
    Modern,
    Legacy,
}

fn reply(status: u16, body: &Value, headers: Vec<(String, String)>) -> Reply {
    Reply {
        status,
        body: None,
        raw: Some(("application/json", body.to_string().into_bytes())),
        headers,
        stream: None,
    }
}

fn empty(status: u16, headers: Vec<(String, String)>) -> Reply {
    Reply {
        status,
        body: None,
        raw: None,
        headers,
        stream: None,
    }
}

fn error(status: u16, e: JsonRpcError) -> Reply {
    reply(status, &val(&e), Vec::new())
}

fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        tools: ListChanged {
            list_changed: false,
        },
        prompts: ListChanged {
            list_changed: false,
        },
    }
}

fn server_info() -> Implementation {
    Implementation {
        name: "pylota-identity".into(),
        title: Some("Pylota Identity".into()),
        version: "1.0.0".into(),
    }
}

/// Decodes the `=?base64?…?=` form of `Mcp-Name` (MCP § 2.2).
fn header_name(v: &str) -> String {
    v.strip_prefix("=?base64?")
        .and_then(|r| r.strip_suffix("?="))
        .and_then(pylota_identity_core::links::decode_base64)
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| v.to_owned())
}

/// A failure before the request could be served as JSON-RPC, with the error envelope as `data`.
fn api_error(
    status: u16,
    id: Option<RequestId>,
    e: &ApiError,
    request_id: &str,
    docs: &str,
) -> Reply {
    let mut r = error(
        status,
        JsonRpcError::new(id, code::SERVER, e.message.clone()).data(envelope(e, request_id, docs)),
    );
    r.headers.extend(e.headers.iter().cloned());
    r
}

/// Serves one request to `/mcp`.
pub async fn serve<P: Platform>(app: &App<P>, req: &HttpRequestIn, request_id: &str) -> Reply {
    let cfg = app.p.config();
    let docs = cfg.docs_url.clone();
    let allow = || vec![("allow".to_owned(), "POST".to_owned())];
    // 1. Method.
    match req.method.as_str() {
        "POST" => {}
        "OPTIONS" => return empty(204, Vec::new()),
        _ => return empty(405, allow()),
    }
    // 2. Origin.
    if let Some(o) = req.header("origin")
        && o != format!("https://{}", cfg.api_host)
    {
        let e = JsonRpcError::new(None, code::SERVER, "Origin not allowed.");
        return reply(403, &e.without_id(), Vec::new());
    }
    // 3. Size and type.
    let size = req.body.as_ref().map_or(0, |b| b.len());
    if size > MAX_BODY {
        let e = ApiError::new(ErrorCode::PayloadTooLarge, "The body is over 7 MiB.");
        return api_error(413, None, &e, request_id, &docs);
    }
    let json_type = req
        .header("content-type")
        .and_then(|t| t.split(';').next())
        .is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json"));
    if !json_type {
        return error(
            415,
            JsonRpcError::new(
                None,
                code::INVALID_REQUEST,
                "Content-Type must be application/json.",
            ),
        );
    }
    // 4. Parse.
    let body = req.body.as_ref().map(|b| b.to_vec()).unwrap_or_default();
    let Ok(raw) = serde_json::from_slice::<Value>(&body) else {
        return error(400, JsonRpcError::new(None, code::PARSE, "Parse error."));
    };
    let raw_id = raw
        .get("id")
        .and_then(|i| serde_json::from_value::<RequestId>(i.clone()).ok());
    if !raw.is_object() {
        return error(
            400,
            JsonRpcError::new(None, code::INVALID_REQUEST, "One JSON-RPC request object."),
        );
    }
    let Ok(msg) = serde_json::from_value::<Incoming>(raw) else {
        return error(
            400,
            JsonRpcError::new(raw_id, code::INVALID_REQUEST, "Invalid request."),
        );
    };
    // 5. Authenticate.
    let key = match auth::authenticate(&app.p, req.header("authorization")).await {
        Ok(k) => k,
        Err(e) => {
            let status = e.code.http_status();
            let mut r = api_error(status, msg.id.clone(), &e, request_id, &docs);
            if status == 401 {
                r.headers.push((
                    "www-authenticate".into(),
                    "Bearer realm=\"pylota-identity\", error=\"invalid_token\"".into(),
                ));
            }
            return r;
        }
    };
    // 6. Rate limit, once per MCP request (MCP § 4).
    if let Err(e) = app.charge(&key).await {
        return api_error(e.code.http_status(), msg.id.clone(), &e, request_id, &docs);
    }
    // 7. Era.
    let params = msg.params.clone().unwrap_or_default();
    let meta_version = params
        .get("_meta")
        .and_then(|m| m.get(META_VERSION))
        .and_then(Value::as_str);
    let header_version = req.header("mcp-protocol-version");
    let unsupported = |requested: &str| {
        error(
            400,
            JsonRpcError::new(
                msg.id.clone(),
                code::UNSUPPORTED_VERSION,
                "Unsupported protocol version.",
            )
            .data(json!({ "supported": SUPPORTED_VERSIONS, "requested": requested })),
        )
    };
    let era = if msg.method == "initialize" {
        Era::Legacy
    } else if let Some(v) = meta_version {
        if v != MODERN {
            return unsupported(v);
        }
        let mismatch = |what: &str| {
            error(
                400,
                JsonRpcError::new(
                    msg.id.clone(),
                    code::HEADER_MISMATCH,
                    format!("{what} does not match the request body."),
                ),
            )
        };
        if header_version != Some(v) {
            return mismatch("MCP-Protocol-Version");
        }
        if req.header("mcp-method") != Some(msg.method.as_str()) {
            return mismatch("Mcp-Method");
        }
        if matches!(msg.method.as_str(), "tools/call" | "prompts/get") {
            let name = params.get("name").and_then(Value::as_str);
            if req.header("mcp-name").map(header_name).as_deref() != name {
                return mismatch("Mcp-Name");
            }
        }
        Era::Modern
    } else {
        match header_version {
            Some(v) if LEGACY.contains(&v) => Era::Legacy,
            v => return unsupported(v.unwrap_or("")),
        }
    };
    // 8. Dispatch; 9. respond.
    let Some(id) = msg.id.clone() else {
        // A notification: nothing to answer (`notifications/cancelled` cannot reach another isolate).
        return empty(202, Vec::new());
    };
    let started = app.p.clock().now_ms();
    let out = dispatch(app, &key, era, &msg.method, &params, request_id).await;
    let (outcome, tool, identity, error_code, query) = (
        match &out {
            Ok(_) => "ok",
            Err(_) => "protocol_error",
        },
        out.as_ref().ok().and_then(|o| o.tool),
        out.as_ref().ok().and_then(|o| o.identity.clone()),
        out.as_ref().ok().and_then(|o| o.error_code),
        out.as_ref().ok().and_then(|o| o.query.clone()),
    );
    let mut fields: Vec<(&'static str, crate::log::Field)> = vec![
        ("method", msg.method.as_str().into()),
        ("key_id", key.key_id.as_str().into()),
        ("duration_ms", (app.p.clock().now_ms() - started).into()),
        (
            "outcome",
            if error_code.is_some() {
                "tool_error"
            } else {
                outcome
            }
            .into(),
        ),
    ];
    if let Some(t) = tool {
        fields.push(("tool", t.into()));
    }
    if let Some(i) = identity {
        fields.push(("identity", i.into()));
    }
    if let Some(c) = error_code {
        fields.push(("error_code", c.into()));
    }
    if let Some(q) = query {
        fields.push((
            "query_hash",
            pylota_identity_core::search::cursor::query_hash(&app.p.secrets().hash_key, &q).into(),
        ));
    }
    crate::log::info("mcp_request", &fields);
    match out {
        Ok(d) => {
            let mut result = d.result;
            if era == Era::Modern
                && let Some(o) = result.as_object_mut()
            {
                o.insert("resultType".into(), "complete".into());
            }
            let r = JsonRpcResponse {
                jsonrpc: super::schemas::TwoPointZero,
                id,
                result,
            };
            reply(200, &val(&r), Vec::new())
        }
        Err((status, e)) => {
            let status = match (era, e.error.code) {
                (Era::Modern, code::METHOD_NOT_FOUND) => 404,
                _ => status,
            };
            error(status, JsonRpcError { id: Some(id), ..e })
        }
    }
}

/// A served method: the result and what the log line names.
struct Done {
    result: Value,
    tool: Option<&'static str>,
    identity: Option<String>,
    error_code: Option<&'static str>,
    query: Option<String>,
}

impl Done {
    fn of(result: Value) -> Self {
        Self {
            result,
            tool: None,
            identity: None,
            error_code: None,
            query: None,
        }
    }
}

fn invalid_params(message: impl Into<String>) -> (u16, JsonRpcError) {
    (200, JsonRpcError::new(None, code::INVALID_PARAMS, message))
}

async fn dispatch<P: Platform>(
    app: &App<P>,
    key: &KeyScope,
    era: Era,
    method: &str,
    params: &Map<String, Value>,
    request_id: &str,
) -> Result<Done, (u16, JsonRpcError)> {
    match (method, era) {
        ("initialize", _) => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked.filter(|v| LEGACY.contains(v)).unwrap_or(LEGACY[0]);
            Ok(Done::of(val(&InitializeResult {
                protocol_version: version.into(),
                capabilities: capabilities(),
                server_info: server_info(),
                instructions: Some(prompts::INSTRUCTIONS.into()),
            })))
        }
        ("server/discover", Era::Modern) => Ok(Done::of(val(&DiscoverResult {
            result_type: "complete".into(),
            supported_versions: SUPPORTED_VERSIONS.iter().map(|v| (*v).to_owned()).collect(),
            capabilities: capabilities(),
            instructions: Some(prompts::INSTRUCTIONS.into()),
            ttl_ms: 3_600_000,
            cache_scope: "public".into(),
            meta: json!({ "io.modelcontextprotocol/serverInfo": server_info() }),
        }))),
        ("ping", _) => Ok(Done::of(json!({}))),
        ("tools/list", _) => {
            let list: Vec<Value> = tools::visible(key)
                .map(|d| val(&tools::listed(d)))
                .collect();
            let mut r = json!({ "tools": list });
            if era == Era::Modern {
                r["ttlMs"] = 300_000.into();
                r["cacheScope"] = "private".into();
            }
            Ok(Done::of(r))
        }
        ("tools/call", _) => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Err(invalid_params("tools/call needs a tool name."));
            };
            let args = match params.get("arguments") {
                None | Some(Value::Null) => json!({}),
                Some(a @ Value::Object(_)) => a.clone(),
                Some(_) => return Err(invalid_params("arguments must be an object.")),
            };
            let Some(def) = tools::find(key, name) else {
                return Err(invalid_params(format!("Unknown tool: {name}")));
            };
            let query = match def.name {
                "mail_search" => args["q"].as_str().map(str::to_owned),
                "mail_deep_search" => args["question"].as_str().map(str::to_owned),
                _ => None,
            };
            let out = tools::run(app, key, def, &args, request_id).await;
            Ok(Done {
                result: val(&out.result),
                tool: Some(def.name),
                identity: out.identity,
                error_code: out.error_code,
                query,
            })
        }
        ("prompts/list", _) => {
            let list: Vec<Value> = if key.has("search:read") {
                vec![val(&prompts::listed())]
            } else {
                Vec::new()
            };
            Ok(Done::of(json!({ "prompts": list })))
        }
        ("prompts/get", _) => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            if name != prompts::PROMPT_NAME || !key.has("search:read") {
                return Err(invalid_params(format!("Unknown prompt: {name}")));
            }
            let goal = params
                .get("arguments")
                .and_then(|a| a.get("goal"))
                .and_then(Value::as_str);
            Ok(Done::of(val(&prompts::get(goal))))
        }
        _ => Err((
            200,
            JsonRpcError::new(None, code::METHOD_NOT_FOUND, "Method not found."),
        )),
    }
}

fn val<T: serde::Serialize>(t: &T) -> Value {
    serde_json::to_value(t).unwrap_or(Value::Null)
}
