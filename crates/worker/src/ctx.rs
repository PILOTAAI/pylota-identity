//! One request as the handlers see it: the platform, the request ID, the matched route, the resolved key
//! and the parsed parts of the request; and what a handler answers.

use std::rc::Rc;

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::cursor;
use pylota_identity_platform::Platform;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::auth::KeyScope;
use crate::errors::ApiError;
use crate::keyring::{self, Purpose};
use crate::router::Matched;

/// The largest JSON body a route of M5 takes.
pub const MAX_BODY: usize = 1 << 20;

pub struct Ctx<P: Platform> {
    pub p: Rc<P>,
    pub request_id: String,
    pub now: i64,
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub route: Matched,
    pub key: Option<KeyScope>,
}

/// A handler's answer: status, JSON body (or `None`), and extra headers.
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Option<Value>,
    /// A body that is not JSON (`security.txt`): content type and bytes.
    pub raw: Option<(&'static str, Vec<u8>)>,
    pub headers: Vec<(String, String)>,
}

impl Reply {
    pub fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            body: Some(body),
            raw: None,
            headers: Vec::new(),
        }
    }

    pub fn no_content() -> Self {
        Self {
            status: 204,
            body: None,
            raw: None,
            headers: Vec::new(),
        }
    }
}

impl<P: Platform> Ctx<P> {
    /// The calling key; every route but the public ones has one.
    pub fn key(&self) -> &KeyScope {
        self.key.as_ref().expect("an authenticated route")
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn query(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The body as JSON (`{}` when absent).
    pub fn json(&self) -> Result<Value, ApiError> {
        match &self.body {
            None => Ok(json!({})),
            Some(b) if b.len() > MAX_BODY => Err(ApiError::new(
                ErrorCode::PayloadTooLarge,
                "The body is too large.",
            )),
            Some(b) if b.iter().all(u8::is_ascii_whitespace) => Ok(json!({})),
            Some(b) => serde_json::from_slice(b)
                .map_err(|e| ApiError::invalid("", format!("The body is not valid JSON: {e}"))),
        }
    }

    /// The body as `T`; a type error names its field.
    pub fn parse<T: DeserializeOwned>(&self) -> Result<(T, Value), ApiError> {
        let v = self.json()?;
        if !v.is_object() {
            return Err(ApiError::invalid("", "The body must be a JSON object."));
        }
        let t = serde_json::from_value(v.clone()).map_err(|e| {
            let msg = e.to_string();
            let path = msg.split('`').nth(1).unwrap_or("").to_owned();
            ApiError::invalid(&path, msg)
        })?;
        Ok((t, v))
    }

    /// `limit` (default 25, at most 100).
    pub fn limit(&self) -> Result<u32, ApiError> {
        match self.query("limit") {
            None => Ok(25),
            Some(l) => match l.parse::<u32>() {
                Ok(n) if (1..=100).contains(&n) => Ok(n),
                _ => Err(ApiError::invalid("limit", "limit is 1 to 100.")),
            },
        }
    }

    /// The filter hash of a list: its path parameters, the named filters in name order, and the key.
    pub fn filter_hash(&self, filters: &[&str]) -> String {
        let mut parts: Vec<String> = self
            .route
            .params
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect();
        let mut names = filters.to_vec();
        names.sort_unstable();
        for n in names {
            parts.push(format!("{n}={}", self.query(n).unwrap_or("")));
        }
        parts.push(self.key().key_id.clone());
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        cursor::filter_hash(&refs)
    }

    /// The keyset after which a list continues, from `cursor`.
    pub async fn after(&self, filters: &[&str]) -> Result<Option<(i64, String)>, ApiError> {
        let Some(token) = self.query("cursor") else {
            return Ok(None);
        };
        let ks = keyring::keys(self.p.as_ref(), Purpose::Cursor).await?;
        let refs: Vec<(&str, &[u8; 32])> = ks.iter().map(|k| (k.kid.as_str(), &k.secret)).collect();
        cursor::decode(
            token,
            &refs,
            self.route.spec.operation_id,
            &self.filter_hash(filters),
            self.now,
        )
        .map(Some)
        .map_err(|e| match e {
            cursor::CursorError::Expired => {
                ApiError::new(ErrorCode::CursorExpired, "The cursor has expired.")
            }
            cursor::CursorError::Invalid => {
                ApiError::invalid("cursor", "The cursor is not valid for this list.")
            }
        })
    }

    /// `{ data, next_cursor }` from `limit + 1` rows ordered newest first; `key_of` gives a row's keyset.
    pub async fn page<T>(
        &self,
        mut rows: Vec<T>,
        limit: u32,
        filters: &[&str],
        key_of: impl Fn(&T) -> (i64, String),
        render: impl Fn(&T) -> Value,
    ) -> Result<Reply, ApiError> {
        let more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next = match (more, rows.last()) {
            (true, Some(last)) => {
                let k = keyring::current(self.p.as_ref(), Purpose::Cursor).await?;
                let (created, id) = key_of(last);
                Value::from(cursor::encode(
                    &k.kid,
                    &k.secret,
                    self.route.spec.operation_id,
                    &self.filter_hash(filters),
                    created,
                    &id,
                    self.now,
                ))
            }
            _ => Value::Null,
        };
        Ok(Reply::json(
            200,
            json!({ "data": rows.iter().map(render).collect::<Vec<_>>(), "next_cursor": next }),
        ))
    }
}
