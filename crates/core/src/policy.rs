//! Send policy ([Outbound › Policy pipeline]). M2 holds the loop limits that inbound classification
//! shares with the send path, and the idempotency fingerprint; the pipeline itself grows with the send
//! milestones.
//!
//! [Outbound › Policy pipeline]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use serde_json::Value;

/// `X-Pylota-Identity-Hop` at or above this makes inbound mail `automated`, and refuses a send
/// (`409 loop_detected`, N13).
pub const MAX_HOP: u32 = 10;

/// Sends per thread, and messages from one address, in 24 hours before step 10b refuses (N35).
pub const MAX_REPLIES_PER_DAY: u32 = 25;

/// The default `policy.auto_reply.max_automatic_exchanges` (D6).
pub const DEFAULT_MAX_AUTOMATIC_EXCHANGES: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendOp {
    Send,
    Reply,
    ReplyAll,
    Forward,
    CalendarReply,
}

impl SendOp {
    pub const fn as_str(self) -> &'static str {
        match self {
            SendOp::Send => "send",
            SendOp::Reply => "reply",
            SendOp::ReplyAll => "reply_all",
            SendOp::Forward => "forward",
            SendOp::CalendarReply => "calendar_reply",
        }
    }
}

/// Canonical JSON: keys sorted by their UTF-8 bytes at every level, no insignificant whitespace,
/// `serde_json`'s minimal string escapes and shortest numbers, arrays in order.
pub fn canonical_json(v: &Value) -> String {
    let mut out = String::new();
    write_canonical(v, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*k).clone()).to_string());
                out.push(':');
                write_canonical(&map[*k], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// `hex(sha256(canonical_json({"body": body, "operation": op, "target": target})))`; `target` is the
/// identity ID for `send`, `{identity_id}/{message_id}` otherwise.
pub fn send_fingerprint(operation: SendOp, target: &str, body: &Value) -> String {
    let v = serde_json::json!({ "body": body, "operation": operation.as_str(), "target": target });
    crate::ses::sha256_hex(canonical_json(&v).as_bytes())
}

/// The idempotency fingerprint of a non-mail request: `hex(sha256(canonical_json({method, path, body})))`.
pub fn request_fingerprint(method: &str, path: &str, body: &Value) -> String {
    let v = serde_json::json!({ "body": body, "method": method, "path": path });
    crate::ses::sha256_hex(canonical_json(&v).as_bytes())
}

// ---- Tenant policy: defaults, merge and who may change a field (Workspace policy § 2–4) -------------

/// The built-in tenant policy (Configuration › Tenant policy), before `PM_DEFAULT_POLICY`.
pub const DEFAULTS_JSON: &str = include_str!("../data/policy_defaults.json");

/// The built-in defaults as JSON.
pub fn builtin_defaults() -> Value {
    serde_json::from_str(DEFAULTS_JSON).expect("the built-in policy defaults parse")
}

/// Deep merge: objects merge key by key, `null` removes the key (so the default applies), anything
/// else (arrays included) replaces.
pub fn merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    b.remove(k);
                } else if v.is_object() && b.get(k).is_some_and(Value::is_object) {
                    merge(b.get_mut(k).expect("checked"), v);
                } else {
                    b.insert(k.clone(), v.clone());
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// Built-in defaults ⊕ `PM_DEFAULT_POLICY` ⊕ the stored `policy_json`.
pub fn effective(deployment_defaults: &Value, stored: &Value) -> Value {
    let mut out = deployment_defaults.clone();
    merge(&mut out, stored);
    out
}

/// The deployment default: built-in ⊕ `PM_DEFAULT_POLICY`.
pub fn deployment_defaults(default_policy: &Value) -> Value {
    let mut out = builtin_defaults();
    merge(&mut out, default_policy);
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldClass {
    PlatformOnly,
    PlatformOrPartner,
    LowerOnly,
    Guard,
    Free,
}

impl FieldClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            FieldClass::PlatformOnly => "platform_only",
            FieldClass::PlatformOrPartner => "partner_or_platform",
            FieldClass::LowerOnly => "lower_only",
            FieldClass::Guard => "guard",
            FieldClass::Free => "free",
        }
    }
}

use FieldClass::{Guard, LowerOnly, PlatformOnly, PlatformOrPartner};

/// Every field that is not free (Configuration › Who may change a field). A path ending in `.*` covers
/// every field under it.
pub const FIELDS: &[(&str, FieldClass)] = &[
    ("web_bot_auth.allowed", PlatformOnly),
    ("domains.allow_create_zone", PlatformOnly),
    ("domains.cloudflare_zones", PlatformOnly),
    ("inbound.trusted_arc_sealers", PlatformOnly),
    ("vault.allowed", PlatformOnly),
    ("quarantine.key_release", PlatformOrPartner),
    ("drafts.key_approval", PlatformOrPartner),
    ("phone.allowed", PlatformOrPartner),
    ("phone.key_approval", PlatformOrPartner),
    ("vault.key_approval", PlatformOrPartner),
    ("identity_daily_send_cap", LowerOnly),
    ("tenant_daily_send_cap", LowerOnly),
    ("max_recipients", LowerOnly),
    ("auto_reply.allowed", LowerOnly),
    ("auto_reply.max_automatic_exchanges", LowerOnly),
    ("inbound.per_sender_per_hour", LowerOnly),
    ("inbound.unauthenticated_per_hour", LowerOnly),
    ("inbound.per_tenant_per_hour", LowerOnly),
    ("inbound.extract_image_text", LowerOnly),
    ("retention.raw_days", LowerOnly),
    ("retention.events_days", LowerOnly),
    ("triage.enabled", LowerOnly),
    ("triage.daily_model_cap", LowerOnly),
    ("search.agentic_enabled", LowerOnly),
    ("search.agentic_daily_cap", LowerOnly),
    ("search.agentic_max_steps", LowerOnly),
    ("search.agentic_max_seconds", LowerOnly),
    ("abuse.complaint_rate_pause", LowerOnly),
    ("abuse.bounce_rate_pause", LowerOnly),
    ("abuse.tenant_complaint_rate_pause", LowerOnly),
    ("abuse.tenant_bounce_rate_pause", LowerOnly),
    ("accounts.require_approval", LowerOnly),
    ("phone.countries", LowerOnly),
    ("phone.max_numbers_per_identity", LowerOnly),
    ("phone.max_numbers_per_tenant", LowerOnly),
    ("phone.spend.*", LowerOnly),
    ("phone.sms.identity_daily_cap", LowerOnly),
    ("phone.sms.tenant_daily_cap", LowerOnly),
    ("phone.sms.tenant_monthly_cap", LowerOnly),
    ("phone.sms.allowed_destination_countries", LowerOnly),
    ("phone.calls.allowed_destination_countries", LowerOnly),
    ("phone.calls.concurrent_limit", LowerOnly),
    ("phone.calls.max_minutes", LowerOnly),
    ("vault.max_items", LowerOnly),
    ("vault.session_max_ttl_days", LowerOnly),
    ("vault.approval_deadline_seconds", LowerOnly),
    ("send_allowlist_only", Guard),
    ("quarantine.on_auth_fail", Guard),
    ("quarantine.spam_threshold", Guard),
    ("quarantine.unsolicited_otp", Guard),
    ("drafts.require_approval", Guard),
    ("screening.mode", Guard),
    ("screening.on_unavailable", Guard),
    ("screening.deadline_seconds", Guard),
    ("phone.auto_approve", Guard),
    ("phone.calls.outbound", Guard),
    ("phone.recording", Guard),
    ("phone.disclosure.calls", Guard),
    ("phone.disclosure.first_sms", Guard),
    ("phone.verification.redact", Guard),
    ("vault.min_approval", Guard),
    ("vault.codes_into_fills_only", Guard),
];

/// Fields whose whole object value is one field (not walked into).
const OBJECT_FIELDS: &[&str] = &["phone.auto_approve"];

/// The class of a dotted path.
pub fn class_of(path: &str) -> FieldClass {
    FIELDS
        .iter()
        .find(|(f, _)| match f.strip_suffix(".*") {
            Some(prefix) => path.starts_with(prefix) && path[prefix.len()..].starts_with('.'),
            None => *f == path,
        })
        .map_or(FieldClass::Free, |(_, c)| *c)
}

/// The value at a dotted path.
pub fn get_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |v, k| v.get(k))
}

/// The leaves of a patch as `(dotted path, value)`, sorted by path. `Err(path)` names the first key the
/// policy does not have.
pub fn patch_leaves(patch: &Value, defaults: &Value) -> Result<Vec<(String, Value)>, String> {
    fn walk(
        p: &Value,
        d: Option<&Value>,
        prefix: &str,
        out: &mut Vec<(String, Value)>,
    ) -> Result<(), String> {
        let Value::Object(map) = p else { return Ok(()) };
        for (k, v) in map {
            let path = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{prefix}.{k}")
            };
            let dv = d.and_then(|d| d.get(k));
            let parent_is_object = d.is_some_and(Value::is_object);
            if parent_is_object
                && dv.is_none()
                && !OBJECT_FIELDS
                    .iter()
                    .any(|o| path.starts_with(&format!("{o}.")))
            {
                return Err(path);
            }
            let walk_in = v.is_object()
                && dv.is_some_and(Value::is_object)
                && !OBJECT_FIELDS.contains(&path.as_str());
            if walk_in {
                walk(v, dv, &path, out)?;
            } else {
                out.push((path, v.clone()));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(patch, Some(defaults), "", &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Who writes the policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    Platform,
    /// The tenant's own partner's key.
    Partner,
    /// A tenant key holding `policy:write`.
    WorkspaceKey,
    /// A console owner or admin.
    WorkspacePerson,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CeilingSource {
    Deployment,
    Platform,
    Partner,
}

impl CeilingSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            CeilingSource::Deployment => "deployment",
            CeilingSource::Platform => "platform",
            CeilingSource::Partner => "partner",
        }
    }
}

/// Why a write was refused; the first refused path (in sorted order) decides.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    /// `403 scope_denied`; `reason = "not_writable"` for a workspace writer.
    NotWritable { field: String },
    /// `403 scope_denied`; for a workspace writer with `reason = "above_ceiling"`, the ceiling and its source.
    AboveCeiling {
        field: String,
        ceiling: Value,
        source: CeilingSource,
    },
    /// `403 permission_denied`, `reason = "person_required"`.
    PersonRequired { field: String },
}

/// Switches whose `false` is the looser value.
const FALSE_IS_LOOSER: &[&str] = &["accounts.require_approval"];

/// `a` is looser than `b` for a lower-only field.
pub fn looser(field: &str, a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => false,
        (Value::Null, _) => true,
        (_, Value::Null) => false,
        (Value::Number(x), Value::Number(y)) => x.as_f64() > y.as_f64(),
        (Value::Bool(x), Value::Bool(y)) => {
            if FALSE_IS_LOOSER.contains(&field) {
                !x && *y
            } else {
                *x && !y
            }
        }
        (Value::Array(x), Value::Array(y)) => x.iter().any(|v| !y.contains(v)),
        _ => a != b,
    }
}

const ORDERS: &[(&str, &[&str])] = &[
    (
        "drafts.require_approval",
        &["never", "new_recipients", "external", "always"],
    ),
    ("screening.mode", &["off", "monitor", "hold"]),
    ("vault.min_approval", &["grant", "per_run", "per_use"]),
];

/// Booleans whose `true` is the stricter value.
const TRUE_IS_STRICTER: &[&str] = &[
    "send_allowlist_only",
    "quarantine.on_auth_fail",
    "quarantine.unsolicited_otp",
    "phone.verification.redact",
    "vault.codes_into_fills_only",
];

/// A guard field's `new` value is at least as strict as `current`.
pub fn tightens(field: &str, new: &Value, current: &Value, effective_after: &Value) -> bool {
    if new == current {
        return true;
    }
    if TRUE_IS_STRICTER.contains(&field) {
        return new == &Value::Bool(true);
    }
    if let Some((_, order)) = ORDERS.iter().find(|(f, _)| *f == field) {
        let pos = |v: &Value| v.as_str().and_then(|s| order.iter().position(|o| *o == s));
        return matches!((pos(new), pos(current)), (Some(n), Some(c)) if n >= c);
    }
    match field {
        "quarantine.spam_threshold" => {
            matches!((new.as_f64(), current.as_f64()), (Some(n), Some(c)) if n <= c)
        }
        "screening.on_unavailable" => new == "quarantine",
        "screening.deadline_seconds" => {
            get_path(effective_after, "screening.on_unavailable")
                == Some(&Value::from("quarantine"))
                || matches!((new.as_f64(), current.as_f64()), (Some(n), Some(c)) if n >= c)
        }
        _ => false, // any other change of a guard object or enum is a loosening a person decides
    }
}

/// Lower-only ceilings as flat maps of dotted path → value.
pub struct Ceilings<'a> {
    pub deployment: &'a Value,
    pub platform: &'a Value,
    pub partner: &'a Value,
}

impl Ceilings<'_> {
    /// The strictest bound for `field` and the writer, with its source (on a tie, the first of
    /// deployment, platform, partner).
    pub fn ceiling(&self, writer: Writer, field: &str) -> (Value, CeilingSource) {
        let mut best = (
            get_path(self.deployment, field)
                .cloned()
                .unwrap_or(Value::Null),
            CeilingSource::Deployment,
        );
        let mut bounds = vec![(self.platform.get(field), CeilingSource::Platform)];
        if matches!(writer, Writer::WorkspaceKey | Writer::WorkspacePerson) {
            bounds.push((self.partner.get(field), CeilingSource::Partner));
        }
        for (v, source) in bounds {
            if let Some(v) = v
                && looser(field, &best.0, v)
            {
                best = (v.clone(), source);
            }
        }
        best
    }
}

/// Checks a policy patch against the field classes (Workspace policy § 4 step 3). `effective` is the
/// tenant's policy before the write; `key_may_decide` whether API keys may take decisions reserved for
/// people on this tenant.
pub fn check_write(
    writer: Writer,
    patch: &Value,
    effective: &Value,
    ceilings: &Ceilings<'_>,
    key_may_decide: bool,
) -> Result<(), Refusal> {
    if writer == Writer::Platform {
        return Ok(());
    }
    let leaves =
        patch_leaves(patch, ceilings.deployment).map_err(|field| Refusal::NotWritable { field })?;
    let mut after = effective.clone();
    merge(&mut after, patch);
    for (field, value) in leaves {
        match class_of(&field) {
            PlatformOnly => return Err(Refusal::NotWritable { field }),
            PlatformOrPartner if writer != Writer::Partner => {
                return Err(Refusal::NotWritable { field });
            }
            PlatformOrPartner | FieldClass::Free => {}
            LowerOnly => {
                let value = if value.is_null() {
                    get_path(ceilings.deployment, &field)
                        .cloned()
                        .unwrap_or(Value::Null)
                } else {
                    value
                };
                let (ceiling, source) = ceilings.ceiling(writer, &field);
                if looser(&field, &value, &ceiling) {
                    return Err(Refusal::AboveCeiling {
                        field,
                        ceiling,
                        source,
                    });
                }
            }
            Guard => {
                if writer == Writer::WorkspaceKey && !key_may_decide {
                    let value = if value.is_null() {
                        get_path(ceilings.deployment, &field)
                            .cloned()
                            .unwrap_or(Value::Null)
                    } else {
                        value
                    };
                    let current = get_path(effective, &field).cloned().unwrap_or(Value::Null);
                    if !tightens(&field, &value, &current, &after) {
                        return Err(Refusal::PersonRequired { field });
                    }
                }
            }
        }
    }
    Ok(())
}

/// The stored policy and ceilings after a write that passed [`check_write`] (§ 4 step 4).
pub fn apply_write(
    writer: Writer,
    stored: &Value,
    patch: &Value,
    platform_ceilings: &Value,
    partner_ceilings: &Value,
    defaults: &Value,
) -> (Value, Value, Value) {
    let mut policy = stored.clone();
    merge(&mut policy, patch);
    let (mut pc, mut ptc) = (platform_ceilings.clone(), partner_ceilings.clone());
    let target = match writer {
        Writer::Platform => Some(&mut pc),
        Writer::Partner => Some(&mut ptc),
        _ => None,
    };
    if let Some(map) = target.and_then(Value::as_object_mut) {
        for (field, value) in patch_leaves(patch, defaults).unwrap_or_default() {
            if class_of(&field) == LowerOnly {
                if value.is_null() {
                    map.remove(&field);
                } else {
                    map.insert(field, value);
                }
            }
        }
    }
    (policy, pc, ptc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_order_and_whitespace() {
        let a: Value =
            serde_json::from_str(r#"{ "b": [3, {"z":1,"a":"é\n"}], "a": 1.50, "B": null }"#)
                .unwrap();
        assert_eq!(
            canonical_json(&a),
            r#"{"B":null,"a":1.5,"b":[3,{"a":"é\n","z":1}]}"#
        );
        let b: Value =
            serde_json::from_str(r#"{"a":1.5,"B":null,"b":[3,{"a":"é\n","z":1}]}"#).unwrap();
        let f = |v: &Value| send_fingerprint(SendOp::Send, "idn_1", v);
        assert_eq!(f(&a), f(&b));
        assert_ne!(f(&a), send_fingerprint(SendOp::Reply, "idn_1", &a));
        assert_ne!(f(&a), f(&json!({"a": 1.5})));
        assert_eq!(f(&a).len(), 64);
    }

    fn view(defaults: &Value) -> (Value, Value) {
        (deployment_defaults(defaults), json!({}))
    }

    /// Each class of Configuration › Who may change a field, for each writer.
    #[test]
    fn workspace_write_table() {
        let (dep, none) = view(&json!({}));
        let eff = dep.clone();
        let c = Ceilings {
            deployment: &dep,
            platform: &none,
            partner: &none,
        };
        let check = |w, p: Value, decide| check_write(w, &p, &eff, &c, decide);

        // Platform: everything.
        assert_eq!(
            check(
                Writer::Platform,
                json!({ "web_bot_auth": { "allowed": true }, "tenant_daily_send_cap": 99999 }),
                false
            ),
            Ok(())
        );
        // Platform-only fields.
        for w in [
            Writer::Partner,
            Writer::WorkspaceKey,
            Writer::WorkspacePerson,
        ] {
            assert_eq!(
                check(w, json!({ "domains": { "allow_create_zone": true } }), true),
                Err(Refusal::NotWritable {
                    field: "domains.allow_create_zone".into()
                })
            );
        }
        // Platform or own partner.
        assert_eq!(
            check(
                Writer::Partner,
                json!({ "quarantine": { "key_release": true } }),
                false
            ),
            Ok(())
        );
        assert_eq!(
            check(
                Writer::WorkspaceKey,
                json!({ "drafts": { "key_approval": true } }),
                true
            ),
            Err(Refusal::NotWritable {
                field: "drafts.key_approval".into()
            })
        );
        // Lower-only: at or below the ceiling; null is the deployment default.
        assert_eq!(
            check(
                Writer::WorkspaceKey,
                json!({ "tenant_daily_send_cap": 4000 }),
                false
            ),
            Ok(())
        );
        assert_eq!(
            check(
                Writer::Partner,
                json!({ "tenant_daily_send_cap": 5001 }),
                false
            ),
            Err(Refusal::AboveCeiling {
                field: "tenant_daily_send_cap".into(),
                ceiling: json!(5000),
                source: CeilingSource::Deployment
            })
        );
        assert_eq!(
            check(
                Writer::WorkspaceKey,
                json!({ "triage": { "enabled": false } }),
                false
            ),
            Ok(())
        );
        assert_eq!(
            check(
                Writer::WorkspaceKey,
                json!({ "accounts": { "require_approval": true } }),
                false
            ),
            Ok(())
        );
        // Guard: a key may only tighten unless keys may decide; a person may do anything.
        assert_eq!(
            check(
                Writer::WorkspaceKey,
                json!({ "screening": { "mode": "hold" } }),
                false
            ),
            Ok(())
        );
        assert_eq!(
            check(
                Writer::WorkspaceKey,
                json!({ "quarantine": { "on_auth_fail": false } }),
                false
            ),
            Err(Refusal::PersonRequired {
                field: "quarantine.on_auth_fail".into()
            })
        );
        assert_eq!(
            check(
                Writer::WorkspaceKey,
                json!({ "quarantine": { "on_auth_fail": false } }),
                true
            ),
            Ok(())
        );
        assert_eq!(
            check(
                Writer::WorkspacePerson,
                json!({ "screening": { "mode": "off" } }),
                false
            ),
            Ok(())
        );
        assert_eq!(
            check(
                Writer::WorkspaceKey,
                json!({ "quarantine": { "spam_threshold": 0.5 } }),
                false
            ),
            Ok(())
        );
        assert!(
            check(
                Writer::WorkspaceKey,
                json!({ "quarantine": { "spam_threshold": 0.9 } }),
                false
            )
            .is_err()
        );
        assert!(
            check(
                Writer::WorkspaceKey,
                json!({ "drafts": { "require_approval": "always" } }),
                false
            )
            .is_ok()
        );
        // Free fields and unknown fields.
        assert_eq!(
            check(
                Writer::WorkspaceKey,
                json!({ "link_ttl_hours": 24, "retention": { "message_days": 30 } }),
                false
            ),
            Ok(())
        );
        assert_eq!(
            check(Writer::Partner, json!({ "nope": 1 }), false),
            Err(Refusal::NotWritable {
                field: "nope".into()
            })
        );
        // The first refused path in sorted order decides.
        assert_eq!(
            check(
                Writer::WorkspaceKey,
                json!({ "web_bot_auth": { "allowed": true }, "domains": { "cloudflare_zones": [] } }),
                true
            ),
            Err(Refusal::NotWritable {
                field: "domains.cloudflare_zones".into()
            })
        );
    }

    /// Platform and partner ceilings: a workspace writer is bound by the strictest of the three, a
    /// partner by the first two (J23, J26).
    #[test]
    fn ceilings_and_apply() {
        let dep = deployment_defaults(&json!({}));
        let pc = json!({ "tenant_daily_send_cap": 3000 });
        let ptc = json!({ "tenant_daily_send_cap": 1000 });
        let c = Ceilings {
            deployment: &dep,
            platform: &pc,
            partner: &ptc,
        };
        assert_eq!(
            c.ceiling(Writer::Partner, "tenant_daily_send_cap"),
            (json!(3000), CeilingSource::Platform)
        );
        assert_eq!(
            c.ceiling(Writer::WorkspaceKey, "tenant_daily_send_cap"),
            (json!(1000), CeilingSource::Partner)
        );
        let eff = dep.clone();
        assert!(
            check_write(
                Writer::Partner,
                &json!({ "tenant_daily_send_cap": 2500 }),
                &eff,
                &c,
                false
            )
            .is_ok()
        );
        assert_eq!(
            check_write(
                Writer::WorkspaceKey,
                &json!({ "tenant_daily_send_cap": null }),
                &eff,
                &c,
                false
            ),
            Err(Refusal::AboveCeiling {
                field: "tenant_daily_send_cap".into(),
                ceiling: json!(1000),
                source: CeilingSource::Partner
            })
        );

        let (policy, pc2, ptc2) = apply_write(
            Writer::Platform,
            &json!({ "max_recipients": 5 }),
            &json!({ "tenant_daily_send_cap": 2000, "max_recipients": null, "link_ttl_hours": 12 }),
            &pc,
            &ptc,
            &dep,
        );
        assert_eq!(
            policy,
            json!({ "tenant_daily_send_cap": 2000, "link_ttl_hours": 12 })
        );
        assert_eq!(pc2, json!({ "tenant_daily_send_cap": 2000 }));
        assert_eq!(ptc2, ptc);
        let (_, _, ptc3) = apply_write(
            Writer::Partner,
            &json!({}),
            &json!({ "tenant_daily_send_cap": null }),
            &pc,
            &ptc,
            &dep,
        );
        assert_eq!(ptc3, json!({}));
        assert_eq!(
            get_path(
                &effective(&dep, &json!({ "triage": { "enabled": false } })),
                "triage.daily_model_cap"
            ),
            Some(&json!(2000))
        );
    }
}
