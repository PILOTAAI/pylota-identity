//! `cargo xtask gen-sdk [--check]`: writes `crates/sdk/src/generated.rs`, one `Client` method per REST
//! operation of `api-types`' operation table that the SDK covers ([Rust workspace § 11]), plus the
//! query structs, the `*_stream` pagers of list operations and the `OPERATIONS` coverage table. With
//! `--check` it fails when the checked-in file differs from what the table produces.
//!
//! [Rust workspace § 11]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md

use std::fmt::Write as _;
use std::path::Path;

use pylota_identity_api_types::openapi::{Idempotency, In, OPERATIONS, Operation, Shape};

const OUT: &str = "crates/sdk/src/generated.rs";

const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn",
];

fn snake(id: &str) -> String {
    let mut out = String::new();
    for (i, c) in id.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn pascal(id: &str) -> String {
    let mut c = id.chars();
    c.next()
        .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
        .unwrap_or_default()
}

/// A parameter name as a Rust identifier.
fn ident(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if KEYWORDS.contains(&s.as_str()) {
        format!("r#{s}")
    } else {
        s
    }
}

/// The Rust type of a query or header parameter's schema.
fn param_type(shape: Shape) -> String {
    match shape {
        Shape::Ref(n) => format!("types::{n}"),
        Shape::Any => "serde_json::Value".into(),
        Shape::Json(j) => {
            let v: serde_json::Value = serde_json::from_str(j).unwrap_or_default();
            let ty = match &v["type"] {
                serde_json::Value::Array(a) => a
                    .iter()
                    .find_map(|t| t.as_str().filter(|t| *t != "null"))
                    .unwrap_or("")
                    .to_owned(),
                t => t.as_str().unwrap_or("").to_owned(),
            };
            match ty.as_str() {
                "string" => "String".into(),
                "integer" => "i64".into(),
                "number" => "f64".into(),
                "boolean" => "bool".into(),
                "array" => match v["items"]["$ref"].as_str() {
                    Some(r) => format!("Vec<types::{}>", r.rsplit('/').next().unwrap_or("")),
                    None => "Vec<String>".into(),
                },
                _ => match v["$ref"].as_str() {
                    Some(r) => format!("types::{}", r.rsplit('/').next().unwrap_or("")),
                    None => "serde_json::Value".into(),
                },
            }
        }
    }
}

fn has_dry_run(op: &Operation) -> bool {
    op.params
        .iter()
        .any(|p| p.location == In::Query && p.name == "dry_run")
}

/// What a call answers: the Rust type and the decoder of `client.rs`. An operation with `dry_run`
/// answers its `202` (the real call); its `_dry_run` method answers the `200`.
fn response(op: &Operation) -> (String, &'static str) {
    response_for(op, if has_dry_run(op) { Some("202") } else { None })
}

fn response_for(op: &Operation, status: Option<&str>) -> (String, &'static str) {
    for r in op
        .responses
        .iter()
        .filter(|r| r.status.starts_with('2') && status.is_none_or(|s| r.status == s))
    {
        if let Some((_, s)) = r.content.iter().find(|(ct, _)| *ct == "application/json") {
            return match s {
                Shape::Ref(n) => (format!("types::{n}"), "json"),
                _ => ("serde_json::Value".into(), "json"),
            };
        }
        if r.content.iter().any(|(ct, _)| ct.ends_with("+json")) {
            return ("serde_json::Value".into(), "json");
        }
        if !r.content.is_empty() {
            return ("Raw".into(), "raw");
        }
    }
    ("()".into(), "unit")
}

/// The body argument: its Rust type and how it is sent.
fn body(op: &Operation) -> Option<(String, &'static str)> {
    let b = op.body?;
    if let Some((_, s)) = b.content.iter().find(|(ct, _)| *ct == "application/json") {
        return Some(match s {
            Shape::Ref(n) => (format!("&types::{n}"), "json"),
            _ => ("&serde_json::Value".into(), "json"),
        });
    }
    Some(("&[u8]".into(), "bytes"))
}

/// The query and header parameters that go into the operation's `…Query` struct.
fn query_params(op: &Operation) -> Vec<&pylota_identity_api_types::openapi::Param> {
    op.params
        .iter()
        .filter(|p| {
            (p.location == In::Query && p.name != "dry_run")
                || (p.location == In::Header && !p.name.eq_ignore_ascii_case("Idempotency-Key"))
        })
        .collect()
}

fn is_list(op: &Operation) -> bool {
    op.method.as_str() == "get"
        && op
            .params
            .iter()
            .any(|p| p.location == In::Query && p.name == "cursor")
        && matches!(response(op).1, "json")
        && response(op).0 != "serde_json::Value"
}

/// The generated file.
pub fn generate() -> String {
    let ops: Vec<&Operation> = OPERATIONS.iter().filter(|o| o.sdk).collect();
    let mut s = String::new();
    s.push_str(
        "// @generated by `cargo xtask gen-sdk` from `api-types`' operation table. Do not edit.\n\
         //! One method per REST operation (Rust workspace § 11), the query structs, the pagers of the\n\
         //! list operations and the coverage table.\n\n\
         #![allow(clippy::too_many_arguments)]\n\n\
         use crate::client::{Body, Client, IdempotencyKey, Pages, Raw, Req};\n\
         use crate::{Error, types};\n\n",
    );
    s.push_str("/// Every operation the SDK covers: `(method, path, operationId)`.\n");
    s.push_str("pub const OPERATIONS: &[(&str, &str, &str)] = &[\n");
    for op in &ops {
        let _ = writeln!(
            s,
            "    (\"{}\", \"{}\", \"{}\"),",
            op.method.as_str().to_ascii_uppercase(),
            op.path,
            op.id
        );
    }
    s.push_str("];\n\n");
    // Query structs.
    for op in &ops {
        let q = query_params(op);
        if q.is_empty() {
            continue;
        }
        let _ = writeln!(
            s,
            "/// The query parameters of `{}`.\n#[derive(Clone, Debug, Default)]\npub struct {}Query {{",
            op.id,
            pascal(op.id)
        );
        for p in &q {
            let _ = writeln!(
                s,
                "    pub {}: Option<{}>,",
                ident(p.name),
                param_type(p.schema)
            );
        }
        s.push_str("}\n\n");
        let _ = writeln!(s, "impl {}Query {{", pascal(op.id));
        s.push_str("    fn apply(&self, req: &mut Req) {\n");
        for p in &q {
            let target = if p.location == In::Header {
                "header"
            } else {
                "query"
            };
            let _ = writeln!(
                s,
                "        if let Some(v) = &self.{} {{\n            req.{target}(\"{}\", v);\n        }}",
                ident(p.name),
                p.name
            );
        }
        s.push_str("    }\n}\n\n");
    }
    s.push_str("impl Client {\n");
    for op in &ops {
        let (ret, decode) = response(op);
        method(&mut s, op, &snake(op.id), &ret, decode, false);
        if has_dry_run(op) {
            let (ret, decode) = response_for(op, Some("200"));
            method(
                &mut s,
                op,
                &format!("{}_dry_run", snake(op.id)),
                &ret,
                decode,
                true,
            );
        }
        if is_list(op) {
            pager(&mut s, op, &ret);
        }
    }
    s.push_str("}\n");
    s
}

/// The path parameters, the path expression and its `format!` arguments, and whether the call
/// authenticates.
fn path_parts(op: &Operation) -> (Vec<&'static str>, String, String, bool) {
    let paths: Vec<&str> = op
        .params
        .iter()
        .filter(|p| p.location == In::Path)
        .map(|p| p.name)
        .collect();
    let prefix = if op.root { "" } else { "/v1" };
    let mut path_expr = format!("{prefix}{}", op.path);
    for p in &paths {
        path_expr = path_expr.replace(&format!("{{{p}}}"), "{}");
    }
    let fmt_args: String = paths
        .iter()
        .map(|p| format!(", crate::client::segment({})", ident(p)))
        .collect();
    let auth = !op.security.is_some_and(|s| s.is_empty());
    // A path without parameters needs no `format!`.
    let path_expr = if paths.is_empty() {
        format!("\"{path_expr}\".to_owned()")
    } else {
        format!("format!(\"{path_expr}\"{fmt_args})")
    };
    (paths, path_expr, String::new(), auth)
}

/// One method: `dry_run` adds `dry_run=true` and documents the dry run.
fn method(s: &mut String, op: &Operation, name: &str, ret: &str, decode: &str, dry_run: bool) {
    let (paths, path_expr, fmt_args, auth) = path_parts(op);
    let mut args: Vec<String> = paths
        .iter()
        .map(|p| format!("{}: &str", ident(p)))
        .collect();
    let b = body(op);
    if let Some((ty, _)) = &b {
        args.push(format!("body: {ty}"));
    }
    let has_query = !query_params(op).is_empty();
    if has_query {
        args.push(format!("query: &{}Query", pascal(op.id)));
    }
    let idem = if dry_run {
        Idempotency::None
    } else {
        op.idempotency
    };
    match idem {
        Idempotency::Required => args.push("idempotency_key: &IdempotencyKey".into()),
        Idempotency::Optional => args.push("idempotency_key: Option<&IdempotencyKey>".into()),
        Idempotency::None => {}
    }
    let prefix = if op.root { "" } else { "/v1" };
    let what = if dry_run {
        "Checks without sending: "
    } else {
        ""
    };
    let _ = writeln!(
        s,
        "    /// {what}{} (`{} {prefix}{}{}`, `{}`).",
        op.summary.trim_end_matches('.'),
        op.method.as_str().to_ascii_uppercase(),
        op.path,
        if dry_run { "?dry_run=true" } else { "" },
        op.id
    );
    let _ = writeln!(
        s,
        "    pub async fn {name}(&self, {}) -> Result<{ret}, Error> {{",
        args.join(", ")
    );
    let needs_mut = has_query || b.is_some() || dry_run || idem != Idempotency::None;
    let _ = writeln!(
        s,
        "        let {}req = Req::new(\"{}\", {path_expr}{fmt_args}, {auth});",
        if needs_mut { "mut " } else { "" },
        op.method.as_str().to_ascii_uppercase()
    );
    if has_query {
        s.push_str("        query.apply(&mut req);\n");
    }
    if dry_run {
        s.push_str("        req.query(\"dry_run\", &true);\n");
    }
    match &b {
        Some((_, "json")) => s.push_str("        req.body = Body::json(body)?;\n"),
        Some(_) => s.push_str("        req.body = Body::Bytes(body.to_vec());\n"),
        None => {}
    }
    match idem {
        Idempotency::Required => {
            s.push_str("        req.idempotency(Some(idempotency_key), false);\n")
        }
        Idempotency::Optional => s.push_str(&format!(
            "        req.idempotency(idempotency_key, {});\n",
            op.method.as_str() == "post"
        )),
        Idempotency::None => {}
    }
    let _ = writeln!(s, "        self.send(req).await?.{decode}()\n    }}\n");
}

/// The pager of a list operation.
fn pager(s: &mut String, op: &Operation, ret: &str) {
    let (paths, path_expr, fmt_args, auth) = path_parts(op);
    let name = snake(op.id);
    let mut args: Vec<String> = paths
        .iter()
        .map(|p| format!("{}: &str", ident(p)))
        .collect();
    let has_query = !query_params(op).is_empty();
    if has_query {
        args.push(format!("query: &{}Query", pascal(op.id)));
    }
    let _ = writeln!(
        s,
        "    /// Every page of `{name}`, following `next_cursor`.\n    pub fn {name}_stream(&self, {}) -> Pages<'_, {ret}> {{",
        args.join(", ")
    );
    let _ = writeln!(
        s,
        "        let {}req = Req::new(\"GET\", {path_expr}{fmt_args}, {auth});",
        if has_query { "mut " } else { "" }
    );
    if has_query {
        s.push_str("        query.apply(&mut req);\n");
    }
    s.push_str("        Pages::new(self, req)\n    }\n\n");
}

pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let text = generate();
    let path = root.join(OUT);
    if check {
        let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
        // The checked-in file is rustfmt's output of the generated text.
        if format(&text)? != on_disk {
            return Err(format!("{OUT} is stale: run `cargo xtask gen-sdk`"));
        }
        println!("gen-sdk: {OUT} is current");
        return Ok(());
    }
    std::fs::write(&path, format(&text)?).map_err(|e| e.to_string())?;
    println!("gen-sdk: wrote {OUT}");
    Ok(())
}

/// The text after `rustfmt --edition 2024`.
fn format(text: &str) -> Result<String, String> {
    use std::io::Write as _;
    let mut child = std::process::Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("rustfmt: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("rustfmt: no stdin")?
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("rustfmt: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(snake("listIdentities"), "list_identities");
        assert_eq!(snake("sendMessage"), "send_message");
        assert_eq!(pascal("listThreads"), "ListThreads");
        assert_eq!(ident("type"), "r#type");
        assert_eq!(ident("If-Match"), "if_match");
    }
}
