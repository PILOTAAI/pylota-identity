//! Vectorize (bindings `VECTORS`, and `VECTORS_NEXT` during a re-embed), through `cf::externs`
//! (`worker` 0.8.7 has no binding). Mutations become visible after a few seconds; `getByIds` after
//! about a minute (S6).

use serde_json::{Map, Value, json};

use crate::{PResult, PlatformError};

#[derive(Clone, Debug, PartialEq)]
pub struct VectorRecord {
    pub id: String,
    pub namespace: String,
    pub values: Vec<f32>,
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VectorMatch {
    pub id: String,
    pub score: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexInfo {
    pub vector_count: u64,
    pub dimensions: u32,
}

pub const MAX_UPSERT: usize = 1_000;
pub const MAX_TOP_K: u32 = 100;

pub trait VectorIndex {
    /// At most [`MAX_UPSERT`] vectors; returns the `mutationId`.
    async fn upsert(&self, vectors: &[VectorRecord]) -> PResult<String>;
    /// `returnMetadata: "none"`; `top_k` at most [`MAX_TOP_K`].
    async fn query(
        &self,
        namespace: &str,
        vector: &[f32],
        top_k: u32,
        filter: &Value,
    ) -> PResult<Vec<VectorMatch>>;
    async fn delete_by_ids(&self, ids: &[String]) -> PResult<String>;
    /// The IDs that still exist (the erasure probe).
    async fn get_by_ids(&self, ids: &[String]) -> PResult<Vec<String>>;
    /// The vector count (nightly drift, Search § 6.6).
    async fn describe(&self) -> PResult<IndexInfo>;
}

pub fn check_upsert(binding: &'static str, vectors: &[VectorRecord]) -> PResult<()> {
    if vectors.len() > MAX_UPSERT {
        return Err(PlatformError::invalid(
            binding,
            format!("{} vectors; at most {MAX_UPSERT} per upsert", vectors.len()),
        ));
    }
    Ok(())
}

/// The options of `query`: `{ topK, namespace, filter, returnValues: false, returnMetadata: "none" }`.
pub fn query_options(
    binding: &'static str,
    namespace: &str,
    top_k: u32,
    filter: &Value,
) -> PResult<Value> {
    if top_k == 0 || top_k > MAX_TOP_K {
        return Err(PlatformError::invalid(
            binding,
            format!("topK {top_k}; 1 to {MAX_TOP_K}"),
        ));
    }
    let mut options = json!({
        "topK": top_k, "namespace": namespace, "returnValues": false, "returnMetadata": "none"
    });
    if !filter.is_null() && filter.as_object().is_none_or(|f| !f.is_empty()) {
        options["filter"] = filter.clone();
    }
    Ok(options)
}

/// The matches of a query result `{ count, matches: [{ id, score }] }`.
pub fn read_matches(binding: &'static str, result: &Value) -> PResult<Vec<VectorMatch>> {
    let matches = result["matches"].as_array().ok_or_else(|| {
        PlatformError::new(crate::PlatformErrorKind::Corrupt, binding, "no matches")
    })?;
    Ok(matches
        .iter()
        .filter_map(|m| {
            Some(VectorMatch {
                id: m["id"].as_str()?.to_owned(),
                score: m["score"].as_f64()? as f32,
            })
        })
        .collect())
}

/// `{ mutationId }`.
pub fn read_mutation(binding: &'static str, result: &Value) -> PResult<String> {
    result["mutationId"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| {
            PlatformError::new(crate::PlatformErrorKind::Corrupt, binding, "no mutationId")
        })
}

/// `describe()`: `{ dimensions, vectorCount, … }` (S6).
pub fn read_info(binding: &'static str, result: &Value) -> PResult<IndexInfo> {
    match (
        result["vectorCount"].as_u64(),
        result["dimensions"].as_u64(),
    ) {
        (Some(vector_count), Some(dimensions)) => Ok(IndexInfo {
            vector_count,
            dimensions: dimensions as u32,
        }),
        _ => Err(PlatformError::new(
            crate::PlatformErrorKind::Corrupt,
            binding,
            "describe() without vectorCount or dimensions",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shapes S6 recorded from the V2 binding.
    #[test]
    fn s6_shapes() {
        let o = query_options("VECTORS", "idn_1", 10, &json!({ "tenant": "ten_1" })).unwrap();
        assert_eq!(o["returnMetadata"], "none");
        assert_eq!(o["filter"]["tenant"], "ten_1");
        assert!(query_options("VECTORS", "n", 101, &Value::Null).is_err());
        assert!(
            query_options("VECTORS", "n", 5, &json!({}))
                .unwrap()
                .get("filter")
                .is_none()
        );
        let m = read_matches(
            "VECTORS",
            &json!({ "count": 1, "matches": [{ "id": "a", "namespace": "n", "score": 0.5 }] }),
        )
        .unwrap();
        assert_eq!(
            m,
            [VectorMatch {
                id: "a".into(),
                score: 0.5
            }]
        );
        assert_eq!(
            read_info(
                "VECTORS",
                &json!({ "dimensions": 1024, "vectorCount": 7, "processedUpToMutation": "x" })
            )
            .unwrap(),
            IndexInfo {
                vector_count: 7,
                dimensions: 1024
            }
        );
        assert_eq!(
            read_mutation("VECTORS", &json!({ "mutationId": "m" })).unwrap(),
            "m"
        );
    }
}
