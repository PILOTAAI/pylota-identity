//! The native corpus runner (Testing § 5.3): every case through [`crate::pipeline::run`], compared with
//! its `[expect]` table. An unknown expectation key fails, so a typo cannot pass silently.

use crate::loader::{self, Case};
use crate::pipeline::{self, Outcome};

/// `PM_TRUSTED_AUTHSERV_ID` for the corpus: Cloudflare's, as spike S2 recorded.
const AUTHSERV: &str = "mx.cloudflare.net";

fn strings(v: &toml::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn json_str<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The expectation failures of one case.
pub fn check(case: &Case, o: &Outcome) -> Vec<String> {
    let mut errs: Vec<String> = Vec::new();
    macro_rules! fail {
        ($k:expr, $want:expr, $got:expr) => {
            errs.push(format!("{}: expected {:?}, got {:?}", $k, $want, $got))
        };
    }
    let p = &o.parsed;
    let text = o.text.as_deref().unwrap_or("");
    let html = o.html_sanitized.as_deref().unwrap_or("");
    for (k, v) in case.expect() {
        let s = v.as_str().unwrap_or("");
        let list = strings(&v);
        let int = v.as_integer().unwrap_or(-1);
        match k.as_str() {
            "subject" => {
                if p.subject.as_deref().unwrap_or("") != s {
                    fail!(k, &s, &p.subject);
                }
            }
            "normalised_subject" => {
                if o.normalised_subject() != s {
                    fail!(k, &s, &o.normalised_subject());
                }
            }
            "from" => {
                let got = p.from.as_ref().map_or("", |f| f.address.as_str());
                if got != s {
                    fail!(k, &s, &got);
                }
            }
            "from_name" => {
                let got = p.from.as_ref().map_or("", |f| f.name.as_str());
                if got != s {
                    fail!(k, &s, &got);
                }
            }
            "from_count" => {
                if p.from_count as i64 != int {
                    fail!(k, &int, &p.from_count);
                }
            }
            "message_id" => {
                if p.message_id != s {
                    fail!(k, &s, &p.message_id);
                }
            }
            "in_reply_to" => {
                if p.in_reply_to.as_deref().unwrap_or("") != s {
                    fail!(k, &s, &p.in_reply_to);
                }
            }
            "references" => {
                if p.references.len() as i64 != int {
                    fail!(k, &int, &p.references.len());
                }
            }
            "flags" => {
                let want: std::collections::BTreeSet<String> = list.into_iter().collect();
                if want != o.flags {
                    fail!(k, &want, &o.flags);
                }
            }
            "text_contains" => {
                for t in &list {
                    if !text.contains(t.as_str()) {
                        fail!(k, t, &text);
                    }
                }
            }
            "text_absent" => {
                for t in &list {
                    if text.contains(t.as_str()) || o.extracted_text.contains(t.as_str()) {
                        fail!(k, t, &text);
                    }
                }
            }
            "text_absent_all" => {
                if v.as_bool() == Some(true) && (o.text.is_some() || o.html_sanitized.is_some()) {
                    fail!(k, &"no text or html", &(&o.text, &o.html_sanitized));
                }
            }
            "html_contains" => {
                for t in &list {
                    if !html.contains(t.as_str()) {
                        fail!(k, t, &html);
                    }
                }
            }
            "html_absent" => {
                for t in &list {
                    if html.contains(t.as_str()) {
                        fail!(k, t, &html);
                    }
                }
            }
            "extracted_equals" => {
                if o.extracted_text != s {
                    fail!(k, &s, &o.extracted_text);
                }
            }
            "extracted_contains" => {
                for t in &list {
                    if !o.extracted_text.contains(t.as_str()) {
                        fail!(k, t, &o.extracted_text);
                    }
                }
            }
            "attachments" => {
                if p.attachments.len() as i64 != int {
                    fail!(k, &int, &p.attachments.len());
                }
            }
            "attachments_max" => {
                if p.attachments.len() as i64 > int {
                    fail!(k, &int, &p.attachments.len());
                }
            }
            "attachment_names" => {
                let got: Vec<&str> = p
                    .attachments
                    .iter()
                    .map(|a| a.filename.as_deref().unwrap_or(""))
                    .collect();
                if got != list {
                    fail!(k, &list, &got);
                }
            }
            "attachment_types" => {
                let got: Vec<&str> = o.assessments.iter().map(|a| a.sniffed_type).collect();
                if got != list {
                    fail!(k, &list, &got);
                }
            }
            "risks" => {
                let got: Vec<String> = o.risks().iter().map(json_str).collect();
                if got != list {
                    fail!(k, &list, &got);
                }
            }
            "cids" => {
                if o.cids != list {
                    fail!(k, &list, &o.cids);
                }
            }
            "nested_message_ids" => {
                let got: Vec<String> = p
                    .attachments
                    .iter()
                    .filter_map(|a| a.nested.as_ref()?.message_id.clone())
                    .collect();
                if got != list {
                    fail!(k, &list, &got);
                }
            }
            "nested_text_contains" => {
                let all: String = p
                    .attachments
                    .iter()
                    .filter_map(|a| a.nested.as_ref()?.text.clone())
                    .collect::<Vec<_>>()
                    .join("\n");
                for t in &list {
                    if !all.contains(t.as_str()) {
                        fail!(k, t, &all);
                    }
                }
            }
            "signature" => {
                let got = p
                    .signature
                    .as_ref()
                    .map(|s| json_str(&s.kind))
                    .unwrap_or_default();
                if got != s {
                    fail!(k, &s, &got);
                }
            }
            "kind" => {
                let got = json_str(&o.classification.kind);
                if got != s {
                    fail!(k, &s, &got);
                }
            }
            "automated" => {
                if Some(o.classification.automated) != v.as_bool() {
                    fail!(k, &v, &o.classification.automated);
                }
            }
            "evidence" => {
                if o.classification.evidence != list {
                    fail!(k, &list, &o.classification.evidence);
                }
            }
            "evidence_first" => {
                if o.classification.evidence.first().copied() != Some(s) {
                    fail!(k, &s, &o.classification.evidence);
                }
            }
            "hop" => {
                if i64::from(o.classification.hop) != int {
                    fail!(k, &int, &o.classification.hop);
                }
            }
            "dsn_recipients" | "dsn_status" | "dsn_action" => {
                let r = o
                    .classification
                    .dsn
                    .as_ref()
                    .map(|d| d.recipients.as_slice())
                    .unwrap_or(&[]);
                let got: Vec<String> = r
                    .iter()
                    .map(|r| match k.as_str() {
                        "dsn_recipients" => r.final_recipient.clone(),
                        "dsn_status" => r.status.clone(),
                        _ => r.action.clone(),
                    })
                    .map(Option::unwrap_or_default)
                    .collect();
                if got != list {
                    fail!(k, &list, &got);
                }
            }
            "dsn_original_message_id" => {
                let got = o
                    .classification
                    .dsn
                    .as_ref()
                    .and_then(|d| d.original_message_id.clone());
                if got.as_deref() != Some(s) {
                    fail!(k, &s, &got);
                }
            }
            "mdn_original_message_id" => {
                let got = o
                    .classification
                    .mdn
                    .as_ref()
                    .and_then(|d| d.original_message_id.clone());
                if got.as_deref() != Some(s) {
                    fail!(k, &s, &got);
                }
            }
            "dkim" => {
                let got: Vec<String> = o.dkim.iter().map(|d| json_str(&d.result)).collect();
                if got != list {
                    fail!(k, &list, &got);
                }
            }
            "dmarc" => {
                let got = json_str(&o.dmarc.result);
                if got != s {
                    fail!(k, &s, &got);
                }
            }
            "verdict" => {
                let got = json_str(&o.verdict);
                if got != s {
                    fail!(k, &s, &got);
                }
            }
            other => errs.push(format!("unknown expectation key {other}")),
        }
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every corpus case matches its expectations; with `auth/` this is the DKIM verdict-parity check of
    /// spike S4 against dkimpy's reference results.
    /// Covers: B2, B4, B5, B6, B7, B8, B9, B10, B11, B13, C1, C2, C8, D1, D6, D9
    #[test]
    fn cases_match() {
        let zones = loader::zones();
        let mut failures = Vec::new();
        let cases = loader::cases();
        for case in &cases {
            let from_domain = pylota_identity_core::mime::parse(&case.raw, &Default::default())
                .from
                .map(|f| pylota_identity_core::address::domain_of(&f.address).to_owned());
            let dns = loader::answers_for(&case.raw, from_domain.as_deref(), &zones);
            let outcome = pipeline::run(&case.raw, case.envelope_from(), &dns, AUTHSERV);
            for e in check(case, &outcome) {
                failures.push(format!("conf::{}: {e}", case.name.replace('/', "::")));
            }
        }
        assert!(
            failures.is_empty(),
            "{} failures in {} cases:\n{}",
            failures.len(),
            cases.len(),
            failures.join("\n")
        );
    }
}
