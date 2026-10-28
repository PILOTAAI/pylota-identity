//! What the front Worker and the mailbox exchange for a search: the compiled query in a serialisable
//! form, keyword and read-back candidates, and per-hit details.

use pylota_identity_core::query::{CompiledQuery, SqlParam, TermPattern, TextField};
use pylota_identity_platform::d1::SqlValue;
use serde::{Deserialize, Serialize};

/// [`CompiledQuery`] as it crosses the RPC boundary.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Compiled {
    pub fts_match: Option<String>,
    pub tri_match: Option<String>,
    /// Boolean SQL over alias `m` with `?` parameters; `""` is true.
    pub filter_sql: String,
    pub filter_params: Vec<Param>,
    pub ref_like: Vec<String>,
    /// `(text, prefix, phrase, subject_only)`.
    pub terms: Vec<(String, bool, bool, bool)>,
    pub why_filters: Vec<String>,
    /// Some positive filter is attachment-related (`has:`, `type:`, `filename:`), or there is text.
    pub attachment_query: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Param {
    Int(i64),
    Text(String),
}

impl From<&CompiledQuery> for Compiled {
    fn from(c: &CompiledQuery) -> Self {
        let attachment_query = !c.positive_terms.is_empty()
            || c.why_filters.iter().any(|w| {
                w.starts_with("has:attachment")
                    || w.starts_with("type:")
                    || w.starts_with("filename:")
            });
        Self {
            fts_match: c.fts_match.clone(),
            tri_match: c.tri_match.clone(),
            filter_sql: c.filter.sql.clone(),
            filter_params: c
                .filter
                .params
                .iter()
                .map(|p| match p {
                    SqlParam::Int(i) => Param::Int(*i),
                    SqlParam::Text(t) => Param::Text(t.clone()),
                })
                .collect(),
            ref_like: c.ref_like.clone(),
            terms: c
                .positive_terms
                .iter()
                .map(|t| {
                    (
                        t.text.clone(),
                        t.prefix,
                        t.phrase,
                        t.field == TextField::Subject,
                    )
                })
                .collect(),
            why_filters: c.why_filters.clone(),
            attachment_query,
        }
    }
}

impl Compiled {
    pub fn term_patterns(&self) -> Vec<TermPattern> {
        self.terms
            .iter()
            .map(|(text, prefix, phrase, subject)| TermPattern {
                text: text.clone(),
                prefix: *prefix,
                phrase: *phrase,
                field: if *subject {
                    TextField::Subject
                } else {
                    TextField::Any
                },
            })
            .collect()
    }

    /// The filter as SQL (`1` when empty).
    pub fn filter(&self) -> &str {
        if self.filter_sql.trim().is_empty() {
            "1"
        } else {
            &self.filter_sql
        }
    }

    pub fn params(&self) -> Vec<SqlValue> {
        self.filter_params
            .iter()
            .map(|p| match p {
                Param::Int(i) => (*i).into(),
                Param::Text(t) => t.clone().into(),
            })
            .collect()
    }
}

/// One candidate message (keyword or read-back).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cand {
    pub rowid: i64,
    pub id: String,
    pub thread_id: String,
    pub thread_seq: i64,
    pub direction: String,
    pub status: String,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub subject: Option<String>,
    pub sent_at: Option<i64>,
    pub received_at: i64,
    pub msg_date: i64,
    pub verdict: Option<String>,
    pub known_sender: Option<i64>,
    /// BM25 part (`s ≥ 0`), 0 without text.
    #[serde(default)]
    pub s: f64,
    #[serde(default)]
    pub kw: f64,
    #[serde(default)]
    pub ref_hit: bool,
    #[serde(default)]
    pub fuzzy: bool,
    /// The best chunk of a semantic hit.
    #[serde(default)]
    pub vector_id: Option<String>,
}

/// What the page needs per hit beyond the candidate.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Detail {
    pub rowid: i64,
    pub snippet: String,
    pub why: Vec<String>,
    pub attachment_hits: Vec<serde_json::Value>,
}
