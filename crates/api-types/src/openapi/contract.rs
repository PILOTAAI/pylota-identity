//! The semantic comparison of two OpenAPI documents (build plan M3, `xtask openapi`): paths, methods,
//! operation metadata, parameters, bodies, responses, schemas, enums and required fields. Descriptions,
//! examples, defaults, formats and length or pattern constraints are documentation and validation detail
//! and are not compared; `if`, `not`, `discriminator` and constraint-only `anyOf` (a list of `required`
//! alternatives) are not either.
//!
//! Both documents are reduced to one normal form before comparing: references to shared parameters,
//! responses and request bodies are resolved; `type: [T, "null"]`, `oneOf: [X, {type: null}]` and a
//! `null` enum member all become `nullable: true`; `allOf` with a
//! single member is that member; `anyOf` is `oneOf`, and union members are compared as a set. A `const`
//! is a one-value `enum`, and the values of an `enum` fix its type.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

const SCHEMA_KEYS_DROPPED: &[&str] = &[
    "description",
    "title",
    "examples",
    "example",
    "default",
    "format",
    "pattern",
    "maxLength",
    "minLength",
    "maxItems",
    "minItems",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "uniqueItems",
    "readOnly",
    "writeOnly",
    "deprecated",
    "contentMediaType",
    "contentEncoding",
    "propertyNames",
    "if",
    "then",
    "else",
    "not",
    "discriminator",
    "$comment",
    "externalDocs",
    "minProperties",
    "maxProperties",
    "unevaluatedProperties",
];

fn resolve<'a>(doc: &'a Value, v: &'a Value, kind: &str) -> &'a Value {
    let mut v = v;
    for _ in 0..8 {
        match v.get("$ref").and_then(Value::as_str) {
            Some(r) if r.starts_with(&format!("#/components/{kind}/")) => {
                let name = r.rsplit('/').next().unwrap_or("");
                v = &doc["components"][kind][name];
            }
            _ => break,
        }
    }
    v
}

/// The normal form of a schema.
pub fn schema(v: &Value) -> Value {
    let Some(obj) = v.as_object() else {
        return v.clone();
    };
    let mut o: Map<String, Value> = obj
        .iter()
        .filter(|(k, _)| !SCHEMA_KEYS_DROPPED.contains(&k.as_str()) && !k.starts_with("x-"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut nullable = false;

    if let Some(Value::Array(types)) = o.get("type").cloned()
        && ["string", "number", "boolean", "object", "array"]
            .iter()
            .all(|t| types.iter().any(|x| x == t))
    {
        // Every JSON type is "any value", the same as no type at all.
        o.remove("type");
    }
    if let Some(Value::Array(types)) = o.get("type").cloned() {
        let rest: Vec<Value> = types.iter().filter(|t| *t != "null").cloned().collect();
        nullable |= rest.len() < types.len();
        match rest.len() {
            0 => {
                o.insert("type".into(), json!("null"));
            }
            1 => {
                o.insert("type".into(), rest[0].clone());
            }
            _ => {
                o.insert("type".into(), Value::Array(rest));
            }
        }
    }
    if let Some(c) = o.remove("const") {
        o.insert("enum".into(), json!([c]));
    }
    if let Some(Value::Array(e)) = o.get("enum").cloned() {
        let mut rest: Vec<Value> = e.iter().filter(|x| !x.is_null()).cloned().collect();
        nullable |= rest.len() < e.len();
        rest.sort_by_key(Value::to_string);
        rest.dedup();
        o.insert("enum".into(), Value::Array(rest));
        // The values fix the type; the contract often leaves `type` out.
        o.remove("type");
    }
    // A constraint-only anyOf or oneOf (alternatives that each list required properties) is validation.
    for key in ["anyOf", "oneOf"] {
        if let Some(Value::Array(members)) = o.get(key)
            && members.iter().all(|m| {
                m.as_object()
                    .is_some_and(|m| m.keys().all(|k| k == "required"))
            })
        {
            o.remove(key);
        }
    }
    // allOf members that only state conditions (if/then/else, not) are validation too.
    if let Some(Value::Array(members)) = o.get("allOf").cloned() {
        let kept: Vec<Value> = members
            .into_iter()
            .filter(|m| {
                !m.as_object().is_some_and(|m| {
                    m.keys()
                        .all(|k| ["if", "then", "else", "not", "description"].contains(&k.as_str()))
                })
            })
            .collect();
        if kept.is_empty() {
            o.remove("allOf");
        } else {
            o.insert("allOf".into(), Value::Array(kept));
        }
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(Value::Array(members)) = o.remove(key) {
            let mut rest = Vec::new();
            for m in &members {
                if m.get("type").and_then(Value::as_str) == Some("null")
                    && m.as_object().is_some_and(|m| m.len() <= 2)
                {
                    nullable = true;
                } else {
                    rest.push(schema(m));
                }
            }
            if rest.len() == 1 {
                if let Value::Object(inner) = rest.remove(0) {
                    for (k, v) in inner {
                        if k == "nullable" {
                            nullable |= v == json!(true);
                        } else {
                            o.entry(k).or_insert(v);
                        }
                    }
                }
            } else if !rest.is_empty() {
                let set: BTreeSet<String> = rest.iter().map(Value::to_string).collect();
                let members: Vec<Value> = set
                    .iter()
                    .map(|s| serde_json::from_str(s).expect("json"))
                    .collect();
                o.insert("oneOf".into(), Value::Array(members));
            }
        }
    }
    if let Some(Value::Array(members)) = o.remove("allOf") {
        let members: Vec<Value> = members.iter().map(schema).collect();
        if members.len() == 1 {
            if let Value::Object(inner) = members.into_iter().next().expect("one") {
                for (k, v) in inner {
                    o.entry(k).or_insert(v);
                }
            }
        } else {
            let set: BTreeSet<String> = members.iter().map(Value::to_string).collect();
            o.insert(
                "allOf".into(),
                json!(
                    set.iter()
                        .map(|s| serde_json::from_str::<Value>(s).expect("json"))
                        .collect::<Vec<_>>()
                ),
            );
        }
    }
    if let Some(Value::Object(props)) = o.get("properties").cloned() {
        let props: Map<String, Value> = props.iter().map(|(k, v)| (k.clone(), schema(v))).collect();
        o.insert("properties".into(), Value::Object(props));
        o.insert("type".into(), json!("object"));
    }
    if let Some(Value::Array(req)) = o.remove("required") {
        let set: BTreeSet<String> = req
            .iter()
            .filter_map(|r| r.as_str().map(str::to_owned))
            .collect();
        if !set.is_empty() {
            o.insert("required".into(), json!(set));
        }
    }
    if let Some(items) = o.get("items").cloned() {
        o.insert("items".into(), schema(&items));
    }
    match o.get("additionalProperties").cloned() {
        Some(Value::Object(ap)) if !ap.is_empty() => {
            o.insert("additionalProperties".into(), schema(&Value::Object(ap)));
        }
        Some(_) => {
            o.remove("additionalProperties");
        }
        None => {}
    }
    // `{type: object}` with nothing else and an empty schema both mean "any object"; keep the type.
    if nullable {
        o.insert("nullable".into(), json!(true));
    }
    Value::Object(o)
}

fn content(doc: &Value, c: &Value) -> Value {
    let mut out = Map::new();
    if let Some(c) = c.as_object() {
        for (ct, media) in c {
            out.insert(
                ct.clone(),
                media.get("schema").map(schema).unwrap_or(Value::Null),
            );
        }
    }
    let _ = doc;
    Value::Object(out)
}

/// The normal form of every operation, keyed `METHOD path`.
pub fn operations(doc: &Value) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let Some(paths) = doc["paths"].as_object() else {
        return out;
    };
    for (path, item) in paths {
        let root = item.get("servers").is_some();
        let shared: Vec<Value> = item["parameters"].as_array().cloned().unwrap_or_default();
        for method in ["get", "post", "put", "patch", "delete"] {
            let Some(op) = item.get(method) else { continue };
            let mut params: BTreeSet<String> = BTreeSet::new();
            for p in shared
                .iter()
                .chain(op["parameters"].as_array().into_iter().flatten())
            {
                let p = resolve(doc, p, "parameters");
                params.insert(format!(
                    "{} {} {}",
                    p["in"].as_str().unwrap_or(""),
                    p["name"].as_str().unwrap_or(""),
                    if p["required"].as_bool() == Some(true) {
                        "required"
                    } else {
                        "optional"
                    }
                ));
            }
            let body = op.get("requestBody").map(|b| {
                let b = resolve(doc, b, "requestBodies");
                json!({ "required": b["required"].as_bool() == Some(true), "content": content(doc, &b["content"]) })
            });
            let mut responses = Map::new();
            if let Some(rs) = op["responses"].as_object() {
                for (status, r) in rs {
                    let r = resolve(doc, r, "responses");
                    responses.insert(status.clone(), content(doc, &r["content"]));
                }
            }
            let security = op.get("security").map(|s| {
                let names: BTreeSet<String> = s
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_object)
                    .flat_map(|m| m.keys().cloned())
                    .collect();
                json!(names)
            });
            out.insert(
                format!("{} {path}", method.to_uppercase()),
                json!({
                    "operationId": op["operationId"],
                    "tags": op["tags"],
                    "root": root,
                    "permissions": op.get("x-required-permission"),
                    "permissions_any": op.get("x-required-permission-any"),
                    "key_levels": op.get("x-key-levels"),
                    "rate_limit_buckets": op.get("x-rate-limit-buckets"),
                    "idempotency": op.get("x-idempotency"),
                    "sdk": op.get("x-sdk").and_then(Value::as_bool).unwrap_or(true),
                    "security": security,
                    "parameters": params,
                    "requestBody": body,
                    "responses": responses,
                }),
            );
        }
    }
    out
}

/// The normal form of every component schema.
pub fn schemas(doc: &Value) -> BTreeMap<String, Value> {
    doc["components"]["schemas"]
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), schema(v))).collect())
        .unwrap_or_default()
}

fn diff(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for k in keys {
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => diff(&format!("{path}.{k}"), p, q, out),
                    (Some(p), None) => {
                        out.push(format!("{path}.{k}: only in the contract: {}", short(p)))
                    }
                    (None, Some(q)) => {
                        out.push(format!("{path}.{k}: only in the types: {}", short(q)))
                    }
                    (None, None) => {}
                }
            }
        }
        _ if a == b => {}
        _ => out.push(format!(
            "{path}: contract {} ≠ types {}",
            short(a),
            short(b)
        )),
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 160 {
        format!(
            "{}…",
            &s[..s.char_indices().nth(160).map_or(s.len(), |(i, _)| i)]
        )
    } else {
        s
    }
}

/// Every semantic difference between the contract and the generated document; empty when they agree.
pub fn compare(contract: &Value, generated: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let (oc, og) = (operations(contract), operations(generated));
    let keys: BTreeSet<&String> = oc.keys().chain(og.keys()).collect();
    for k in keys {
        match (oc.get(k), og.get(k)) {
            (Some(a), Some(b)) => diff(k, a, b, &mut out),
            (Some(_), None) => out.push(format!("{k}: operation missing from the types")),
            (None, Some(_)) => out.push(format!("{k}: operation not in the contract")),
            (None, None) => {}
        }
    }
    let (sc, sg) = (schemas(contract), schemas(generated));
    let keys: BTreeSet<&String> = sc.keys().chain(sg.keys()).collect();
    for k in keys {
        match (sc.get(k), sg.get(k)) {
            (Some(a), Some(b)) => diff(&format!("schemas.{k}"), a, b, &mut out),
            (Some(_), None) => out.push(format!("schemas.{k}: missing from the types")),
            (None, Some(_)) => out.push(format!("schemas.{k}: not in the contract")),
            (None, None) => {}
        }
    }
    if contract["openapi"] != generated["openapi"] {
        out.push(format!(
            "openapi: {} ≠ {}",
            contract["openapi"], generated["openapi"]
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> Value {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/src/reference/openapi.yaml"
        ))
        .expect("openapi.yaml");
        yaml_serde::from_str(&text).expect("openapi.yaml parses")
    }

    /// The generated `openapi.json` equals `openapi.yaml` semantically (build plan M3).
    /// Covers: FR-API-1
    #[test]
    fn generated_matches_contract() {
        let diffs = compare(&contract(), &super::super::openapi_json());
        assert!(
            diffs.is_empty(),
            "{} differences:\n{}",
            diffs.len(),
            diffs.join("\n")
        );
    }

    /// The document is OpenAPI 3.1.0 with every operation of the contract.
    /// Covers: FR-API-1
    #[test]
    fn generated_is_openapi_3_1() {
        let doc = super::super::openapi_json();
        assert_eq!(doc["openapi"], "3.1.0");
        assert_eq!(operations(&doc).len(), 287);
        assert_eq!(super::super::OPERATIONS.len(), 287);
    }

    /// The normal form makes the three spellings of a nullable string equal, and a single-member allOf
    /// its member.
    #[test]
    fn normal_form() {
        let a = schema(&json!({ "type": ["string", "null"], "description": "x" }));
        let b = schema(&json!({ "oneOf": [{ "type": "string" }, { "type": "null" }] }));
        let c = schema(&json!({ "type": "string", "enum": ["a", null] }));
        assert_eq!(a, b);
        assert_eq!(c, json!({ "enum": ["a"], "nullable": true }));
        assert_eq!(schema(&c), c, "the normal form is stable");
        assert_eq!(
            schema(&json!({ "allOf": [{ "$ref": "#/x" }], "default": 1 })),
            json!({ "$ref": "#/x" })
        );
        assert_eq!(schema(&json!({ "const": "m" })), json!({ "enum": ["m"] }));
    }
}
