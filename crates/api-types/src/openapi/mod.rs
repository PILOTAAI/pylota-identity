//! `openapi.json` (FR-API-1): the component schemas derived from this crate's types, the paths built from
//! [`operations::OPERATIONS`], and `frame.json` (the document's info, servers, tags, security schemes
//! and shared parameter, header and response descriptions, which the types do not model). The contract
//! is `docs/src/reference/openapi.yaml`; [`contract`] compares the two semantically.

pub mod contract;
mod operations;
#[cfg(test)]
mod round_trip;
mod schemas;

use serde_json::{Map, Value, json};

pub use operations::OPERATIONS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    pub const fn as_str(self) -> &'static str {
        match self {
            Method::Get => "get",
            Method::Post => "post",
            Method::Put => "put",
            Method::Patch => "patch",
            Method::Delete => "delete",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum In {
    Path,
    Query,
    Header,
    Cookie,
}

impl In {
    pub const fn as_str(self) -> &'static str {
        match self {
            In::Path => "path",
            In::Query => "query",
            In::Header => "header",
            In::Cookie => "cookie",
        }
    }
}

/// `x-idempotency` ([ADR 0004]).
///
/// [ADR 0004]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/adr/0004-idempotency.md
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Idempotency {
    Required,
    Optional,
    None,
}

impl Idempotency {
    pub const fn as_str(self) -> &'static str {
        match self {
            Idempotency::Required => "required",
            Idempotency::Optional => "optional",
            Idempotency::None => "none",
        }
    }
}

/// The schema of a parameter or a body.
#[derive(Clone, Copy, Debug)]
pub enum Shape {
    /// A component schema of this crate.
    Ref(&'static str),
    /// An inline schema, as JSON.
    Json(&'static str),
    /// No schema.
    Any,
}

impl Shape {
    fn to_json(self) -> Option<Value> {
        match self {
            Shape::Ref(name) => Some(json!({ "$ref": format!("#/components/schemas/{name}") })),
            Shape::Json(s) => Some(serde_json::from_str(s).expect("inline schemas are valid JSON")),
            Shape::Any => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub name: &'static str,
    pub location: In,
    pub required: bool,
    pub schema: Shape,
    /// The shared `components.parameters` entry it comes from.
    pub component: Option<&'static str>,
}

#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub required: bool,
    /// Media type and schema.
    pub content: &'static [(&'static str, Shape)],
}

#[derive(Clone, Copy, Debug)]
pub struct Response {
    /// `200`, `4XX`, …
    pub status: &'static str,
    pub content: &'static [(&'static str, Shape)],
    /// The shared `components.responses` entry it is, for error responses.
    pub component: Option<&'static str>,
}

/// One REST operation.
#[derive(Clone, Copy, Debug)]
pub struct Operation {
    pub id: &'static str,
    pub method: Method,
    /// Relative to `/v1`, or to the host root when `root`.
    pub path: &'static str,
    /// The path item overrides `servers` with the host root (`/health`, `/hooks/*`, `/.well-known/*`).
    pub root: bool,
    pub tag: &'static str,
    pub summary: &'static str,
    /// `x-required-permission`: every one is needed.
    pub permissions: Option<&'static [&'static str]>,
    /// `x-required-permission-any`: any one is enough.
    pub permissions_any: Option<&'static [&'static str]>,
    /// `x-key-levels`.
    pub key_levels: &'static [&'static str],
    /// `x-rate-limit-buckets`.
    pub rate_limit_buckets: &'static [&'static str],
    pub idempotency: Idempotency,
    /// `false` for `x-sdk: false`.
    pub sdk: bool,
    /// The security requirement names; `None` uses the document's default, `Some(&[])` is none.
    pub security: Option<&'static [&'static str]>,
    pub params: &'static [Param],
    pub body: Option<Body>,
    pub responses: &'static [Response],
}

fn content(c: &[(&str, Shape)]) -> Value {
    let mut m = Map::new();
    for (ct, shape) in c {
        let mut media = Map::new();
        if let Some(s) = shape.to_json() {
            media.insert("schema".into(), s);
        }
        m.insert((*ct).to_owned(), Value::Object(media));
    }
    Value::Object(m)
}

impl Operation {
    fn to_json(self, frame: &Value) -> Value {
        let mut op = Map::new();
        op.insert("operationId".into(), json!(self.id));
        op.insert("tags".into(), json!([self.tag]));
        op.insert("summary".into(), json!(self.summary));
        if let Some(p) = self.permissions {
            op.insert("x-required-permission".into(), json!(p));
        }
        if let Some(p) = self.permissions_any {
            op.insert("x-required-permission-any".into(), json!(p));
        }
        if !self.key_levels.is_empty() {
            op.insert("x-key-levels".into(), json!(self.key_levels));
        }
        op.insert(
            "x-rate-limit-buckets".into(),
            json!(self.rate_limit_buckets),
        );
        op.insert("x-idempotency".into(), json!(self.idempotency.as_str()));
        if !self.sdk {
            op.insert("x-sdk".into(), json!(false));
        }
        if let Some(sec) = self.security {
            let reqs: Vec<Value> = sec.iter().map(|s| json!({ *s: [] })).collect();
            op.insert("security".into(), Value::Array(reqs));
        }
        let params: Vec<Value> = self
            .params
            .iter()
            .map(|p| match p.component {
                Some(c) => json!({ "$ref": format!("#/components/parameters/{c}") }),
                None => {
                    let mut v = json!({ "name": p.name, "in": p.location.as_str(), "required": p.required });
                    if let Some(s) = p.schema.to_json() {
                        v["schema"] = s;
                    }
                    v
                }
            })
            .collect();
        if !params.is_empty() {
            op.insert("parameters".into(), Value::Array(params));
        }
        if let Some(b) = self.body {
            op.insert(
                "requestBody".into(),
                json!({ "required": b.required, "content": content(b.content) }),
            );
        }
        let mut responses = Map::new();
        for r in self.responses {
            let v = match r.component {
                Some(c) if frame["components"]["responses"].get(c).is_some() => {
                    json!({ "$ref": format!("#/components/responses/{c}") })
                }
                _ => {
                    let mut v = json!({ "description": "" });
                    if !r.content.is_empty() {
                        v["content"] = content(r.content);
                    }
                    v
                }
            };
            responses.insert(r.status.to_owned(), v);
        }
        op.insert("responses".into(), Value::Object(responses));
        Value::Object(op)
    }
}

/// `openapi.json`: what `GET /openapi.json` serves.
pub fn openapi_json() -> Value {
    let mut doc: Value =
        serde_json::from_str(include_str!("frame.json")).expect("frame.json is valid JSON");
    let mut paths = Map::new();
    let root_servers = json!([{
        "url": "https://{host}",
        "description": "Deployment root (unversioned).",
        "variables": { "host": { "default": "mail.example.com", "description": "The deployment's API host (`PM_API_HOST`)." } }
    }]);
    for op in OPERATIONS {
        let item = paths.entry(op.path.to_owned()).or_insert_with(|| json!({}));
        if op.root {
            item["servers"] = root_servers.clone();
        }
        item[op.method.as_str()] = op.to_json(&doc);
    }
    doc["paths"] = Value::Object(paths);
    let mut schemas = Map::new();
    for (name, schema) in schemas::components() {
        schemas.insert(
            name,
            serde_json::to_value(schema).expect("schemas serialise"),
        );
    }
    doc["components"]["schemas"] = Value::Object(schemas);
    doc
}
