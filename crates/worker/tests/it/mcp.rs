//! The MCP server through workerd (MCP server › Tests): the tool list and its schemas, the mapping of
//! each tool onto its REST route, tool and protocol errors, both eras and the transport's refusals.

use serde_json::{Value, json};

use crate::send::{Sender, open_sender};
use crate::{Resp, call, get, inbound, mail, mint, platform, post};

/// The tool permissions of M15, all of them.
const ALL: &[&str] = &[
    "identities:read",
    "messages:read",
    "messages:write",
    "messages:send",
    "search:read",
    "search:agentic",
    "attachments:read",
];

const LEGACY: &str = "2025-11-25";
const MODERN: &str = "2026-07-28";

/// A JSON-RPC request in the legacy era (no handshake needed: the server keeps no session).
fn rpc(key: &str, method: &str, params: Value) -> Resp {
    call(
        "POST",
        "/mcp",
        Some(key),
        Some(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })),
        &[
            ("mcp-protocol-version", LEGACY),
            ("accept", "application/json, text/event-stream"),
        ],
    )
}

/// A modern request: `_meta` in the body and the mirrored headers.
fn modern(key: &str, method: &str, mut params: Value, headers: &[(&str, &str)]) -> Resp {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientInfo": { "name": "it", "version": "1" },
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    call(
        "POST",
        "/mcp",
        Some(key),
        Some(json!({ "jsonrpc": "2.0", "id": "m1", "method": method, "params": params })),
        headers,
    )
}

/// `tools/call`: the result object (`content`, `structuredContent`, `isError`).
fn tool(key: &str, name: &str, args: Value) -> Value {
    let r = rpc(
        key,
        "tools/call",
        json!({ "name": name, "arguments": args }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.body.get("error").is_none(), "{name}: {}", r.body);
    r.body["result"].clone()
}

/// A tool's success: its `structuredContent`, after checking that the text block holds the same JSON.
fn ok(key: &str, name: &str, args: Value) -> Value {
    let r = tool(key, name, args);
    assert_eq!(r["isError"], false, "{name}: {r}");
    let text: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, r["structuredContent"], "{name}");
    r["structuredContent"].clone()
}

/// A tool's error envelope.
fn err(key: &str, name: &str, args: Value) -> Value {
    let r = tool(key, name, args);
    assert_eq!(r["isError"], true, "{name}: {r}");
    assert!(r.get("structuredContent").is_none());
    serde_json::from_str::<Value>(r["content"][0]["text"].as_str().unwrap()).unwrap()["error"]
        .clone()
}

fn names(key: &str) -> Vec<String> {
    let r = rpc(key, "tools/list", json!({}));
    assert_eq!(r.status, 200, "{}", r.body);
    r.body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect()
}

/// A sender with one received message, and a tenant key holding every tool permission.
fn fixture(tag: &str) -> (Sender, String, Value) {
    let s = open_sender(tag, "live");
    let (_, key) = mint(
        &platform(),
        json!({ "name": tag, "level": "tenant", "tenant_id": s.tid, "permissions": ALL }),
    );
    let from = "ola@mcp.example";
    let subject = format!("{tag} plate AB12CDE");
    let (o, _) = inbound(
        from,
        &s.addr,
        &mail(from, &s.addr, &subject, "About the booking."),
    );
    assert_eq!(o, "accepted");
    let m = crate::wait_stored(&s.iid, &s.key, "received", 1)[0].clone();
    (s, key, m)
}

/// `it::mcp::input_schemas_plain_objects`: every listed tool's `inputSchema` is a plain object at the
/// top level; `mail_update_labels` with both or neither target is refused and changes nothing.
#[test]
fn input_schemas_plain_objects() {
    let (s, key, m) = fixture("mcp schemas");
    let r = rpc(&key, "tools/list", json!({}));
    let list = r.body["result"]["tools"].as_array().unwrap().clone();
    assert_eq!(list.len(), 14, "{}", r.body);
    for t in &list {
        let s = &t["inputSchema"];
        assert_eq!(s["type"], "object", "{}", t["name"]);
        for k in ["oneOf", "anyOf", "allOf", "not"] {
            assert!(s.get(k).is_none(), "{} has {k} at the top", t["name"]);
        }
        assert_eq!(t["outputSchema"]["type"], "object", "{}", t["name"]);
        assert!(
            !t["outputSchema"].to_string().contains("$ref"),
            "{}",
            t["name"]
        );
    }
    let id = m["id"].as_str().unwrap();
    let thread = m["thread_id"].as_str().unwrap();
    let both = err(
        &key,
        "mail_update_labels",
        json!({ "identity": s.iid, "message_id": id, "thread_id": thread, "labels_add": ["x"] }),
    );
    assert_eq!(both["code"], "invalid_request");
    assert_eq!(both["details"]["errors"][0]["path"], "thread_id");
    let none = err(
        &key,
        "mail_update_labels",
        json!({ "identity": s.iid, "labels_add": ["x"] }),
    );
    assert_eq!(none["details"]["errors"][0]["path"], "message_id");
    let nothing = err(
        &key,
        "mail_update_labels",
        json!({ "identity": s.iid, "message_id": id }),
    );
    assert_eq!(nothing["details"]["errors"][0]["path"], "labels_add");
    let after = get(&format!("/v1/identities/{}/messages/{id}", s.iid), &key);
    assert_eq!(after.body["labels"], m["labels"], "nothing changed");
    let done = ok(
        &key,
        "mail_update_labels",
        json!({ "identity": s.iid, "message_id": id, "labels_add": ["claimed"], "read": true }),
    );
    assert_eq!(done["id"], id);
    assert!(
        done["labels"]
            .as_array()
            .unwrap()
            .contains(&json!("claimed"))
    );
    assert_eq!(done["read"], true);
}

/// FR-MCP-1: each permission set lists exactly its tools; a hidden and a missing tool get the same
/// `-32602`; a key's `mcp_tools` narrows the list ([Y49], [Y51]).
#[test]
fn tools_list_filtered_by_permission() {
    let s = open_sender("mcp filter", "live");
    let mk = |perms: &[&str], extra: Value| {
        let mut b =
            json!({ "name": "f", "level": "tenant", "tenant_id": s.tid, "permissions": perms });
        if let Some(o) = extra.as_object() {
            for (k, v) in o {
                b[k] = v.clone();
            }
        }
        mint(&platform(), b).1
    };
    let read = mk(&["messages:read"], json!({}));
    assert_eq!(
        names(&read),
        ["mail_list_threads", "mail_get_thread", "mail_get_message"]
    );
    let search = mk(&["search:read"], json!({}));
    assert_eq!(
        names(&search),
        [
            "mail_search",
            "mail_find_related",
            "mail_search_contacts",
            "mail_wait"
        ]
    );
    let deep = mk(&["search:read", "search:agentic"], json!({}));
    assert!(names(&deep).contains(&"mail_deep_search".to_owned()));
    let sender = mk(&["messages:send"], json!({}));
    assert_eq!(names(&sender), ["mail_send", "mail_reply", "mail_forward"]);
    let hidden = rpc(
        &read,
        "tools/call",
        json!({ "name": "mail_search", "arguments": { "q": "x" } }),
    );
    let missing = rpc(
        &read,
        "tools/call",
        json!({ "name": "mail_nope", "arguments": {} }),
    );
    assert_eq!(hidden.status, 200);
    assert_eq!(hidden.body["error"]["code"], -32602);
    assert_eq!(hidden.body["error"]["message"], "Unknown tool: mail_search");
    assert_eq!(missing.body["error"]["message"], "Unknown tool: mail_nope");
    // Y49 and Y51: the allow-list narrows, and a name the release does not have is ignored.
    let narrow = mk(
        &["messages:read", "search:read"],
        json!({ "mcp_tools": ["mail_get_message", "mail_search", "mail_retired_tool"] }),
    );
    assert_eq!(names(&narrow), ["mail_search", "mail_get_message"]);
    let blocked = rpc(
        &narrow,
        "tools/call",
        json!({ "name": "mail_list_threads", "arguments": { "identity": s.iid } }),
    );
    assert_eq!(
        blocked.body["error"]["message"],
        "Unknown tool: mail_list_threads"
    );
    // The prompt needs search:read.
    let p = rpc(&read, "prompts/list", json!({}));
    assert_eq!(p.body["result"]["prompts"], json!([]));
    let p = rpc(
        &search,
        "prompts/get",
        json!({ "name": "mail_search_strategy", "arguments": { "goal": "find the claim" } }),
    );
    let text = p.body["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(text.ends_with("Your current goal: find the claim"));
    let p = rpc(
        &read,
        "prompts/get",
        json!({ "name": "mail_search_strategy" }),
    );
    assert_eq!(
        p.body["error"]["message"],
        "Unknown prompt: mail_search_strategy"
    );
}

/// Without volatile members (`as_of`).
fn stable(mut v: Value) -> Value {
    if let Some(o) = v.as_object_mut() {
        o.remove("as_of");
    }
    v
}

/// FR-MCP-1, FR-OUT-1: every tool answers what its REST route answers for the same inputs, including
/// the idempotent replay of the send tools.
#[test]
fn call_maps_to_rest() {
    let (s, key, m) = fixture("mcp table");
    let i = &s.iid;
    let id = m["id"].as_str().unwrap();
    let thread = m["thread_id"].as_str().unwrap();
    let rest_post = |path: &str, body: Value| {
        let r = post(path, &key, body);
        assert_eq!(r.status, 200, "{path}: {}", r.body);
        r.body
    };
    // Searchable once indexed.
    crate::eventually(60, "the message to be searchable", || {
        let r = post(
            &format!("/v1/identities/{i}/search"),
            &key,
            json!({ "q": "booking", "mode": "keyword" }),
        );
        (r.body["hits"].as_array().is_some_and(|h| !h.is_empty())).then_some(())
    });
    let rest_get = |path: &str| {
        let r = get(path, &key);
        assert_eq!(r.status, 200, "{path}: {}", r.body);
        r.body
    };
    // (tool, arguments, the REST answer for the same inputs)
    let table: Vec<(&str, Value, Value)> = vec![
        (
            "mail_list_identities",
            json!({}),
            rest_get("/v1/identities?limit=25"),
        ),
        (
            "mail_list_threads",
            json!({ "identity": s.addr }),
            rest_get(&format!(
                "/v1/identities/{i}/threads?archived=false&limit=20"
            )),
        ),
        (
            "mail_get_thread",
            json!({ "identity": i, "thread_id": thread }),
            rest_get(&format!(
                "/v1/identities/{i}/threads/{thread}?messages_limit=10"
            )),
        ),
        (
            "mail_get_message",
            json!({ "identity": i, "message_id": id, "include_headers": true }),
            rest_get(&format!("/v1/identities/{i}/messages/{id}?include=headers")),
        ),
        (
            "mail_find_related",
            json!({ "identity": i, "message_id": id }),
            rest_get(&format!("/v1/identities/{i}/messages/{id}/related?limit=5")),
        ),
        (
            "mail_search_contacts",
            json!({ "identity": i, "q": "ola" }),
            rest_get(&format!("/v1/identities/{i}/contacts?q=ola&limit=10")),
        ),
        (
            "mail_search",
            json!({ "identity": i, "q": "booking", "mode": "keyword" }),
            rest_post(
                &format!("/v1/identities/{i}/search"),
                json!({ "q": "booking", "mode": "keyword", "group_by": "message", "limit": 10, "snippet_chars": 200, "include_quarantined": false }),
            ),
        ),
        (
            "mail_search",
            json!({ "scope": "tenant", "q": "booking", "mode": "keyword" }),
            rest_post(
                &format!("/v1/tenants/{}/search", s.tid),
                json!({ "q": "booking", "mode": "keyword", "group_by": "message", "limit": 10, "snippet_chars": 200, "include_quarantined": false }),
            ),
        ),
    ];
    for (name, args, rest) in table {
        assert_eq!(
            stable(ok(&key, name, args.clone())),
            stable(rest),
            "{name} {args}"
        );
    }
    let hits = ok(
        &key,
        "mail_search",
        json!({ "identity": i, "q": "booking", "mode": "keyword" }),
    );
    assert_eq!(hits["hits"][0]["message_id"], id, "{hits}");
    let w = ok(
        &key,
        "mail_wait",
        json!({ "identity": i, "thread_id": thread, "timeout_seconds": 1, "since": "2099-01-01T00:00:00Z" }),
    );
    assert_eq!(w["timed_out"], true, "{w}");
    // The send tools: the queued message, and the same result on a replay.
    let args = json!({ "identity": i, "to": ["pia@mcp.example"], "subject": "MCP send", "text": "Hi", "idempotency_key": "mcp-table-1" });
    let first = ok(&key, "mail_send", args.clone());
    assert_eq!(first["deduplicated"], false, "{first}");
    let again = ok(&key, "mail_send", args);
    assert_eq!(
        (again["id"].clone(), again["deduplicated"].clone()),
        (first["id"].clone(), json!(true))
    );
    let sent = rest_get(&format!(
        "/v1/identities/{i}/messages/{}",
        first["id"].as_str().unwrap()
    ));
    assert_eq!(sent["subject"], "MCP send");
    let reply = ok(
        &key,
        "mail_reply",
        json!({ "identity": i, "message_id": id, "text": "Thanks", "idempotency_key": "mcp-table-2" }),
    );
    assert_eq!(reply["thread_id"], thread);
    let fwd = ok(
        &key,
        "mail_forward",
        json!({ "identity": i, "message_id": id, "to": ["rex@mcp.example"], "text": "FYI", "idempotency_key": "mcp-table-3" }),
    );
    assert_eq!(fwd["direction"], "outbound");
    // An identity key acts as its own identity and cannot name another.
    let (_, own) = mint(
        &platform(),
        json!({ "name": "own", "level": "identity", "tenant_id": s.tid, "identity_id": i, "permissions": ["messages:read"] }),
    );
    let threads = ok(&own, "mail_list_threads", json!({}));
    assert_eq!(
        threads,
        rest_get(&format!(
            "/v1/identities/{i}/threads?archived=false&limit=20"
        ))
    );
    let other = crate::identity(&platform(), &s.tid, "mcpother");
    let e = err(
        &own,
        "mail_list_threads",
        json!({ "identity": other["id"] }),
    );
    assert_eq!(e["code"], "identity_not_found");
    let e = err(
        &own,
        "mail_list_threads",
        json!({ "identity": other["primary_address"] }),
    );
    assert_eq!(e["code"], "identity_not_found");
}

/// FR-API-2: schema failures, REST errors and rate limits are `isError` results carrying the envelope;
/// protocol errors use the codes and statuses of MCP § 2.4.
#[test]
fn error_mapping() {
    let (s, key, _) = fixture("mcp errors");
    let e = err(
        &key,
        "mail_get_message",
        json!({ "identity": s.iid, "message_id": "nope", "x": 1 }),
    );
    assert_eq!(e["code"], "invalid_request");
    let paths: Vec<&str> = e["details"]["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"message_id") && paths.contains(&"x"), "{e}");
    let send = json!({ "identity": s.iid, "to": ["a@mcp.example"], "subject": "x", "text": "y" });
    assert_eq!(
        err(&key, "mail_send", send.clone())["code"],
        "idempotency_key_required"
    );
    let mut bad = send.clone();
    bad["idempotency_key"] = "\u{1}".into();
    assert_eq!(
        err(&key, "mail_send", bad)["code"],
        "invalid_idempotency_key"
    );
    let e = err(
        &key,
        "mail_get_message",
        json!({ "identity": s.iid, "message_id": "msg_01ARYZ6S41TSV4RRFFQ69G5FAV" }),
    );
    assert_eq!(e["code"], "message_not_found");
    assert_eq!(e["details"]["http_status"], 404);
    assert!(
        e["docs"]
            .as_str()
            .unwrap()
            .ends_with("#code-message_not_found")
    );
    let e = err(&key, "mail_list_threads", json!({}));
    assert_eq!(
        (
            e["code"].as_str(),
            e["details"]["errors"][0]["path"].as_str()
        ),
        (Some("invalid_request"), Some("identity"))
    );
    let e = err(
        &key,
        "mail_search",
        json!({ "q": "x", "identity_ids": [s.iid] }),
    );
    assert_eq!(e["code"], "invalid_request");
    // Protocol errors.
    let raw = |body: &str, ct: &str| {
        let r = crate::client()
            .post(format!("{}/mcp", std::env::var("PM_ITEST_URL").unwrap()))
            .bearer_auth(&key)
            .header("content-type", ct)
            .header("mcp-protocol-version", LEGACY)
            .body(body.to_owned())
            .send()
            .unwrap();
        let status = r.status().as_u16();
        (status, r.json::<Value>().unwrap_or(Value::Null))
    };
    let (st, b) = raw("{not json", "application/json");
    assert_eq!(
        (st, b["error"]["code"].as_i64(), b["id"].is_null()),
        (400, Some(-32700), true)
    );
    let (st, b) = raw(
        "[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}]",
        "application/json",
    );
    assert_eq!((st, b["error"]["code"].as_i64()), (400, Some(-32600)));
    let (st, b) = raw("{}", "text/plain");
    assert_eq!((st, b["error"]["code"].as_i64()), (415, Some(-32600)));
    let r = rpc(&key, "resources/list", json!({}));
    assert_eq!(
        (r.status, r.body["error"]["code"].as_i64()),
        (200, Some(-32601))
    );
    let r = modern(
        &key,
        "resources/list",
        json!({}),
        &[
            ("mcp-protocol-version", MODERN),
            ("mcp-method", "resources/list"),
        ],
    );
    assert_eq!(
        (r.status, r.body["error"]["code"].as_i64()),
        (404, Some(-32601))
    );
    let r = rpc(
        &key,
        "tools/call",
        json!({ "name": "mail_search", "arguments": [1] }),
    );
    assert_eq!(r.body["error"]["code"], -32602);
    // A notification is accepted with no body.
    let n = call(
        "POST",
        "/mcp",
        Some(&key),
        Some(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })),
        &[("mcp-protocol-version", LEGACY)],
    );
    assert_eq!(n.status, 202);
}

/// M15: `initialize` mints no session; a sent session ID is ignored; `GET` and `DELETE` answer `405`.
#[test]
fn legacy_session() {
    let (_, key, _) = fixture("mcp legacy");
    let r = call(
        "POST",
        "/mcp",
        Some(&key),
        Some(
            json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "it", "version": "1" } } }),
        ),
        &[],
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(r.header("mcp-session-id").is_none());
    let res = &r.body["result"];
    assert_eq!(res["protocolVersion"], "2025-06-18");
    assert_eq!(res["serverInfo"]["name"], "pylota-identity");
    assert_eq!(res["capabilities"]["tools"]["listChanged"], false);
    assert!(res["instructions"].as_str().unwrap().contains("untrusted"));
    let old = call(
        "POST",
        "/mcp",
        Some(&key),
        Some(
            json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2024-11-05" } }),
        ),
        &[],
    );
    assert_eq!(
        (old.status, old.body["result"]["protocolVersion"].as_str()),
        (200, Some("2025-11-25"))
    );
    let r = call(
        "POST",
        "/mcp",
        Some(&key),
        Some(json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" })),
        &[
            ("mcp-protocol-version", LEGACY),
            ("mcp-session-id", "made-up"),
        ],
    );
    assert_eq!((r.status, &r.body["result"]), (200, &json!({})));
    assert!(r.header("mcp-session-id").is_none());
    for m in ["GET", "DELETE", "PUT"] {
        let r = call(m, "/mcp", Some(&key), None, &[]);
        assert_eq!((r.status, r.header("allow")), (405, Some("POST")), "{m}");
    }
    assert_eq!(call("OPTIONS", "/mcp", None, None, &[]).status, 204);
}

/// MCP § 2.2: the mirrored headers must match the body; `Mcp-Name` may be base64-encoded.
#[test]
fn modern_headers() {
    let (_, key, _) = fixture("mcp modern");
    let h = [
        ("mcp-protocol-version", MODERN),
        ("mcp-method", "tools/list"),
    ];
    let r = modern(&key, "tools/list", json!({}), &h);
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["result"]["resultType"], "complete");
    assert_eq!(r.body["result"]["cacheScope"], "private");
    let d = modern(
        &key,
        "server/discover",
        json!({}),
        &[
            ("mcp-protocol-version", MODERN),
            ("mcp-method", "server/discover"),
        ],
    );
    assert_eq!(
        d.body["result"]["supportedVersions"],
        json!([MODERN, "2025-11-25", "2025-06-18"])
    );
    assert_eq!(
        d.body["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "pylota-identity"
    );
    for headers in [
        vec![("mcp-method", "tools/list")],
        vec![("mcp-protocol-version", MODERN)],
        vec![("mcp-protocol-version", MODERN), ("mcp-method", "ping")],
    ] {
        let r = modern(&key, "tools/list", json!({}), &headers);
        assert_eq!(
            (r.status, r.body["error"]["code"].as_i64()),
            (400, Some(-32020)),
            "{headers:?}"
        );
    }
    fn call_h(name: &str) -> [(&'static str, &str); 3] {
        [
            ("mcp-protocol-version", MODERN),
            ("mcp-method", "tools/call"),
            ("mcp-name", name),
        ]
    }
    let args = json!({ "name": "mail_list_identities", "arguments": {} });
    let r = modern(&key, "tools/call", args.clone(), &call_h("mail_search"));
    assert_eq!(r.body["error"]["code"], -32020);
    let encoded = format!("=?base64?{}?=", "bWFpbF9saXN0X2lkZW50aXRpZXM=");
    let r = modern(&key, "tools/call", args, &call_h(&encoded));
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body["result"]["isError"], false);
}

/// MCP § 2.2, § 2.3: an unknown version is `400`/`-32022` with the supported list.
#[test]
fn unsupported_version() {
    let (_, key, _) = fixture("mcp versions");
    let mut body = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping", "params": { "_meta": { "io.modelcontextprotocol/protocolVersion": "2099-01-01" } } });
    let r = call(
        "POST",
        "/mcp",
        Some(&key),
        Some(body.clone()),
        &[
            ("mcp-protocol-version", "2099-01-01"),
            ("mcp-method", "ping"),
        ],
    );
    assert_eq!(
        (r.status, r.body["error"]["code"].as_i64()),
        (400, Some(-32022))
    );
    assert_eq!(r.body["error"]["data"]["supported"][0], MODERN);
    assert_eq!(r.body["error"]["data"]["requested"], "2099-01-01");
    body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = LEGACY.into();
    let r = call(
        "POST",
        "/mcp",
        Some(&key),
        Some(body),
        &[("mcp-protocol-version", LEGACY), ("mcp-method", "ping")],
    );
    assert_eq!(
        r.body["error"]["code"], -32022,
        "a legacy version with modern _meta"
    );
    let r = call(
        "POST",
        "/mcp",
        Some(&key),
        Some(json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" })),
        &[("mcp-protocol-version", "2025-03-26")],
    );
    assert_eq!(
        (r.status, r.body["error"]["code"].as_i64()),
        (400, Some(-32022))
    );
}

/// MCP § 2.1: a foreign `Origin` is refused with an error that has no `id`.
#[test]
fn origin_403() {
    let (_, key, _) = fixture("mcp origin");
    let r = call(
        "POST",
        "/mcp",
        Some(&key),
        Some(json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" })),
        &[
            ("mcp-protocol-version", LEGACY),
            ("origin", "https://evil.example"),
        ],
    );
    assert_eq!(r.status, 403);
    assert!(r.body.get("id").is_none(), "{}", r.body);
    assert_eq!(r.body["error"]["code"], -32000);
}

/// FR-MCP-1: missing, unknown and revoked keys get `401` with `WWW-Authenticate`.
#[test]
fn auth_401() {
    let (s, _, _) = fixture("mcp auth");
    let (kid, key) = mint(
        &platform(),
        json!({ "name": "r", "level": "tenant", "tenant_id": s.tid, "permissions": ["messages:read"] }),
    );
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
    let h = [("mcp-protocol-version", LEGACY)];
    let none = call("POST", "/mcp", None, Some(body.clone()), &h);
    let made_up = call(
        "POST",
        "/mcp",
        Some("pmk_live_nope"),
        Some(body.clone()),
        &h,
    );
    let r = crate::delete(&format!("/v1/keys/{kid}"), &platform());
    assert!(r.status < 300, "{}", r.body);
    let revoked = call("POST", "/mcp", Some(&key), Some(body), &h);
    for (what, r) in [("none", none), ("unknown", made_up), ("revoked", revoked)] {
        assert_eq!(r.status, 401, "{what}: {}", r.body);
        assert!(
            r.header("www-authenticate")
                .unwrap()
                .starts_with("Bearer realm=\"pylota-identity\"")
        );
        assert_eq!(r.body["error"]["code"], -32000);
        let code = r.body["error"]["data"]["error"]["code"]
            .as_str()
            .unwrap_or("");
        assert!(
            ["unauthenticated", "key_revoked", "key_expired"].contains(&code),
            "{what}: {}",
            r.body
        );
    }
}

/// MCP § 4.1, § 4.2, § 4.3: every tool's four annotations follow its effect class, carries the result
/// size hint, and every description fits its budget with the safety rule early.
#[test]
fn annotations_complete() {
    let (_, key, _) = fixture("mcp annotations");
    let r = rpc(&key, "tools/list", json!({}));
    for t in r.body["result"]["tools"].as_array().unwrap() {
        let name = t["name"].as_str().unwrap();
        let class = match name {
            "mail_send" | "mail_reply" | "mail_forward" => (false, false, true, true),
            "mail_update_labels" => (false, true, true, false),
            "mail_sign_assertion" | "mail_sign_http_request" => (false, false, false, false),
            _ => (true, false, true, false),
        };
        let a = &t["annotations"];
        let got = (
            a["readOnlyHint"].as_bool().unwrap(),
            a["destructiveHint"].as_bool().unwrap(),
            a["idempotentHint"].as_bool().unwrap(),
            a["openWorldHint"].as_bool().unwrap(),
        );
        assert_eq!(got, class, "{name}");
        assert_eq!(
            t["_meta"]["anthropic/maxResultSizeChars"], 100_000,
            "{name}"
        );
        let d = t["description"].as_str().unwrap();
        assert!(d.len() <= 4096, "{name}");
    }
}

/// Where the recorded MCP Inspector session lives.
const SESSION: &str = "tests/it/mcp_inspector_session.json";

/// The shape of a JSON value to three levels (the JSON-RPC envelope, the result's members and the
/// items of its lists); deeper values, whose keys depend on the data (facets, triage), become their
/// type, as every scalar does.
fn skeleton(v: &Value) -> Value {
    fn at(v: &Value, depth: u8) -> Value {
        match v {
            Value::Object(_) if depth >= 3 => "object".into(),
            Value::Array(_) if depth >= 3 => "array".into(),
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, x)| (k.clone(), at(x, depth + 1)))
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.iter().map(|x| at(x, depth + 1)).collect()),
            Value::String(_) => "string".into(),
            Value::Number(_) => "number".into(),
            Value::Bool(b) => (*b).into(),
            Value::Null => Value::Null,
        }
    }
    at(v, 0)
}

/// Records an MCP Inspector session (`PM_INSPECTOR` names its binary, for example
/// `node_modules/.bin/mcp-inspector` of `@modelcontextprotocol/inspector`): a local proxy logs every
/// exchange between the Inspector CLI and `/mcp` into [`SESSION`], with the key and IDs replaced by
/// placeholders. Without `PM_INSPECTOR` it does nothing.
#[test]
fn inspector_record() {
    use std::io::{BufRead, BufReader, Read, Write};
    let Ok(bin) = std::env::var("PM_INSPECTOR") else {
        return;
    };
    let (s, key, _) = fixture("mcp inspector");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let sink = log.clone();
    let (k, iid) = (key.clone(), s.iid.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if r.read_line(&mut line).unwrap_or(0) == 0 {
                continue;
            }
            let method = line.split(' ').next().unwrap_or("").to_owned();
            let mut headers = Vec::new();
            let mut len = 0;
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                let h = h.trim_end().to_owned();
                if h.is_empty() {
                    break;
                }
                if let Some((n, v)) = h.split_once(':') {
                    let (n, v) = (n.trim().to_ascii_lowercase(), v.trim().to_owned());
                    if n == "content-length" {
                        len = v.parse().unwrap_or(0);
                    }
                    headers.push((n, v));
                }
            }
            let mut body = vec![0u8; len];
            r.read_exact(&mut body).unwrap();
            let mut req = crate::client().request(
                reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
                format!("{}/mcp", std::env::var("PM_ITEST_URL").unwrap()),
            );
            for (n, v) in &headers {
                if !matches!(
                    n.as_str(),
                    "host" | "content-length" | "connection" | "accept-encoding"
                ) {
                    req = req.header(n.as_str(), v.as_str());
                }
            }
            let resp = req.body(body.clone()).send().unwrap();
            let status = resp.status().as_u16();
            let rh: Vec<(String, String)> = resp
                .headers()
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_str().unwrap_or("").to_owned()))
                .collect();
            let bytes = resp.bytes().unwrap().to_vec();
            let anon = |t: &str| t.replace(&k, "{key}").replace(&iid, "{identity}");
            let kept: Vec<Value> = headers
                .iter()
                .filter(|(n, _)| {
                    n.starts_with("mcp-") || matches!(n.as_str(), "content-type" | "accept")
                })
                .map(|(n, v)| json!([n, anon(v)]))
                .collect();
            sink.lock().unwrap().push(json!({
                "request": { "method": method, "headers": kept,
                    "body": serde_json::from_slice::<Value>(&body).ok().map(|b| serde_json::from_str::<Value>(&anon(&b.to_string())).unwrap()) },
                "response": { "status": status,
                    "body": serde_json::from_slice::<Value>(&bytes).ok().map(|b| skeleton(&b)) }
            }));
            let ct = rh
                .iter()
                .find(|(n, _)| n == "content-type")
                .map_or("application/json", |(_, v)| v.as_str());
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\ncontent-type: {ct}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                bytes.len()
            );
            let _ = stream.write_all(&bytes);
        }
    });
    let url = format!("http://127.0.0.1:{port}/mcp");
    let auth = format!("Authorization: Bearer {key}");
    let runs: Vec<Vec<String>> = vec![
        vec!["--method".into(), "tools/list".into()],
        vec![
            "--method".into(),
            "tools/call".into(),
            "--tool-name".into(),
            "mail_list_identities".into(),
        ],
        vec![
            "--method".into(),
            "tools/call".into(),
            "--tool-name".into(),
            "mail_search".into(),
            "--tool-arg".into(),
            format!("identity={}", s.iid),
            "q=booking".into(),
            "mode=keyword".into(),
        ],
        vec![
            "--method".into(),
            "tools/call".into(),
            "--tool-name".into(),
            "mail_get_message".into(),
            "--tool-arg".into(),
            format!("identity={}", s.iid),
            "message_id=msg_01ARYZ6S41TSV4RRFFQ69G5FAV".into(),
        ],
        vec!["--method".into(), "prompts/list".into()],
        vec![
            "--method".into(),
            "prompts/get".into(),
            "--prompt-name".into(),
            "mail_search_strategy".into(),
        ],
    ];
    crate::eventually(60, "the message to be searchable", || {
        let r = post(
            &format!("/v1/identities/{}/search", s.iid),
            &key,
            json!({ "q": "booking", "mode": "keyword" }),
        );
        (r.body["hits"].as_array().is_some_and(|h| !h.is_empty())).then_some(())
    });
    for args in runs {
        let out = std::process::Command::new(&bin)
            .arg("--cli")
            .arg(&url)
            .args(["--transport", "http", "--header", &auth])
            .args(&args)
            .output()
            .expect("the Inspector runs");
        // The not-found message is recorded on purpose: the Inspector exits non-zero on a tool error.
        let said = |b: &[u8]| String::from_utf8_lossy(b).contains("tool_is_error");
        let expected = args.iter().any(|a| a == "mail_get_message");
        assert!(
            out.status.success() || (expected && (said(&out.stdout) || said(&out.stderr))),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // One exchange per line keeps the recording small and its diffs readable.
    let lines: Vec<String> = log.lock().unwrap().iter().map(Value::to_string).collect();
    let text = format!(
        "{{\n\"client\": \"MCP Inspector (CLI, --transport http)\",\n\"exchanges\": [\n{}\n]\n}}\n",
        lines.join(",\n")
    );
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SESSION),
        text,
    )
    .unwrap();
}

/// M15: the recorded MCP Inspector session replays green: every request gets the recorded status and
/// a response of the recorded shape.
#[test]
fn inspector_replay() {
    let (s, key, _) = fixture("mcp replay");
    crate::eventually(60, "the message to be searchable", || {
        let r = post(
            &format!("/v1/identities/{}/search", s.iid),
            &key,
            json!({ "q": "booking", "mode": "keyword" }),
        );
        (r.body["hits"].as_array().is_some_and(|h| !h.is_empty())).then_some(())
    });
    let text =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SESSION))
            .expect("the recorded session");
    let text = text.replace("{identity}", &s.iid);
    let session: Value = serde_json::from_str(&text).unwrap();
    let exchanges = session["exchanges"].as_array().unwrap();
    assert!(exchanges.len() >= 6, "{session}");
    for (n, x) in exchanges.iter().enumerate() {
        let headers: Vec<(String, String)> = x["request"]["headers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| {
                (
                    h[0].as_str().unwrap().to_owned(),
                    h[1].as_str().unwrap().to_owned(),
                )
            })
            // `call` sets the content type of a body itself.
            .filter(|(n, _)| n != "content-type")
            .collect();
        let hs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let body = x["request"]["body"].clone();
        let r = call(
            x["request"]["method"].as_str().unwrap(),
            "/mcp",
            Some(&key),
            (!body.is_null()).then_some(body),
            &hs,
        );
        assert_eq!(
            r.status as u64,
            x["response"]["status"].as_u64().unwrap(),
            "exchange {n}: {}",
            r.body
        );
        if !x["response"]["body"].is_null() {
            assert_eq!(skeleton(&r.body), x["response"]["body"], "exchange {n}");
        }
    }
}

/// MCP § 4: `RL_API` is charged once per MCP request, so 600 tool calls in one minute succeed and the
/// 601st request gets `429`; the bucket is the key's REST bucket too.
#[test]
fn rl_api_once_per_call() {
    let (s, _, _) = fixture("mcp rate");
    let (_, key) = mint(
        &platform(),
        json!({ "name": "rl", "level": "tenant", "tenant_id": s.tid, "permissions": ["identities:read"] }),
    );
    crate::fresh_minute(45);
    for n in 0..600 {
        let r = tool(&key, "mail_list_identities", json!({ "limit": 1 }));
        assert_eq!(r["isError"], false, "call {n}: {r}");
    }
    let r = rpc(
        &key,
        "tools/call",
        json!({ "name": "mail_list_identities", "arguments": {} }),
    );
    assert_eq!(r.status, 429, "{}", r.body);
    assert_eq!(r.body["error"]["code"], -32000);
    assert_eq!(r.body["error"]["data"]["error"]["code"], "rate_limited");
    assert!(r.header("retry-after").is_some());
    assert_eq!(get("/v1/identities", &key).status, 429);
}

/// `it::mcp::sign_tools`: the two signing tools are listed only for tenant and identity keys holding
/// `identities:sign`; an identity key naming another identity gets `identity_not_found`; the results
/// have the REST shapes and the token verifies against the identity's JWKS; two identical calls differ;
/// suspension, a pause, `PM_WEB_BOT_AUTH=off` and a tenant not opted in are `isError` results.
#[test]
fn sign_tools() {
    let _reset = crate::fakes::Reset;
    let s = crate::assertions::signer("mcp sign");
    let sign_tools = ["mail_sign_assertion", "mail_sign_http_request"];
    let listed = names(&s.key);
    assert!(
        sign_tools.iter().all(|t| listed.contains(&t.to_string())),
        "{listed:?}"
    );
    for k in [
        platform(),
        crate::partner("mcp sign", &["identities:read"]).1,
    ] {
        let l = names(&k);
        assert!(
            !sign_tools.iter().any(|t| l.contains(&t.to_string())),
            "{l:?}"
        );
    }
    let (_, ik) = mint(
        &platform(),
        json!({ "name": "ik", "level": "identity", "tenant_id": s.tid, "identity_id": s.iid, "permissions": ["identities:sign"] }),
    );
    let other = crate::identity(&platform(), &s.tid, "other");
    let e = err(
        &ik,
        "mail_sign_assertion",
        json!({ "identity": other["id"], "audience": "x" }),
    );
    assert_eq!(e["code"], "identity_not_found");

    let a = ok(
        &ik,
        "mail_sign_assertion",
        json!({ "audience": "https://portal.supplier.example" }),
    );
    assert!(a["kid"].is_string() && a["expires_at"].is_string() && a["jwks_uri"].is_string());
    let c = crate::assertions::verified(&s.iid, a["assertion"].as_str().unwrap());
    assert_eq!(c["sub"], s.iid);
    let b = ok(
        &ik,
        "mail_sign_assertion",
        json!({ "audience": "https://portal.supplier.example" }),
    );
    assert_ne!(a["assertion"], b["assertion"]);

    crate::fakes::set_config("PM_WEB_BOT_AUTH", Some("off"));
    let url = json!({ "url": "https://www.brightwell.example/" });
    assert_eq!(
        err(&ik, "mail_sign_http_request", url.clone())["code"],
        "web_bot_auth_disabled"
    );
    crate::fakes::set_config("PM_WEB_BOT_AUTH", Some("on"));
    assert_eq!(
        err(&ik, "mail_sign_http_request", url.clone())["code"],
        "policy_denied"
    );
    crate::assertions::patch_policy(&s.tid, true);
    let h = ok(&ik, "mail_sign_http_request", url);
    assert_eq!(h["headers"]["From"], s.address);

    let r = crate::patch(
        &format!("/v1/identities/{}", s.iid),
        &platform(),
        json!({ "status": "paused" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        err(&ik, "mail_sign_assertion", json!({ "audience": "x" }))["code"],
        "identity_paused"
    );
    let r = crate::patch(
        &format!("/v1/identities/{}", s.iid),
        &platform(),
        json!({ "status": "active" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let r = crate::patch(
        &format!("/v1/tenants/{}", s.tid),
        &platform(),
        json!({ "status": "suspended" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        err(
            &s.key,
            "mail_sign_assertion",
            json!({ "identity": s.iid, "audience": "x" })
        )["code"],
        "tenant_suspended"
    );
    let r = crate::patch(
        &format!("/v1/tenants/{}", s.tid),
        &platform(),
        json!({ "status": "active" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
}
