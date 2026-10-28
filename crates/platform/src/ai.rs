//! Workers AI (binding `AI`). `AI.run` options and `AI.toMarkdown` go through `cf::externs`, because
//! `worker` 0.8.7 lacks them (Rust workspace § 7; S6 passed, so no REST fallback).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::PResult;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AiOptions {
    /// `PM_AI_GATEWAY`.
    pub gateway: Option<String>,
    /// `false` for every call that carries mail content (Security § 3.5).
    pub collect_log: bool,
    /// `true` for every call that carries mail content.
    pub skip_cache: bool,
}

impl AiOptions {
    /// The options of a call that carries mail content.
    pub fn content(gateway: Option<String>) -> Self {
        Self {
            gateway,
            collect_log: false,
            skip_cache: true,
        }
    }

    /// The third argument of `AI.run`: `{ gateway: { id, collectLog, skipCache } }` with a gateway, `{}`
    /// without one.
    pub fn to_json(&self) -> Value {
        match &self.gateway {
            Some(id) => json!({
                "gateway": { "id": id, "collectLog": self.collect_log, "skipCache": self.skip_cache }
            }),
            None => json!({}),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownInput {
    pub name: String,
    /// The sniffed type, given to the binding as the `Blob`'s type.
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkdownFormat {
    Markdown,
    Text,
    Error,
}

/// One result of `AI.toMarkdown`, as the binding spells it (S6: `mimeType`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkdownOutput {
    pub name: String,
    pub format: MarkdownFormat,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub tokens: u32,
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub error: Option<String>,
}

/// Whether a failed `AI.run` was Workers AI refusing for load: an HTTP 429, a rate-limit message, or
/// error 3040 ("Capacity temporarily exceeded"). The binding reports these only as error text, so the
/// test is on the text ([Search § 6], F16).
///
/// [Search § 6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md
pub fn is_rate_limited(e: &crate::PlatformError) -> bool {
    let d = e.detail.to_ascii_lowercase();
    d.contains("429")
        || d.contains("rate limit")
        || d.contains("too many requests")
        || d.contains("3040")
}

pub trait Ai {
    async fn run<I: Serialize, O: DeserializeOwned>(
        &self,
        model: &str,
        input: &I,
        opts: &AiOptions,
    ) -> PResult<O>;
    /// A model whose output is bytes (an image or audio).
    async fn run_bytes<I: Serialize>(
        &self,
        model: &str,
        input: &I,
        opts: &AiOptions,
    ) -> PResult<Vec<u8>>;
    async fn to_markdown(&self, docs: &[MarkdownInput]) -> PResult<Vec<MarkdownOutput>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_answers() {
        let e = |d: &str| crate::PlatformError::unavailable("AI", d);
        assert!(is_rate_limited(&e("fake answered 429")));
        assert!(is_rate_limited(&e(
            "AiError: 3040: Capacity temporarily exceeded"
        )));
        assert!(!is_rate_limited(&e("fake answered 500")));
    }

    #[test]
    fn gateway_options_and_markdown_shape() {
        let opts = AiOptions::content(Some("gw".into()));
        assert_eq!(
            opts.to_json(),
            json!({ "gateway": { "id": "gw", "collectLog": false, "skipCache": true } })
        );
        assert_eq!(AiOptions::default().to_json(), json!({}));
        // S6's toMarkdown result for a PDF.
        let out: MarkdownOutput = serde_json::from_value(json!({
            "id": "x", "name": "a.pdf", "format": "markdown", "mimeType": "application/pdf",
            "tokens": 12, "data": "### Page 1\n..."
        }))
        .unwrap();
        assert_eq!(out.format, MarkdownFormat::Markdown);
        assert_eq!(out.mime_type, "application/pdf");
    }
}
