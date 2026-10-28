//! Identities (Identities and domains › Identities): create, list, lookup, get, update (pause and
//! resume included) and delete. Every change that the identity's mailbox must hear about is written to
//! `rpc_intents` in the change's D1 batch and sent after commit (Design conventions § 9).

use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::address::{self, AddressError, DomainClass};
use pylota_identity_core::policy::canonical_json;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::{ControlDb as _, Ids as _, ObjectClient as _, Platform};
use serde_json::{Value, json};

use super::{identity_in_scope, tenant_in_scope};
use crate::auth::Level;
use crate::ctx::{Ctx, Reply};
use crate::db::addresses as adb;
use crate::db::domains as ddb;
use crate::db::identities as idb;
use crate::db::tenants as tdb;
use crate::db::{audit, intents, jobs};
use crate::errors::ApiError;
use crate::policy;
use crate::quota::{Feature, HoldAnswer, QuotaRequest};
use crate::rpc::{RpcResult, envelope_for};
use crate::webhooks::envelope as ev;

const CREATE_FIELDS: &[&str] = &[
    "username",
    "display_name",
    "purpose",
    "owner",
    "signature",
    "domain_id",
    "client_id",
    "send_policy",
    "retention",
    "metadata",
];
const UPDATE_FIELDS: &[&str] = &[
    "display_name",
    "purpose",
    "owner",
    "signature",
    "metadata",
    "send_policy",
    "retention",
    "status",
];

pub(crate) fn address_error(e: AddressError) -> ApiError {
    let (code, msg) = match e {
        AddressError::Reserved => (ErrorCode::AddressReserved, "This name is reserved."),
        AddressError::Unsupported => (
            ErrorCode::AddressUnsupported,
            "Only ASCII local parts can be routed.",
        ),
        AddressError::Invalid => (ErrorCode::AddressInvalid, "This is not a valid username."),
        AddressError::LocalPartTooLong => (
            ErrorCode::LocalPartTooLong,
            "Username and tenant suffix are longer than 40 characters.",
        ),
        AddressError::Taken => (ErrorCode::UsernameTaken, "This username is taken."),
    };
    ApiError::new(code, msg).details(json!({ "field": "username" }))
}

fn taken() -> ApiError {
    ApiError::new(ErrorCode::AddressTaken, "This address is taken.")
}

/// The validated values of the fields a create or update may carry.
#[derive(Default)]
struct Fields {
    display_name: Option<String>,
    purpose: Option<Option<String>>,
    owner: Option<(String, String)>,
    signature: Option<(Option<String>, Option<String>)>,
    metadata: Option<Value>,
    send_policy: Option<Value>,
    message_hours: Option<Option<i64>>,
}

fn str_of<'a>(v: &'a Value, field: &str) -> Result<&'a str, ApiError> {
    v.as_str()
        .ok_or_else(|| ApiError::invalid(field, format!("{field} is a string.")))
}

fn valid_email(s: &str) -> bool {
    s.len() <= 254
        && s.split_once('@').is_some_and(|(l, d)| {
            !l.is_empty()
                && l.len() <= 64
                && d.contains('.')
                && !d.starts_with('.')
                && !d.ends_with('.')
        })
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '<' || c == '>')
        && s.matches('@').count() == 1
}

/// Step 1 of *Create* (and the same rules for `PATCH`): shapes, sizes and the caps a key may set.
fn validate<P: Platform>(
    c: &Ctx<P>,
    t: &tdb::Row,
    body: &Value,
    allowed: &[&str],
    current_hours: Option<i64>,
) -> Result<Fields, ApiError> {
    let Some(obj) = body.as_object() else {
        return Err(ApiError::invalid("", "The body must be a JSON object."));
    };
    if let Some(k) = obj.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(ApiError::invalid(
            k,
            format!("{k} is not a field of this request."),
        ));
    }
    let mut f = Fields::default();
    if let Some(v) = obj.get("display_name") {
        let s = str_of(v, "display_name")?;
        let n = s.chars().count();
        if n == 0 || n > 78 || s.contains(['\r', '\n']) {
            return Err(ApiError::invalid(
                "display_name",
                "display_name is 1–78 characters without line breaks.",
            ));
        }
        f.display_name = Some(s.to_owned());
    }
    if let Some(v) = obj.get("purpose") {
        f.purpose = Some(match v {
            Value::Null => None,
            v => {
                let s = str_of(v, "purpose")?;
                if s.is_empty() || s.chars().count() > 64 || s.contains(['\r', '\n']) {
                    return Err(ApiError::invalid("purpose", "purpose is 1–64 characters."));
                }
                Some(s.to_owned())
            }
        });
    }
    if let Some(v) = obj.get("owner") {
        let name = str_of(&v["name"], "owner.name")?;
        let email = str_of(&v["email"], "owner.email")?
            .trim()
            .to_ascii_lowercase();
        if name.trim().is_empty() || name.chars().count() > 200 || name.contains(['\r', '\n']) {
            return Err(ApiError::invalid(
                "owner.name",
                "owner.name is 1–200 characters.",
            ));
        }
        if !valid_email(&email) {
            return Err(ApiError::invalid(
                "owner.email",
                "owner.email is not an address.",
            ));
        }
        f.owner = Some((name.to_owned(), email));
    }
    if let Some(v) = obj.get("signature") {
        let text = match &v["text"] {
            Value::Null => None,
            t => Some(str_of(t, "signature.text")?.to_owned()),
        };
        let html = match &v["html"] {
            Value::Null => None,
            h => Some(pylota_identity_core::sanitize::sanitize(str_of(h, "signature.html")?).0),
        };
        if text.as_ref().is_some_and(|t| t.len() > 4096)
            || html.as_ref().is_some_and(|h| h.len() > 16_384)
        {
            return Err(ApiError::invalid("signature", "The signature is too long."));
        }
        f.signature = Some((text, html));
    }
    if let Some(v) = obj.get("metadata") {
        let Some(m) = v.as_object() else {
            return Err(ApiError::invalid("metadata", "metadata is an object."));
        };
        if m.len() > 16 || m.values().any(|x| x.as_str().is_none_or(|s| s.len() > 512)) {
            return Err(ApiError::invalid(
                "metadata",
                "metadata has at most 16 keys, each a string of at most 512 bytes.",
            ));
        }
        f.metadata = Some(v.clone());
    }
    if let Some(v) = obj.get("send_policy") {
        let Some(sp) = v.as_object() else {
            return Err(ApiError::invalid(
                "send_policy",
                "send_policy is an object.",
            ));
        };
        for (k, x) in sp {
            let ok = match k.as_str() {
                "daily_cap" => x.as_i64().is_some_and(|n| n >= 1),
                "auto_reply" => matches!(x.as_str(), Some("allowed" | "denied")),
                "require_known_recipient" => x.is_boolean(),
                "require_approval" => {
                    x.is_null()
                        || matches!(
                            x.as_str(),
                            Some("never" | "new_recipients" | "external" | "always")
                        )
                }
                _ => false,
            };
            if !ok {
                return Err(ApiError::invalid(
                    &format!("send_policy.{k}"),
                    "Not a valid value.",
                ));
            }
        }
        if let Some(cap) = sp.get("daily_cap").and_then(Value::as_i64) {
            let eff = policy::effective(c.p.as_ref(), t);
            let ceiling = eff["identity_daily_send_cap"].as_i64().unwrap_or(i64::MAX);
            if cap > ceiling && c.key().level != Level::Platform {
                return Err(ApiError::scope_denied(Some("send_policy.daily_cap"))
                    .details(json!({ "field": "send_policy.daily_cap", "ceiling": ceiling })));
            }
        }
        f.send_policy = Some(v.clone());
    }
    if let Some(r) = obj.get("retention") {
        let h = match &r["message_hours"] {
            Value::Null => None,
            v => match v.as_i64() {
                Some(n) if (1..=8760).contains(&n) => Some(n),
                _ => {
                    return Err(ApiError::invalid(
                        "retention.message_hours",
                        "retention.message_hours is null or 1–8760.",
                    ));
                }
            },
        };
        // Setting a number where there was none, or lowering it, erases mail sooner.
        let sooner = match (h, current_hours) {
            (Some(n), Some(cur)) => n < cur,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if sooner && !c.key().has("erasure:manage") {
            return Err(ApiError::permission_denied(&["erasure:manage"]));
        }
        f.message_hours = Some(h);
    }
    Ok(f)
}

/// An `emit_event` intent of an identity: its `event_id` is derived from the intent ID when it is sent.
fn event_intent(
    request_id: &str,
    row: &idb::Row,
    event_type: &str,
    data: &Value,
    now: i64,
) -> Stmt {
    let body = json!({ "op": "emit_event", "event_id": "", "event_type": event_type, "occurred_at": now, "data": data });
    intents::insert(
        &format!("{request_id}:{}:{event_type}", row.id),
        "mailbox",
        &row.mailbox_do_id,
        &row.tenant_id,
        Some(&row.id),
        "emit_event",
        &body.to_string(),
        now,
    )
}

/// Sends a request to the tenant's `TenantQuota`.
async fn quota<P: Platform>(
    c: &Ctx<P>,
    t: &tdb::Row,
    req: QuotaRequest,
) -> Result<Value, ApiError> {
    let env = envelope_for(
        c.p.as_ref(),
        Some(&t.id),
        None,
        &c.request_id,
        Some(&c.key().key_id),
        req,
    );
    match c
        .p
        .objects()
        .call::<_, Value>(DoClass::Quota, &t.quota_do_id, &env)
        .await?
    {
        RpcResult::Ok(v) => Ok(v),
        RpcResult::Err(_) => Err(ApiError::new(
            ErrorCode::Unavailable,
            "The tenant's quota is not ready; retry.",
        )),
    }
}

async fn addresses_of<P: Platform>(c: &Ctx<P>, id: &str) -> Result<Vec<adb::Row>, ApiError> {
    Ok(c.p.db().query(adb::of_identity(id)).await?)
}

async fn render<P: Platform>(c: &Ctx<P>, row: &idb::Row) -> Result<Value, ApiError> {
    Ok(row.json(&addresses_of(c, &row.id).await?))
}

/// `client_fingerprint`: hex SHA-256 of the canonical JSON body (FR-IDN-1).
fn fingerprint(body: &Value) -> String {
    pylota_identity_core::ses::sha256_hex(canonical_json(body).as_bytes())
}

pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let key = c.key().clone();
    // A18: the default tenant's addresses stand alone on the platform domain, so only a platform key
    // creates identities there, whatever the key's scope (checked before it: the default tenant is no
    // secret).
    let named: Option<tdb::Row> = c.p.db().first(tdb::get(c.route.param("tenant_id"))).await?;
    if named.is_some_and(|t| t.address_suffix.is_empty()) && key.level != Level::Platform {
        return Err(ApiError::scope_denied(None).details(json!({ "reason": "default_tenant" })));
    }
    let t = tenant_in_scope(c, c.route.param("tenant_id"), true).await?;
    let body = c.json()?;
    let f = validate(c, &t, &body, CREATE_FIELDS, None)?;
    let username_in = str_of(&body["username"], "username")?;
    let Some(display_name) = f.display_name.clone() else {
        return Err(ApiError::invalid(
            "display_name",
            "display_name is required.",
        ));
    };

    // Step 2: client_id decides between create, replay and conflict.
    let client_id = match body.get("client_id") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let s = str_of(v, "client_id")?;
            if s.is_empty() || s.len() > 200 {
                return Err(ApiError::invalid(
                    "client_id",
                    "client_id is 1–200 characters.",
                ));
            }
            Some(s.to_owned())
        }
    };
    let print = fingerprint(&body);
    if let Some(cid) = &client_id {
        let existing: Option<idb::Row> = c.p.db().first(idb::by_client_id(&t.id, cid)).await?;
        if let Some(e) = existing {
            if e.client_fingerprint.as_deref() != Some(print.as_str()) {
                return Err(ApiError::new(
                    ErrorCode::ClientIdConflict,
                    "Another identity has this client_id.",
                ));
            }
            if !e.mailbox_do_id.is_empty() {
                crate::intents::flush_object(c.p.as_ref(), &c.request_id, &e.mailbox_do_id).await;
            }
            return Ok(Reply::json(200, render(c, &e).await?));
        }
    }

    // Step 3: the domain. Tenant domains arrive with M13; until then only the platform domain exists.
    let platform: ddb::Row = c.p.db().first(ddb::platform()).await?.ok_or_else(|| {
        ApiError::new(ErrorCode::Unavailable, "The platform domain is not set up.")
    })?;
    if let Some(d) = body.get("domain_id").filter(|d| !d.is_null()) {
        let d = str_of(d, "domain_id")?;
        let row: Option<ddb::Row> = c.p.db().first(ddb::get(d)).await?;
        match row {
            Some(r) if r.kind == "platform" => {}
            Some(r) if r.tenant_id.as_deref() == Some(t.id.as_str()) => {
                return Err(ApiError::new(
                    ErrorCode::DomainNotReady,
                    "Addresses on tenant domains arrive with their milestone.",
                ));
            }
            _ => return Err(ApiError::not_found(ErrorCode::DomainNotFound)),
        }
    }

    // Username (Identities and domains › Username validation).
    let existing: Vec<idb::Username> = c.p.db().query(idb::usernames(&t.id)).await?;
    let names: Vec<&str> = existing.iter().map(|u| u.username.as_str()).collect();
    let class = if t.address_suffix.is_empty() {
        DomainClass::Platform
    } else {
        DomainClass::Tenant
    };
    let username = address::validate_username(username_in, class, 24, &t.address_suffix, &names)
        .map_err(address_error)?;

    // Step 4: the platform address, free, not tombstoned and not a person's sign-in address.
    let cfg = c.p.config();
    let local = format!("{username}{}", t.address_suffix);
    let addr = format!("{local}@{}", cfg.platform_domain);
    let hash = address::address_hash(&c.p.secrets().hash_key, &addr);
    let in_use: Option<adb::Row> = c.p.db().first(adb::by_address(&addr)).await?;
    let tomb: Option<adb::Tombstone> = c.p.db().first(adb::tombstone(&hash)).await?;
    let user: Option<super::Count> = c.p.db().first(adb::is_user_email(&addr)).await?;
    if in_use.is_some() || tomb.is_some() || user.is_some_and(|u| u.n > 0) {
        return Err(taken());
    }

    // Step 5 and 6: the mailbox ID and the inboxes hold.
    let ids = c.p.ids();
    let idn = ids.new_id(IdPrefix::Identity);
    let mailbox_do_id = c.p.objects().new_object_id(DoClass::Mailbox)?;
    let hold = quota(
        c,
        &t,
        QuotaRequest::Hold {
            feature: Feature::Inboxes,
            units: 1,
            r#ref: idn.clone(),
            gates: vec![Feature::StorageGb],
        },
    )
    .await?;
    if let Ok(HoldAnswer::Denied(d)) = serde_json::from_value::<HoldAnswer>(hold) {
        return Err(ApiError::new(
            ErrorCode::BillingLimit,
            "The plan's inboxes allowance is spent.",
        )
        .details(json!({ "feature": "inboxes", "granted": d.granted, "used": d.used })));
    }

    // Step 7: the rows, the audit row and the mailbox's init intent in one batch.
    let now = c.now;
    let (owner_name, owner_email) = f.owner.clone().unzip();
    let (sig_text, sig_html) = f.signature.clone().unwrap_or_default();
    let row = idb::Row {
        id: idn.clone(),
        tenant_id: t.id.clone(),
        username: username.clone(),
        display_name,
        purpose: f.purpose.clone().flatten(),
        client_id: client_id.clone(),
        client_fingerprint: client_id.as_ref().map(|_| print.clone()),
        owner_name,
        owner_email,
        signature_text: sig_text,
        signature_html: sig_html,
        status: "active".into(),
        pause_reason: None,
        send_policy_json: f
            .send_policy
            .clone()
            .unwrap_or_else(|| json!({}))
            .to_string(),
        message_hours: f.message_hours.flatten(),
        metadata_json: f.metadata.clone().unwrap_or_else(|| json!({})).to_string(),
        mailbox_do_id: mailbox_do_id.clone(),
        is_system: 0,
        created_at: now,
        updated_at: now,
    };
    let adr = adb::Row {
        id: ids.new_id(IdPrefix::Address),
        address: addr.clone(),
        local_part: local.clone(),
        domain_id: platform.id.clone(),
        tenant_id: t.id.clone(),
        identity_id: idn.clone(),
        role: "primary".into(),
        status: "active".into(),
        routing_rule_id: None,
        retire_at: None,
        retired_at: None,
        ses_bounce_rule: None,
        forwarding: None,
        forwarding_checked_at: None,
        created_at: now,
    };
    let identity = row.json(std::slice::from_ref(&adr));
    let init = json!({
        "op": "init", "created_at": now, "message_hours": row.message_hours, "is_system": false,
        "created_event": { "event_id": "", "event_type": "identity.created", "occurred_at": now, "data": ev::identity_created(&identity) },
    });
    let batch = vec![
        Stmt::new(
            "INSERT INTO identities (id, tenant_id, username, display_name, purpose, client_id, client_fingerprint, owner_name, owner_email, signature_text, signature_html, status, send_policy_json, message_hours, metadata_json, mailbox_do_id, is_system, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'active', ?12, ?13, ?14, ?15, 0, ?16, ?16)",
            vec![
                row.id.clone().into(),
                row.tenant_id.clone().into(),
                row.username.clone().into(),
                row.display_name.clone().into(),
                row.purpose.clone().into(),
                row.client_id.clone().into(),
                row.client_fingerprint.clone().into(),
                row.owner_name.clone().into(),
                row.owner_email.clone().into(),
                row.signature_text.clone().into(),
                row.signature_html.clone().into(),
                row.send_policy_json.clone().into(),
                row.message_hours.into(),
                row.metadata_json.clone().into(),
                row.mailbox_do_id.clone().into(),
                now.into(),
            ],
        ),
        adb::insert(
            &adr.id,
            &addr,
            &local,
            &platform.id,
            &t.id,
            &idn,
            "primary",
            "active",
            now,
        ),
        audit::insert(
            &ids.new_id(IdPrefix::Audit),
            Some(&t.id),
            Some(&key.key_id),
            "identity.create",
            "identity",
            &idn,
            &json!({ "username": username, "client_id": client_id }),
            &c.request_id,
            now,
        ),
        intents::insert(
            &format!("{}:{idn}:init", c.request_id),
            "mailbox",
            &mailbox_do_id,
            &t.id,
            Some(&idn),
            "init",
            &init.to_string(),
            now,
        ),
    ];
    let committed = c.p.db().batch(batch).await;
    let settle = |consume: u32| QuotaRequest::Settle {
        feature: Feature::Inboxes,
        r#ref: idn.clone(),
        consume,
        keep: 0,
    };
    if let Err(e) = committed {
        let _ = quota(c, &t, settle(0)).await;
        let detail = e.detail.clone();
        return Err(
            if e.kind == pylota_identity_platform::PlatformErrorKind::Conflict {
                if detail.contains("addresses") {
                    taken()
                } else if detail.contains("client") {
                    ApiError::new(
                        ErrorCode::ClientIdConflict,
                        "Another identity has this client_id.",
                    )
                } else {
                    address_error(AddressError::Taken)
                }
            } else {
                e.into()
            },
        );
    }
    let _ = quota(c, &t, settle(1)).await;
    // Step 8: Init, which emits identity.created; a failure is left to the cron (J21).
    crate::intents::flush_object(c.p.as_ref(), &c.request_id, &mailbox_do_id).await;
    Ok(Reply::json(201, identity))
}

/// The page's identities rendered with their addresses (one D1 read for all of them).
async fn render_page<P: Platform>(
    c: &Ctx<P>,
    rows: Vec<idb::Row>,
    limit: u32,
    filters: &[&str],
) -> Result<Reply, ApiError> {
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    let addrs: Vec<adb::Row> =
        c.p.db()
            .query(adb::of_identities(
                &serde_json::to_string(&ids).unwrap_or_default(),
            ))
            .await?;
    c.page(
        rows,
        limit,
        filters,
        |r| (r.created_at, r.id.clone()),
        |r| {
            let mine: Vec<adb::Row> = addrs
                .iter()
                .filter(|a| a.identity_id == r.id)
                .cloned()
                .collect();
            r.json(&mine)
        },
    )
    .await
}

fn status_filter<P: Platform>(c: &Ctx<P>) -> Result<Option<&str>, ApiError> {
    match c.query("status") {
        None => Ok(None),
        Some(s @ ("active" | "paused" | "deleting" | "deleted")) => Ok(Some(s)),
        Some(_) => Err(ApiError::invalid("status", "Unknown status.")),
    }
}

pub async fn list_tenant<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = tenant_in_scope(c, c.route.param("tenant_id"), false).await?;
    let limit = c.limit()?;
    const F: &[&str] = &["status", "purpose", "client_id"];
    let after = c.after(F).await?;
    let rows: Vec<idb::Row> =
        c.p.db()
            .query(idb::list(
                Some(&t.id),
                None,
                None,
                status_filter(c)?,
                c.query("purpose"),
                c.query("client_id"),
                after,
                limit,
            ))
            .await?;
    render_page(c, rows, limit, F).await
}

pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let key = c.key().clone();
    let limit = c.limit()?;
    const F: &[&str] = &["tenant_id", "status", "purpose"];
    let after = c.after(F).await?;
    let tenant = match c.query("tenant_id") {
        Some(tid) => Some(tenant_in_scope(c, tid, false).await?.id),
        None => None,
    };
    let (tenant, partner, only) = match key.level {
        Level::Platform => (tenant, None, None),
        Level::Partner => (tenant, key.partner_id.clone(), None),
        Level::Tenant => (key.tenant_id.clone(), None, None),
        Level::Identity => (key.tenant_id.clone(), None, key.identity_id.clone()),
    };
    let rows: Vec<idb::Row> =
        c.p.db()
            .query(idb::list(
                tenant.as_deref(),
                partner.as_deref(),
                only.as_deref(),
                status_filter(c)?,
                c.query("purpose"),
                None,
                after,
                limit,
            ))
            .await?;
    render_page(c, rows, limit, F).await
}

/// Resolves an `active` or `retiring` address; anything else is `404` as an unknown address.
pub async fn lookup<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let Some(a) = c.query("address") else {
        return Err(ApiError::invalid("address", "address is required."));
    };
    let a = a.trim().to_ascii_lowercase();
    if !valid_email(&a) {
        return Err(ApiError::invalid("address", "address is not an address."));
    }
    let row: Option<adb::Row> = c.p.db().first(adb::by_address(&a)).await?;
    let Some(row) = row.filter(|r| matches!(r.status.as_str(), "active" | "retiring")) else {
        return Err(ApiError::not_found(ErrorCode::IdentityNotFound));
    };
    let (idn, _) = identity_in_scope(c, &row.identity_id, false).await?;
    Ok(Reply::json(200, render(c, &idn).await?))
}

pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (row, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    Ok(Reply::json(200, render(c, &row).await?))
}

pub async fn update<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let key = c.key().clone();
    let (row, t) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let body = c.json()?;
    let f = validate(c, &t, &body, UPDATE_FIELDS, row.message_hours)?;
    if row.is_system == 1 && f.message_hours.is_some() {
        return Err(
            ApiError::invalid("retention.message_hours", "Not for the system identity.").details(
                json!({ "field": "retention.message_hours", "reason": "system_identity" }),
            ),
        );
    }

    // Status: pause and resume (Identities and domains › Lifecycle).
    let (status, reason, status_event) = match body.get("status").map(|s| s.as_str()) {
        None => (row.status.clone(), row.pause_reason.clone(), None),
        Some(Some(s)) if s == row.status => (row.status.clone(), row.pause_reason.clone(), None),
        Some(Some("paused")) => (
            "paused".to_owned(),
            Some("manual".to_owned()),
            Some("identity.paused"),
        ),
        Some(Some("active")) => {
            match row.pause_reason.as_deref() {
                Some("tenant_suspended") => {
                    return Err(ApiError::new(
                        ErrorCode::IdentityPaused,
                        "The identity is paused because its tenant is suspended.",
                    ));
                }
                Some("abuse_threshold") => {
                    let ok = match key.level {
                        Level::Platform => true,
                        Level::Identity => false,
                        // J17: on a tenant a partner created, only a platform key.
                        _ => t.partner_id.is_none(),
                    };
                    if !ok {
                        return Err(ApiError::scope_denied(Some("status")));
                    }
                }
                _ => {}
            }
            ("active".to_owned(), None, Some("identity.resumed"))
        }
        _ => return Err(ApiError::invalid("status", "status is active or paused.")),
    };

    // The new row and the names of the fields that changed.
    let mut new = row.clone();
    let mut changed: Vec<String> = Vec::new();
    let mut set = |name: &str, differs: bool| {
        if differs && !changed.iter().any(|c| c == name) {
            changed.push(name.to_owned());
        }
    };
    if let Some(d) = f.display_name {
        set("display_name", d != row.display_name);
        new.display_name = d;
    }
    if let Some(p) = f.purpose {
        set("purpose", p != row.purpose);
        new.purpose = p;
    }
    if let Some((n, e)) = f.owner {
        set(
            "owner",
            Some(&n) != row.owner_name.as_ref() || Some(&e) != row.owner_email.as_ref(),
        );
        new.owner_name = Some(n);
        new.owner_email = Some(e);
    }
    if let Some((tx, html)) = f.signature {
        set(
            "signature",
            tx != row.signature_text || html != row.signature_html,
        );
        new.signature_text = tx;
        new.signature_html = html;
    }
    if let Some(m) = f.metadata {
        set("metadata", m != row.metadata());
        new.metadata_json = m.to_string();
    }
    if let Some(sp) = f.send_policy {
        let mut merged = row.send_policy();
        pylota_identity_core::policy::merge(&mut merged, &sp);
        set("send_policy", merged != row.send_policy());
        new.send_policy_json = merged.to_string();
    }
    if let Some(h) = f.message_hours {
        set("retention", h != row.message_hours);
        new.message_hours = h;
    }
    new.status = status.clone();
    new.pause_reason = reason.clone();
    if changed.is_empty() && status_event.is_none() {
        return Ok(Reply::json(200, render(c, &row).await?));
    }
    let now = c.now;
    new.updated_at = now;
    let ids = c.p.ids();
    let mut batch = vec![Stmt::new(
        "UPDATE identities SET display_name = ?2, purpose = ?3, owner_name = ?4, owner_email = ?5, signature_text = ?6, signature_html = ?7, metadata_json = ?8, send_policy_json = ?9, message_hours = ?10, status = ?11, pause_reason = ?12, updated_at = ?13 WHERE id = ?1 AND status IN ('active','paused')",
        vec![
            new.id.clone().into(),
            new.display_name.clone().into(),
            new.purpose.clone().into(),
            new.owner_name.clone().into(),
            new.owner_email.clone().into(),
            new.signature_text.clone().into(),
            new.signature_html.clone().into(),
            new.metadata_json.clone().into(),
            new.send_policy_json.clone().into(),
            new.message_hours.into(),
            status.clone().into(),
            reason.clone().into(),
            now.into(),
        ],
    )];
    let mut audit_fields = changed.clone();
    if status_event.is_some() {
        audit_fields.push("status".into());
    }
    batch.push(audit::insert(
        &ids.new_id(IdPrefix::Audit),
        Some(&t.id),
        Some(&key.key_id),
        "identity.update",
        "identity",
        &row.id,
        &json!({ "changed": audit_fields, "status": status, "pause_reason": reason }),
        &c.request_id,
        now,
    ));
    if changed.iter().any(|c| c == "retention") {
        batch.push(audit::insert(
            &ids.new_id(IdPrefix::Audit),
            Some(&t.id),
            Some(&key.key_id),
            "identity.retention_update",
            "identity",
            &row.id,
            &json!({ "message_hours": new.message_hours, "previous": row.message_hours }),
            &c.request_id,
            now,
        ));
    }
    let has_mailbox = !row.mailbox_do_id.is_empty();
    if has_mailbox && !changed.is_empty() {
        let addrs = addresses_of(c, &row.id).await?;
        batch.push(event_intent(
            &c.request_id,
            &row,
            "identity.updated",
            &ev::identity_updated(&new.json(&addrs), &changed),
            now,
        ));
    }
    if let (true, Some(e)) = (has_mailbox, status_event) {
        let data = if e == "identity.paused" {
            ev::identity_paused(&row.id, "manual", None)
        } else {
            ev::identity_resumed(&row.id)
        };
        batch.push(event_intent(&c.request_id, &row, e, &data, now));
    }
    let metas = c.p.db().batch(batch).await?;
    if metas.first().is_some_and(|m| m.changes == 0) {
        return Err(ApiError::not_found(ErrorCode::IdentityNotFound));
    }
    if has_mailbox {
        crate::intents::flush_object(c.p.as_ref(), &c.request_id, &row.mailbox_do_id).await;
    }
    Ok(Reply::json(200, render(c, &new).await?))
}

/// `DELETE /v1/identities/{identity_id}` (Identities and domains › Delete, Privacy § 6.1).
pub async fn delete<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let key = c.key().clone();
    let (row, t) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    if row.is_system == 1 {
        return Err(ApiError::new(
            ErrorCode::SystemProtected,
            "The system identity cannot be deleted.",
        ));
    }
    if key.level != Level::Platform && matches!(t.status.as_str(), "erasing" | "erased") {
        return Err(ApiError::not_found(ErrorCode::IdentityNotFound));
    }
    let now = c.now;
    let ids = c.p.ids();
    let latest: Option<jobs::Erasure> =
        c.p.db()
            .first(jobs::latest_identity_erasure(&t.id, &row.id))
            .await?;
    // A second DELETE starts nothing, unless the latest request failed: then it resumes it.
    let resumes = match (row.live(), &latest) {
        (false, Some(l)) if l.status != "failed" => return Ok(Reply::json(200, l.json())),
        (false, Some(l)) => Some(l.clone()),
        (false, None) => return Err(ApiError::not_found(ErrorCode::IdentityNotFound)),
        (true, _) => None,
    };
    let addrs = addresses_of(c, &row.id).await?;
    let era = ids.new_id(IdPrefix::Erasure);
    let job = ids.new_id(IdPrefix::Job);
    let runner = c.p.objects().new_object_id(DoClass::Job)?;
    let rules: Vec<Value> = addrs
        .iter()
        .filter(|a| a.routing_rule_id.is_some() || a.ses_bounce_rule.is_some())
        .map(|a| json!({ "domain_id": a.domain_id, "routing_rule_id": a.routing_rule_id, "ses_bounce_rule": a.ses_bounce_rule }))
        .collect();
    let params = json!({ "scope": "identity", "identity_id": row.id, "erasure_request_id": era, "rules": rules });
    let deadline = resumes
        .as_ref()
        .map_or(now + jobs::ERASURE_DEADLINE_MS, |l| l.deadline_at);
    let mut batch: Vec<Stmt> = Vec::new();
    if resumes.is_none() {
        batch.push(Stmt::new(
            "UPDATE identities SET status = 'deleting', updated_at = ?2 WHERE id = ?1 AND status IN ('active','paused')",
            vec![row.id.clone().into(), now.into()],
        ));
        let hash_key = &c.p.secrets().hash_key;
        for a in &addrs {
            batch.push(adb::insert_tombstone(
                &address::address_hash(hash_key, &a.address),
                &row.id,
                "deleted",
            ));
        }
        batch.push(adb::delete_of_identity(&row.id));
    }
    batch.push(jobs::insert_job(
        &job,
        &t.id,
        "erasure",
        &runner,
        &params,
        &key.key_id,
        now,
    ));
    let mut era_stmt = jobs::insert_erasure(
        &era,
        &t.id,
        &job,
        "identity",
        Some(&row.id),
        "identity_deleted",
        &key.key_id,
        deadline,
        now,
    );
    if let Some(l) = &resumes {
        era_stmt = jobs::resuming(era_stmt, &l.id);
    }
    batch.push(era_stmt);
    batch.push(audit::insert(
        &ids.new_id(IdPrefix::Audit),
        Some(&t.id),
        Some(&key.key_id),
        "identity.delete",
        "identity",
        &row.id,
        &json!({ "erasure_request_id": era, "resumes_id": resumes.as_ref().map(|l| l.id.clone()) }),
        &c.request_id,
        now,
    ));
    let metas = c.p.db().batch(batch).await?;
    if resumes.is_none() && metas.first().is_some_and(|m| m.changes == 0) {
        // A concurrent DELETE won; its request is the answer.
        let l: Option<jobs::Erasure> =
            c.p.db()
                .first(jobs::latest_identity_erasure(&t.id, &row.id))
                .await?;
        return l.map_or_else(
            || Err(ApiError::not_found(ErrorCode::IdentityNotFound)),
            |l| Ok(Reply::json(200, l.json())),
        );
    }
    // After commit: the runner (a stub until M14) and the inboxes allowance.
    let start = crate::jobs::JobRequest::Start {
        job_id: job.clone(),
        kind: "erasure".into(),
        tenant_id: Some(t.id.clone()),
        params,
        resume_job_id: resumes.as_ref().map(|l| l.job_id.clone()),
    };
    let env = envelope_for(
        c.p.as_ref(),
        Some(&t.id),
        None,
        &c.request_id,
        Some(&key.key_id),
        start,
    );
    let _ =
        c.p.objects()
            .call::<_, Value>(DoClass::Job, &runner, &env)
            .await;
    if resumes.is_none() {
        let _ = quota(
            c,
            &t,
            QuotaRequest::Adjust {
                feature: Feature::Inboxes,
                delta: -1,
                r#ref: row.id.clone(),
            },
        )
        .await;
    }
    let e: Option<jobs::Erasure> = c.p.db().first(jobs::get_erasure(&era)).await?;
    let e = e.ok_or_else(ApiError::internal)?;
    Ok(Reply::json(202, e.json()))
}
