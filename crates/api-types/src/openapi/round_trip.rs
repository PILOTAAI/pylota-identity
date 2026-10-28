//! Serde round trips (build plan M3): every component schema's Rust type deserialises and re-serialises,
//! unchanged, the examples of `openapi.yaml` (schema `examples`, and the `example`s of request and
//! response bodies, with every object nested in them), and two instances built from the contract's own
//! schema: one with every property, one with only the required ones. The examples of `api.md` are not
//! used: they elide fields (`"…"`).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

use super::schemas::round_trip;

fn contract() -> Value {
    let text = include_str!("../../../../docs/src/reference/openapi.yaml");
    yaml_serde::from_str(text).expect("openapi.yaml parses")
}

fn ref_name(v: &Value) -> Option<&str> {
    v.get("$ref")?.as_str()?.rsplit('/').next()
}

/// Records `value` under every component schema it instantiates, walking `schema` beside it.
fn collect(
    doc: &Value,
    schema: &Value,
    value: &Value,
    depth: usize,
    out: &mut BTreeMap<String, Vec<Value>>,
) {
    if depth > 24 {
        return;
    }
    if let Some(name) = ref_name(schema) {
        out.entry(name.to_owned()).or_default().push(value.clone());
        collect(
            doc,
            &doc["components"]["schemas"][name],
            value,
            depth + 1,
            out,
        );
        return;
    }
    if let Some(all) = schema.get("allOf").and_then(Value::as_array) {
        for s in all {
            collect(doc, s, value, depth + 1, out);
        }
    }
    match value {
        Value::Object(map) => {
            if let Some(props) = schema.get("properties").and_then(Value::as_object) {
                for (k, v) in map {
                    if let Some(s) = props.get(k) {
                        collect(doc, s, v, depth + 1, out);
                    }
                }
            }
            if let Some(ap) = schema.get("additionalProperties").filter(|a| a.is_object()) {
                for v in map.values() {
                    collect(doc, ap, v, depth + 1, out);
                }
            }
        }
        Value::Array(items) => {
            if let Some(s) = schema.get("items") {
                for v in items {
                    collect(doc, s, v, depth + 1, out);
                }
            }
        }
        _ => {}
    }
    // A nullable or one-of-two member: walk the referenced member when the value is not null.
    if !value.is_null()
        && let Some(members) = schema
            .get("oneOf")
            .or_else(|| schema.get("anyOf"))
            .and_then(Value::as_array)
    {
        let refs: Vec<&Value> = members.iter().filter(|m| m.get("$ref").is_some()).collect();
        if refs.len() == 1 && members.len() == 2 && members.iter().any(|m| m["type"] == "null") {
            collect(doc, refs[0], value, depth + 1, out);
        }
    }
}

/// Every example of the contract, by component schema.
fn examples(doc: &Value) -> BTreeMap<String, Vec<Value>> {
    let mut out = BTreeMap::new();
    for (name, s) in doc["components"]["schemas"].as_object().expect("schemas") {
        let r = json!({ "$ref": format!("#/components/schemas/{name}") });
        for e in s["examples"].as_array().into_iter().flatten() {
            collect(doc, &r, e, 0, &mut out);
        }
    }
    let mut media = Vec::new();
    for item in doc["paths"].as_object().expect("paths").values() {
        for op in item.as_object().into_iter().flat_map(|m| m.values()) {
            media.extend(
                op["requestBody"]["content"]
                    .as_object()
                    .into_iter()
                    .flat_map(|c| c.values())
                    .cloned(),
            );
            for r in op["responses"]
                .as_object()
                .into_iter()
                .flat_map(|m| m.values())
            {
                media.extend(
                    r["content"]
                        .as_object()
                        .into_iter()
                        .flat_map(|c| c.values())
                        .cloned(),
                );
            }
        }
    }
    for r in doc["components"]["responses"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
    {
        media.extend(
            r["content"]
                .as_object()
                .into_iter()
                .flat_map(|c| c.values())
                .cloned(),
        );
    }
    for m in media {
        let schema = &m["schema"];
        if let Some(e) = m.get("example") {
            collect(doc, schema, e, 0, &mut out);
        }
        for e in m["examples"]
            .as_object()
            .into_iter()
            .flat_map(|x| x.values())
        {
            if let Some(v) = e.get("value") {
                collect(doc, schema, v, 0, &mut out);
            }
        }
    }
    out
}

/// An instance of `schema` built from the contract: with every property (`full`) or only the required
/// ones. Strings take their first enum value or `"x"`; dates a fixed RFC 3339 time.
fn instance(doc: &Value, schema: &Value, full: bool, depth: usize) -> Value {
    if let Some(name) = ref_name(schema) {
        return instance(doc, &doc["components"]["schemas"][name], full, depth + 1);
    }
    if let Some(c) = schema.get("const") {
        return c.clone();
    }
    if let Some(e) = schema
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|e| e.iter().find(|v| !v.is_null()))
    {
        return e.clone();
    }
    if let Some(all) = schema.get("allOf").and_then(Value::as_array) {
        if all.len() == 1 && schema.get("properties").is_none() {
            return instance(doc, &all[0], full, depth + 1);
        }
        let mut obj = Map::new();
        for s in all {
            if let Value::Object(m) = instance(doc, s, full, depth + 1) {
                obj.extend(m);
            }
        }
        if let Some(Value::Object(own)) = schema.get("properties").map(|_| {
            let mut s = schema.clone();
            s.as_object_mut().map(|m| m.remove("allOf"));
            instance(doc, &s, full, depth + 1)
        }) {
            obj.extend(own);
        }
        return Value::Object(obj);
    }
    if let Some(members) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
    {
        let members: Vec<&Value> = members
            .iter()
            .filter(|m| {
                m["type"] != "null"
                    && !m
                        .as_object()
                        .is_some_and(|o| o.keys().all(|k| k == "required"))
            })
            .collect();
        if let Some(first) = members.first() {
            return instance(doc, first, full, depth + 1);
        }
    }
    let ty = match &schema["type"] {
        Value::Array(types) => types
            .iter()
            .find(|t| *t != "null")
            .cloned()
            .unwrap_or(Value::Null),
        t => t.clone(),
    };
    match ty.as_str() {
        Some("string") => match schema["format"].as_str() {
            Some("date-time") => json!("2026-10-27T22:00:00Z"),
            Some("date") => json!("2026-10-27"),
            _ => json!("x"),
        },
        Some("integer") => json!(1),
        Some("number") => json!(1.5),
        Some("boolean") => json!(true),
        Some("null") => Value::Null,
        Some("array") => {
            if depth > 10 || (!full && schema["minItems"].as_u64().unwrap_or(0) == 0) {
                json!([])
            } else {
                json!([instance(doc, &schema["items"], full, depth + 1)])
            }
        }
        _ => {
            let mut obj = Map::new();
            let required: BTreeSet<&str> = schema["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            for (k, s) in schema["properties"].as_object().into_iter().flatten() {
                if required.contains(k.as_str()) || (full && depth < 10) {
                    obj.insert(k.clone(), instance(doc, s, full, depth + 1));
                }
            }
            if let Some(ap) = schema.get("additionalProperties").filter(|a| a.is_object())
                && full
                && depth < 10
            {
                obj.insert("X-Extra".into(), instance(doc, ap, full, depth + 1));
            }
            Value::Object(obj)
        }
    }
}

/// JSON numbers compared by value: a `number` field reads `0` and writes `0.0`.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

fn check(name: &str, value: &Value, origin: &str, failures: &mut Vec<String>) {
    match round_trip(name, value) {
        Some(Ok(back)) if same(&back, value) => {}
        Some(Ok(back)) => failures.push(format!("{name} ({origin}): {value} came back as {back}")),
        Some(Err(e)) => failures.push(format!("{name} ({origin}): {e}: {value}")),
        None => failures.push(format!("{name}: no Rust type")),
    }
}

/// Every example of `openapi.yaml` round-trips through its schema's type.
/// Covers: FR-API-1
#[test]
fn contract_examples_round_trip() {
    let doc = contract();
    let ex = examples(&doc);
    assert!(ex.len() >= 200, "{} schemas have examples", ex.len());
    let mut failures = Vec::new();
    for (name, values) in &ex {
        for v in values {
            check(name, v, "example", &mut failures);
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Every component schema's type round-trips an instance with every property and one with only the
/// required ones, both built from the contract.
/// Covers: FR-API-1
#[test]
fn every_object_round_trips() {
    let doc = contract();
    let mut failures = Vec::new();
    for name in doc["components"]["schemas"]
        .as_object()
        .expect("schemas")
        .keys()
    {
        let r = json!({ "$ref": format!("#/components/schemas/{name}") });
        for full in [true, false] {
            let v = instance(&doc, &r, full, 0);
            check(
                name,
                &v,
                if full {
                    "every property"
                } else {
                    "required only"
                },
                &mut failures,
            );
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
