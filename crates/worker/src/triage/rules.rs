//! `policy.triage` read for a job, checked when the policy is written ([Triage § 4, § 8]), and the
//! mailbox's row turned into the rules' input.
//!
//! [Triage § 4, § 8]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/triage.md

use pylota_identity_core::attach::Risk;
use pylota_identity_core::auth::{DmarcResult, Verdict};
use pylota_identity_core::classify::Kind;
use pylota_identity_core::mime::Mailbox;
use pylota_identity_core::policy::get_path;
use pylota_identity_core::triage_rules::{
    AttachmentMeta, AutomatedClass, Categories, CategoryError, CustomCategory, MessageFlag,
    RuleMatch, RuleSet, TenantRule, TriageInput, TrustFlag, fold, validate_categories,
};
use serde_json::Value;

use crate::errors::ApiError;
use crate::mailbox::triage::Loaded;

/// At most this many tenant rules, labels a rule adds, entries per match list and characters per
/// string (§ 4).
pub const MAX_RULES: usize = 50;
pub const MAX_LABELS_ADD: usize = 10;
pub const MAX_LIST: usize = 20;
pub const MAX_STRING: usize = 200;

/// `policy.triage` of an effective policy.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub enabled: bool,
    pub daily_model_cap: u32,
    pub categories: Categories,
    pub rules: Vec<TenantRule>,
}

pub fn settings(policy: &Value) -> Settings {
    Settings {
        enabled: get_path(policy, "triage.enabled") != Some(&Value::Bool(false)),
        daily_model_cap: get_path(policy, "triage.daily_model_cap")
            .and_then(Value::as_u64)
            .map_or(2_000, |n| n.min(u64::from(u32::MAX)) as u32),
        categories: categories(policy),
        rules: get_path(policy, "triage.rules")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(rule).collect())
            .unwrap_or_default(),
    }
}

fn categories(policy: &Value) -> Categories {
    match get_path(policy, "triage.categories").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => Categories::Custom(
            list.iter()
                .map(|c| CustomCategory {
                    name: c["name"].as_str().unwrap_or("").to_owned(),
                    description: c["description"].as_str().unwrap_or("").to_owned(),
                })
                .collect(),
        ),
        _ => Categories::BuiltIn,
    }
}

fn list(v: &Value, k: &str) -> Option<Vec<String>> {
    v.get(k)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(fold).collect())
}

/// One stored rule. Values are folded here as well as when written, so a rule stored before the fold
/// still matches.
fn rule(v: &Value) -> Option<TenantRule> {
    let m = v.get("match")?;
    let set = v.get("set")?;
    Some(TenantRule {
        id: v["id"].as_str()?.to_owned(),
        r#match: RuleMatch {
            from: list(m, "from"),
            from_domain: list(m, "from_domain"),
            to_identity: m.get("to_identity").and_then(Value::as_array).map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            }),
            subject_contains: list(m, "subject_contains"),
            body_contains: list(m, "body_contains"),
            has_attachment: m.get("has_attachment").and_then(Value::as_bool),
            label: list(m, "label"),
        },
        set: RuleSet {
            category: set["category"].as_str().map(str::to_owned),
            labels_add: set.get("labels_add").and_then(Value::as_array).map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            }),
            urgency_min: set["urgency_min"].as_u64().map(|u| u.min(3) as u8),
            needs_reply: set["needs_reply"]
                .as_f64()
                .map(|n| n.clamp(0.0, 1.0) as f32),
            skip_model: set["skip_model"].as_bool(),
        },
        stop: v["stop"].as_bool().unwrap_or(false),
    })
}

fn rule_id_ok(id: &str) -> bool {
    let b = id.as_bytes();
    (1..=48).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
}

/// `^[a-z0-9][a-z0-9_:-]{0,63}$`, the label grammar.
pub fn label_ok(l: &str) -> bool {
    let b = l.as_bytes();
    (1..=64).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|&c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b':' | b'-')
        })
}

/// Checks `policy.triage` of the policy a write would leave (§ 4, § 8); the first error decides.
pub fn validate(policy: &Value) -> Result<(), ApiError> {
    let base = "policy.triage";
    let cats = categories(policy);
    if let Categories::Custom(list) = &cats {
        validate_categories(list).map_err(|e| {
            let (path, msg) = match e {
                CategoryError::Count => (
                    format!("{base}.categories"),
                    "Between 1 and 20 categories.",
                ),
                CategoryError::Name(i) => (
                    format!("{base}.categories[{i}].name"),
                    "Lower-case letters, digits and underscores, starting with a letter; at most 32 characters.",
                ),
                CategoryError::Duplicate(i) => (
                    format!("{base}.categories[{i}].name"),
                    "Category names must be unique.",
                ),
                CategoryError::Description(i) => (
                    format!("{base}.categories[{i}].description"),
                    "1 to 200 characters.",
                ),
            };
            ApiError::invalid(&path, msg)
        })?;
    }
    let Some(rules) = get_path(policy, "triage.rules").and_then(Value::as_array) else {
        return Ok(());
    };
    if rules.len() > MAX_RULES {
        return Err(ApiError::invalid(
            &format!("{base}.rules"),
            "At most 50 rules.",
        ));
    }
    for (i, r) in rules.iter().enumerate() {
        let at = |k: &str| format!("{base}.rules[{i}]{k}");
        let id = r["id"].as_str().unwrap_or("");
        if !rule_id_ok(id) {
            return Err(ApiError::invalid(
                &at(".id"),
                "Lower-case letters, digits, '_' and '-', starting with a letter or digit; at most 48 characters.",
            ));
        }
        if rules[..i].iter().any(|p| p["id"].as_str() == Some(id)) {
            return Err(ApiError::invalid(&at(".id"), "Rule IDs must be unique."));
        }
        let m = r["match"]
            .as_object()
            .filter(|m| m.values().any(|v| !v.is_null()));
        let Some(m) = m else {
            return Err(ApiError::invalid(&at(".match"), "At least one condition."));
        };
        for (k, v) in m {
            if let Some(a) = v.as_array() {
                if a.is_empty() || a.len() > MAX_LIST {
                    return Err(ApiError::invalid(
                        &at(&format!(".match.{k}")),
                        "1 to 20 entries.",
                    ));
                }
                if let Some(j) = a.iter().position(|s| {
                    s.as_str()
                        .is_none_or(|s| s.is_empty() || s.chars().count() > MAX_STRING)
                }) {
                    return Err(ApiError::invalid(
                        &at(&format!(".match.{k}[{j}]")),
                        "1 to 200 characters.",
                    ));
                }
            }
        }
        let set = r["set"]
            .as_object()
            .filter(|s| s.values().any(|v| !v.is_null()));
        let Some(set) = set else {
            return Err(ApiError::invalid(&at(".set"), "At least one field."));
        };
        if let Some(c) = set.get("category").and_then(Value::as_str)
            && !cats.contains(c)
        {
            return Err(ApiError::invalid(
                &at(".set.category"),
                "Not a category of this tenant.",
            ));
        }
        if let Some(a) = set.get("labels_add").and_then(Value::as_array) {
            if a.len() > MAX_LABELS_ADD {
                return Err(ApiError::invalid(
                    &at(".set.labels_add"),
                    "At most 10 labels.",
                ));
            }
            if let Some(j) = a.iter().position(|l| !l.as_str().is_some_and(label_ok)) {
                return Err(ApiError::invalid(
                    &at(&format!(".set.labels_add[{j}]")),
                    "Lower-case letters, digits, '_', ':' and '-', starting with a letter or digit; at most 64 characters.",
                ));
            }
        }
        if let Some(u) = set.get("urgency_min").filter(|v| !v.is_null())
            && !u.as_u64().is_some_and(|u| u <= 3)
        {
            return Err(ApiError::invalid(&at(".set.urgency_min"), "0 to 3."));
        }
        if let Some(n) = set.get("needs_reply").filter(|v| !v.is_null())
            && !n.as_f64().is_some_and(|n| (0.0..=1.0).contains(&n))
        {
            return Err(ApiError::invalid(&at(".set.needs_reply"), "0 to 1."));
        }
    }
    Ok(())
}

/// Rule values folded as stored (§ 4): NFKC and lower case, so matching never re-folds them.
pub fn fold_rules(patch: &mut Value) {
    let Some(rules) = patch
        .pointer_mut("/triage/rules")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for r in rules {
        let Some(m) = r.get_mut("match").and_then(Value::as_object_mut) else {
            continue;
        };
        for k in [
            "from",
            "from_domain",
            "subject_contains",
            "body_contains",
            "label",
        ] {
            if let Some(a) = m.get_mut(k).and_then(Value::as_array_mut) {
                for s in a.iter_mut() {
                    if let Some(t) = s.as_str() {
                        *s = Value::String(fold(t));
                    }
                }
            }
        }
    }
}

fn kind(k: &str) -> Kind {
    match k {
        "dsn" => Kind::Dsn,
        "mdn" => Kind::Mdn,
        "list" => Kind::List,
        "automated" => Kind::Automated,
        "calendar" => Kind::Calendar,
        _ => Kind::Normal,
    }
}

pub fn verdict(v: &str) -> Verdict {
    match v {
        "pass" => Verdict::Pass,
        "arc_rescued" => Verdict::ArcRescued,
        "unverified" => Verdict::Unverified,
        "fail" => Verdict::Fail,
        "unaligned" => Verdict::Unaligned,
        "softfail" => Verdict::Softfail,
        _ => Verdict::None,
    }
}

fn dmarc(d: &str) -> DmarcResult {
    match d {
        "pass" => DmarcResult::Pass,
        "fail" => DmarcResult::Fail,
        "temperror" => DmarcResult::Temperror,
        _ => DmarcResult::None,
    }
}

fn risk(r: &str) -> Option<Risk> {
    Some(match r {
        "executable" => Risk::Executable,
        "macro" => Risk::Macro,
        "archive_bomb" => Risk::ArchiveBomb,
        "encrypted_archive" => Risk::EncryptedArchive,
        "type_mismatch" => Risk::TypeMismatch,
        "encrypted_document" => Risk::EncryptedDocument,
        _ => return None,
    })
}

/// `automated.class` from inbound's evidence: an auto-reply header or `Precedence`, or an
/// out-of-office subject. `Auto-Submitted` alone is a notification: inbound does not keep its value,
/// and `auto-generated` (RFC 3834) is not a reply.
fn automated_class(evidence: &[String]) -> Option<AutomatedClass> {
    let has = |e: &str| evidence.iter().any(|x| x == e);
    if has("auto_reply_subject") {
        Some(AutomatedClass::OutOfOffice)
    } else if has("x_autoreply") || has("x_autorespond") || has("precedence_auto") {
        Some(AutomatedClass::AutoReply)
    } else if evidence.is_empty() {
        None
    } else {
        Some(AutomatedClass::Other)
    }
}

/// The rules' input. `excerpts` are the attachment texts read from R2, by attachment index.
pub fn input(
    l: &Loaded,
    identity_id: &str,
    username: &str,
    excerpts: &[Option<String>],
) -> TriageInput {
    let flag = |f: &str| l.flags.iter().any(|x| x == f);
    TriageInput {
        kind: kind(&l.kind),
        automated_class: automated_class(&l.evidence),
        from: Mailbox {
            address: l.from_address.clone(),
            name: l.from_name.clone(),
        },
        subject: l.subject.clone(),
        extracted_text: l.extracted_text.clone(),
        verdict: verdict(&l.verdict),
        dmarc: dmarc(&l.dmarc),
        known_sender: l.known_sender,
        spam_score: l.spam_score as f32,
        trust_flags: [
            ("display_name_spoof", TrustFlag::DisplayNameSpoof),
            ("lookalike_domain", TrustFlag::LookalikeDomain),
            ("reply_to_mismatch", TrustFlag::ReplyToMismatch),
            ("thread_join_unverified", TrustFlag::ThreadJoinUnverified),
            ("shared_domain_sender", TrustFlag::SharedDomainSender),
        ]
        .into_iter()
        .filter(|(n, _)| flag(n))
        .map(|(_, f)| f)
        .collect(),
        message_flags: [
            ("hidden_text", MessageFlag::HiddenText),
            ("encrypted", MessageFlag::Encrypted),
            ("parse_degraded", MessageFlag::ParseDegraded),
        ]
        .into_iter()
        .filter(|(n, _)| flag(n))
        .map(|(_, f)| f)
        .collect(),
        attachments: l
            .attachments
            .iter()
            .enumerate()
            .map(|(i, a)| AttachmentMeta {
                filename: a.filename.clone(),
                inline: a.inline,
                risk: a.risk.as_deref().and_then(risk),
                excerpt: excerpts.get(i).cloned().flatten(),
            })
            .collect(),
        labels: l.labels.clone(),
        has_verification: l.has_verification,
        identity_id: identity_id.to_owned(),
        identity_username: username.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn pol(triage: Value) -> Value {
        json!({ "triage": triage })
    }

    #[test]
    fn validation_paths() {
        let ok = pol(json!({ "rules": [{ "id": "pcn-council",
            "match": { "from_domain": ["westminster.example"], "subject_contains": ["pcn"] },
            "set": { "category": "legal_compliance", "labels_add": ["pcn"], "urgency_min": 2, "needs_reply": 0.9 },
            "stop": true }] }));
        assert!(validate(&ok).is_ok());
        let path =
            |p: &Value| validate(p).unwrap_err().details.unwrap()["errors"][0]["path"].clone();
        let mut bad = ok.clone();
        bad["triage"]["rules"][0]["set"]["category"] = json!("pcn");
        assert_eq!(path(&bad), "policy.triage.rules[0].set.category");
        // With a custom list naming it, the category is accepted.
        bad["triage"]["categories"] =
            json!([{ "name": "pcn", "description": "Penalty charge notices" }]);
        assert!(validate(&bad).is_ok());
        let mut bad = ok.clone();
        bad["triage"]["rules"][0]["id"] = json!("Bad ID");
        assert_eq!(path(&bad), "policy.triage.rules[0].id");
        let mut bad = ok.clone();
        bad["triage"]["rules"][0]["match"] = json!({});
        assert_eq!(path(&bad), "policy.triage.rules[0].match");
        let mut bad = ok.clone();
        bad["triage"]["rules"][0]["set"]["labels_add"] = json!(["Not OK"]);
        assert_eq!(path(&bad), "policy.triage.rules[0].set.labels_add[0]");
        let mut bad = ok.clone();
        bad["triage"]["rules"][0]["set"]["urgency_min"] = json!(4);
        assert_eq!(path(&bad), "policy.triage.rules[0].set.urgency_min");
        let mut two = ok.clone();
        let r = two["triage"]["rules"][0].clone();
        two["triage"]["rules"].as_array_mut().unwrap().push(r);
        assert_eq!(path(&two), "policy.triage.rules[1].id");
        let bad = pol(json!({ "categories": [{ "name": "Bad", "description": "x" }] }));
        assert_eq!(path(&bad), "policy.triage.categories[0].name");
    }

    #[test]
    fn settings_fold_and_defaults() {
        let s = settings(&pol(
            json!({ "enabled": true, "daily_model_cap": 3, "rules": [{ "id": "r",
            "match": { "subject_contains": ["ＰＣＮ Notice"] }, "set": { "skip_model": true } }] }),
        ));
        assert_eq!(s.daily_model_cap, 3);
        assert_eq!(s.categories, Categories::BuiltIn);
        assert_eq!(
            s.rules[0].r#match.subject_contains.as_deref(),
            Some(&["pcn notice".to_owned()][..])
        );
        let mut patch = json!({ "triage": { "rules": [{ "id": "r", "match": { "from": ["A@B.Example"] }, "set": { "skip_model": true } }] } });
        fold_rules(&mut patch);
        assert_eq!(
            patch["triage"]["rules"][0]["match"]["from"][0],
            "a@b.example"
        );
        assert!(!settings(&pol(json!({ "enabled": false }))).enabled);
    }

    #[test]
    fn automated_classes() {
        let ev =
            |e: &[&str]| automated_class(&e.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert_eq!(ev(&[]), None);
        assert_eq!(ev(&["auto_submitted"]), Some(AutomatedClass::Other));
        assert_eq!(
            ev(&["auto_submitted", "x_autoreply"]),
            Some(AutomatedClass::AutoReply)
        );
        assert_eq!(
            ev(&["auto_reply_subject"]),
            Some(AutomatedClass::OutOfOffice)
        );
    }
}
