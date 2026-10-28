//! Contacts search ([Search § 12.1]): the identity's correspondents ranked by match, then
//! `inbound + 2 · outbound`, then recency. Contacts known only from quarantined mail need
//! `quarantine:review` ([F7]).
//!
//! [Search § 12.1]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md
//! [F7]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/edge-cases.md

use pylota_identity_core::days::rfc3339;
use serde_json::{Value, json};

/// `q` as the SQL compares it: NFKC, lower case, at most 100 characters; `None` when longer.
pub fn fold_q(q: &str) -> Option<String> {
    let f = pylota_identity_core::query::fold_nfkc(q.trim());
    (f.chars().count() <= 100).then_some(f)
}

/// The keyset of a row: `(match_rank, weight, last_seen_at, address)`.
pub fn boundary(row: &Value) -> (i64, i64, i64, String) {
    (
        row["match_rank"].as_i64().unwrap_or(0),
        row["weight"].as_i64().unwrap_or(0),
        row["last_seen_at"].as_i64().unwrap_or(0),
        row["address"].as_str().unwrap_or("").to_owned(),
    )
}

/// The cursor key string of a boundary (the list cursor carries one string).
pub fn encode_boundary(b: &(i64, i64, i64, String)) -> String {
    json!([b.0, b.1, b.2, b.3]).to_string()
}

pub fn decode_boundary(s: &str) -> Option<(i64, i64, i64, String)> {
    let v: Value = serde_json::from_str(s).ok()?;
    Some((
        v[0].as_i64()?,
        v[1].as_i64()?,
        v[2].as_i64()?,
        v[3].as_str()?.to_owned(),
    ))
}

/// A `Contact` of the API.
pub fn render(row: &Value) -> Value {
    let ms = |k: &str| rfc3339(row[k].as_i64().unwrap_or(0));
    json!({
        "address": row["address"],
        "name": row["name"],
        "domain": row["domain"],
        "first_seen_at": ms("first_seen_at"),
        "last_seen_at": ms("last_seen_at"),
        "inbound_count": row["inbound_count"].as_i64().unwrap_or(0),
        "outbound_count": row["outbound_count"].as_i64().unwrap_or(0),
        "last_thread_id": row["last_thread_id"],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q_and_boundary() {
        assert_eq!(fold_q(" Ｂrightwell ").as_deref(), Some("brightwell"));
        assert_eq!(fold_q(&"a".repeat(101)), None);
        let b = (2, 7, 1_000, "a@b.example".to_owned());
        assert_eq!(decode_boundary(&encode_boundary(&b)), Some(b));
        assert_eq!(decode_boundary("[1]"), None);
    }
}
