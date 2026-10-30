//! The golden mailbox in the harness (Testing § 9): the generated mail and the hand-written cases of
//! `crates/conformance/golden/`, delivered through the `email()` handler to the four identities of a
//! fresh tenant. Mail from a legitimate sender is DKIM-signed with the corpus test key, whose record
//! (and a `p=none` DMARC policy) the fake DoH publishes for that domain; spoofed and phishing senders
//! stay unsigned on domains without records, as they would be. Used by
//! `it::search::golden_keyword_recall` and by the evaluations (`it::eval`).

use std::collections::{BTreeMap, BTreeSet};

use pylota_identity_conformance::golden::generator::{self, Truth};
use pylota_identity_conformance::golden::metrics;
use pylota_identity_conformance::golden::sets::{Golden, Query};
use serde_json::{Value, json};

use crate::{dkim, eventually, fakes, mailbox_query, mint, patch, platform, post};

pub struct Mailbox {
    pub tid: String,
    /// A tenant key with every read and search permission.
    pub key: String,
    /// username → `(identity id, address)`.
    pub identities: BTreeMap<String, (String, String)>,
    pub golden: Golden,
    /// Truth key → message ID, for every stored message.
    pub ids: BTreeMap<String, String>,
    /// Message ID → truth key.
    pub keys: BTreeMap<String, String>,
}

/// Mail from these domains is not the sender's own: it stays unsigned.
fn spoofed(domain: &str) -> bool {
    domain.ends_with(".invalid") || domain.contains("-payments.") || domain.contains("-billing.")
}

fn deliver(from: &str, domain: &str, to: &str, raw: &str) {
    let raw = if spoofed(domain) {
        raw.to_owned()
    } else {
        dkim::sign_as(raw, domain)
    };
    crate::inbound(from, to, &raw);
}

/// Creates the tenant (with `policy`) and its identities, delivers the whole golden mailbox and waits
/// until every message is stored.
pub fn load(name: &str, policy: Value) -> Mailbox {
    let now = fakes::worker_now();
    let golden =
        Golden::load(&pylota_identity_conformance::golden::dir(), now).expect("the golden set");
    let p = platform();
    let t = crate::tenant(&p, name);
    let tid = t["id"].as_str().unwrap().to_owned();
    if !policy.is_null() {
        let r = patch(&format!("/v1/tenants/{tid}/policy"), &p, policy);
        assert_eq!(r.status, 200, "{}", r.body);
    }
    let (_, key) = mint(
        &p,
        json!({ "name": name, "level": "tenant", "tenant_id": tid, "permissions": [
            "messages:read", "messages:write", "quarantine:review", "attachments:read",
            "search:read", "search:agentic", "identities:read"
        ] }),
    );
    let mut identities = BTreeMap::new();
    for u in &golden.config.identities {
        let idn = crate::identity(&p, &tid, u);
        identities.insert(
            u.clone(),
            (
                idn["id"].as_str().unwrap().to_owned(),
                idn["primary_address"].as_str().unwrap().to_owned(),
            ),
        );
    }

    // The senders' DNS.
    let record = dkim::public_record();
    let domains: BTreeSet<&str> = golden.truths().iter().map(|t| t.domain.as_str()).collect();
    for d in domains.iter().filter(|d| !spoofed(d)) {
        fakes::publish_txt(&format!("s1._domainkey.{d}"), std::slice::from_ref(&record));
        fakes::publish_txt(&format!("_dmarc.{d}"), &["v=DMARC1; p=none".to_owned()]);
    }

    for m in &golden.generated {
        let to = &identities[&m.truth.identity].1;
        deliver(
            &m.truth.from,
            &m.truth.domain,
            to,
            &generator::render(m, to, now),
        );
    }
    for h in &golden.hard {
        let to = &identities[&h.truth.identity].1;
        deliver(
            &h.truth.from,
            &h.truth.domain,
            to,
            &h.raw.replace("{to}", to),
        );
    }

    // Stored: every message's `Message-ID` is `<{key}@golden.example>`.
    let want = golden.truths().len();
    let mut ids = BTreeMap::new();
    eventually(1_800, "the golden mailbox to be stored", || {
        ids.clear();
        for (idn, _) in identities.values() {
            for r in mailbox_query(idn, "SELECT id, rfc_message_id FROM messages") {
                let rfc = r["rfc_message_id"].as_str().unwrap_or("");
                if let Some(k) = rfc
                    .trim_matches(|c| c == '<' || c == '>')
                    .strip_suffix("@golden.example")
                {
                    ids.insert(k.to_owned(), r["id"].as_str().unwrap_or("").to_owned());
                }
            }
        }
        if ids.len() < want {
            std::thread::sleep(std::time::Duration::from_secs(5));
            return None;
        }
        Some(())
    });
    let keys = ids.iter().map(|(k, v)| (v.clone(), k.clone())).collect();
    Mailbox {
        tid,
        key,
        identities,
        golden,
        ids,
        keys,
    }
}

impl Mailbox {
    pub fn truths(&self) -> Vec<&Truth> {
        self.golden.truths()
    }

    /// One tenant-scope search: the ranked truth keys of its hits.
    pub fn search(&self, q: &str, mode: &str, limit: u32) -> (Vec<String>, Value) {
        let r = post(
            &format!("/v1/tenants/{}/search", self.tid),
            &self.key,
            json!({ "q": q, "mode": mode, "limit": limit }),
        );
        assert_eq!(r.status, 200, "{q}: {}", r.body);
        let ranked = r.body["hits"]
            .as_array()
            .map(|h| {
                h.iter()
                    .filter_map(|x| x["message_id"].as_str())
                    .map(|id| self.keys.get(id).cloned().unwrap_or_else(|| id.to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        (ranked, r.body)
    }

    /// Waits until hybrid search reports `semantic_coverage = 1.0` for every identity.
    pub fn wait_semantic(&self) {
        for (idn, _) in self.identities.values() {
            eventually(3_600, "semantic coverage 1.0", || {
                let r = post(
                    &format!("/v1/identities/{idn}/search"),
                    &self.key,
                    json!({ "q": "booking", "mode": "hybrid", "limit": 1 }),
                );
                if r.body["semantic_coverage"].as_f64() == Some(1.0) {
                    return Some(());
                }
                std::thread::sleep(std::time::Duration::from_secs(10));
                None
            });
        }
    }
}

/// Per-query results of one search run in `mode`.
pub struct SearchRun {
    pub recall_at_10: f64,
    pub mrr: f64,
    pub ndcg_at_10: f64,
    pub zero_result_rate: f64,
    /// The exact-reference queries with no hit.
    pub exact_zero: Vec<String>,
    pub items: Vec<Value>,
}

pub fn search_run(m: &Mailbox, queries: &[Query], mode: &str) -> SearchRun {
    let truths = m.truths();
    let mut items = Vec::new();
    let (mut recall, mut rr, mut ndcg, mut zero) = (Vec::new(), Vec::new(), Vec::new(), 0usize);
    let mut exact_zero = Vec::new();
    for q in queries {
        let grades = q.grades(&truths);
        let (ranked, _) = m.search(&q.q, mode, 50);
        let r10 = metrics::recall_at(&ranked, &grades, 10);
        recall.push(r10);
        rr.push(Some(metrics::reciprocal_rank(&ranked, &grades)));
        ndcg.push(Some(metrics::ndcg_at_10(&ranked, &grades)));
        if ranked.is_empty() {
            zero += 1;
            if q.kind == "exact_reference" {
                exact_zero.push(q.id.clone());
            }
        }
        items
            .push(json!({ "id": q.id, "kind": q.kind, "recall_at_10": r10, "hits": ranked.len() }));
    }
    SearchRun {
        recall_at_10: metrics::mean(recall),
        mrr: metrics::mean(rr),
        ndcg_at_10: metrics::mean(ndcg),
        zero_result_rate: zero as f64 / queries.len().max(1) as f64,
        exact_zero,
        items,
    }
}

/// A figure recorded in `docs/src/project/quality.md`: the number in the table row whose first cell
/// is `` `name` ``, or `None` while the row says `pending`.
pub fn baseline(name: &str) -> Option<f64> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/src/project/quality.md");
    let text = std::fs::read_to_string(path).ok()?;
    let row = text
        .lines()
        .find(|l| l.starts_with(&format!("| `{name}` |")))?;
    row.split('|').nth(2)?.trim().parse().ok()
}
