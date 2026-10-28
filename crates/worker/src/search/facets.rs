//! Facets across mailboxes ([Search § 5.7]): each identity's counts summed per key, then the caps
//! re-applied (top 10 by count, ties by value ascending; `month` the 24 newest).
//!
//! [Search § 5.7]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use std::collections::BTreeMap;

use serde_json::{Map, Value};

pub const KEYS: [&str; 6] = [
    "sender",
    "sender_domain",
    "month",
    "label",
    "attachment_type",
    "category",
];

/// The facets object of several mailboxes' answers.
pub fn merge(sets: &[Value]) -> Value {
    let mut out = Map::new();
    for key in KEYS {
        let mut sum: BTreeMap<String, i64> = BTreeMap::new();
        for s in sets {
            if let Some(o) = s[key].as_object() {
                for (v, n) in o {
                    *sum.entry(v.clone()).or_default() += n.as_i64().unwrap_or(0);
                }
            }
        }
        let mut rows: Vec<(String, i64)> = sum.into_iter().collect();
        if key == "month" {
            rows.sort_by(|a, b| b.0.cmp(&a.0));
            rows.truncate(24);
        } else {
            rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            rows.truncate(10);
        }
        out.insert(
            key.to_owned(),
            Value::Object(rows.into_iter().map(|(v, n)| (v, n.into())).collect()),
        );
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_sums_and_caps() {
        let a = json!({ "sender": { "a@x": 3, "b@x": 1 }, "month": { "2026-10": 2 } });
        let b = json!({ "sender": { "b@x": 2 }, "month": { "2026-09": 1 } });
        let m = merge(&[a, b]);
        assert_eq!(m["sender"], json!({ "a@x": 3, "b@x": 3 }));
        assert_eq!(m["month"], json!({ "2026-10": 2, "2026-09": 1 }));
        assert_eq!(m.as_object().unwrap().len(), 6);
        let many: Map<String, Value> = (0..15).map(|i| (format!("l{i:02}"), json!(1))).collect();
        let m = merge(&[json!({ "label": many })]);
        let labels = m["label"].as_object().unwrap();
        assert_eq!(labels.len(), 10);
        assert!(labels.contains_key("l00") && !labels.contains_key("l10"));
    }
}
