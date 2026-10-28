//! `POST`, `GET` and `PATCH /v1/tenants…` (REST API › Tenants, FR-TEN-1/2/3).

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::{address, days, policy as cpolicy};
use pylota_identity_platform::config::BillingMode;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::ratelimit::RlBucket;
use pylota_identity_platform::{ControlDb as _, Ids as _, ObjectClient as _, Platform};
use serde_json::{Value, json};

use super::Count;
use crate::auth::Level;
use crate::ctx::{Ctx, Reply};
use crate::db::{audit, intents, partners as pdb, tenants as tdb};
use crate::errors::ApiError;
use crate::quota::QuotaRequest;
use crate::rpc::{RpcResult, envelope_for};
use crate::{policy, ratelimit};

fn str_field<'a>(body: &'a Value, name: &str) -> Result<Option<&'a str>, ApiError> {
    match body.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(ApiError::invalid(name, format!("{name} must be a string."))),
    }
}

/// A plausible RFC 5321 address, lower-cased.
fn email(s: &str) -> Option<String> {
    let s = s.trim().to_ascii_lowercase();
    let (local, domain) = s.rsplit_once('@')?;
    let ok = !local.is_empty()
        && local.len() <= 64
        && s.len() <= 254
        && domain.contains('.')
        && s.is_ascii()
        && !s.contains(char::is_whitespace)
        && domain.split('.').all(|l| !l.is_empty());
    ok.then_some(s)
}

/// Sends `QuotaRequest::Init` for a tenant whose intent row is in D1; deletes the row on success. A
/// failure leaves it for the every-minute cron (`intents::resend`).
pub async fn send_quota_init<P: Platform>(
    c: &Ctx<P>,
    tenant_id: &str,
    quota_do_id: &str,
    intent_id: &str,
) {
    let env = envelope_for(
        c.p.as_ref(),
        Some(tenant_id),
        None,
        &c.request_id,
        Some(&c.key().key_id),
        QuotaRequest::Init {
            tenant_id: tenant_id.to_owned(),
        },
    );
    match c
        .p
        .objects()
        .call::<_, Value>(DoClass::Quota, quota_do_id, &env)
        .await
    {
        Ok(RpcResult::Ok(_)) => {
            if c.p.db().execute(intents::done(intent_id)).await.is_err() {
                crate::log::warn("intent_delete_failed", &[("intent", intent_id.into())]);
            }
        }
        _ => crate::log::warn("quota_init_deferred", &[("tenant", tenant_id.into())]),
    }
}

pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let key = c.key().clone();
    let body = c.json()?;
    if !body.is_object() {
        return Err(ApiError::invalid("", "The body must be a JSON object."));
    }
    let slug =
        str_field(&body, "slug")?.ok_or_else(|| ApiError::invalid("slug", "slug is required."))?;
    if !address::valid_slug(slug) {
        return Err(ApiError::invalid(
            "slug",
            "slug is 2–32 of a-z, 0-9 and -, starting with a letter or digit.",
        ));
    }
    let name =
        str_field(&body, "name")?.ok_or_else(|| ApiError::invalid("name", "name is required."))?;
    if name.trim().is_empty() || name.chars().count() > 200 {
        return Err(ApiError::invalid("name", "name is 1–200 characters."));
    }
    let mode =
        str_field(&body, "mode")?.ok_or_else(|| ApiError::invalid("mode", "mode is required."))?;
    if !matches!(mode, "live" | "test") {
        return Err(ApiError::invalid("mode", "mode is live or test."));
    }
    let timezone = str_field(&body, "timezone")?.unwrap_or("UTC");
    if !days::valid_timezone(timezone) {
        return Err(ApiError::invalid("timezone", "Unknown IANA time zone."));
    }
    let suffix = match str_field(&body, "address_suffix")? {
        Some(s) => s.to_owned(),
        None => format!(".{slug}"),
    };
    let suffix_fold = match address::suffix_fold(&suffix) {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => {
            // Only the default tenant, which `idvy setup` creates first with a platform key, has an
            // empty suffix.
            let empty: Option<Count> =
                c.p.db()
                    .first(Stmt::new(
                        "SELECT COUNT(*) AS n FROM tenants WHERE suffix_fold = ''",
                        vec![],
                    ))
                    .await?;
            if key.level != Level::Platform || empty.is_some_and(|n| n.n > 0) {
                return Err(ApiError::invalid(
                    "address_suffix",
                    "Only the default tenant may have an empty suffix.",
                ));
            }
            String::new()
        }
        Err(true) => {
            return Err(ApiError::invalid(
                "address_suffix",
                "The suffix is `.` followed by 2–32 of a-z, 0-9 and -.",
            ));
        }
        Err(false) => {
            return Err(
                ApiError::new(ErrorCode::AddressReserved, "This suffix is reserved.")
                    .details(json!({ "field": "address_suffix" })),
            );
        }
    };

    // Billing: platform-only.
    let partner = match key.level {
        Level::Partner => {
            let pid = key.partner_id.as_deref().ok_or_else(ApiError::internal)?;
            Some(
                c.p.db()
                    .first::<pdb::Row>(pdb::get(pid))
                    .await?
                    .ok_or_else(ApiError::internal)?,
            )
        }
        _ => None,
    };
    if partner.is_some() && body.get("billing").is_some_and(|b| !b.is_null()) {
        return Err(ApiError::scope_denied(Some("billing")));
    }
    let billing_mode = match &partner {
        Some(p) => p.default_billing_mode.clone(),
        None => match body.pointer("/billing/mode") {
            None | Some(Value::Null) => match c.p.config().billing.mode {
                BillingMode::Stripe => "metered".to_owned(),
                BillingMode::Off => "disabled".to_owned(),
            },
            Some(Value::String(m)) if matches!(m.as_str(), "metered" | "exempt" | "disabled") => {
                m.clone()
            }
            Some(_) => {
                return Err(ApiError::invalid(
                    "billing.mode",
                    "billing.mode is metered, exempt or disabled.",
                ));
            }
        },
    };

    // Owner: validated and stored; the sign-in email comes with M21.
    let owner = match body.get("owner") {
        None | Some(Value::Null) => None,
        Some(o) => {
            let e = o
                .get("email")
                .and_then(Value::as_str)
                .and_then(email)
                .ok_or_else(|| {
                    ApiError::invalid("owner.email", "owner.email is not a valid address.")
                })?;
            let domain = e.rsplit_once('@').map_or("", |(_, d)| d);
            let hosted: Option<Count> =
                c.p.db()
                    .first(Stmt::new(
                        "SELECT COUNT(*) AS n FROM addresses WHERE address = ?1",
                        vec![e.clone().into()],
                    ))
                    .await?;
            if domain == c.p.config().platform_domain || hosted.is_some_and(|h| h.n > 0) {
                return Err(ApiError::invalid(
                    "owner.email",
                    "An address this deployment hosts cannot own a workspace.",
                ));
            }
            let owner_name = o.get("name").and_then(Value::as_str).map(str::to_owned);
            Some((e, owner_name))
        }
    };

    // Policy: checked field by field for the creating key (platform or partner).
    let patch = body
        .get("policy")
        .cloned()
        .filter(|p| !p.is_null())
        .unwrap_or_else(|| json!({}));
    let dep = policy::deployment_defaults(c.p.as_ref());
    let writer = if key.level == Level::Platform {
        cpolicy::Writer::Platform
    } else {
        cpolicy::Writer::Partner
    };
    if patch.as_object().is_some_and(|o| !o.is_empty()) {
        policy::validate(&patch, &dep, &json!({}))?;
        let none = json!({});
        cpolicy::check_write(
            writer,
            &patch,
            &dep,
            &cpolicy::Ceilings {
                deployment: &dep,
                platform: &none,
                partner: &none,
            },
            true,
        )
        .map_err(|r| policy::refusal(r, writer))?;
    } else if !patch.is_object() {
        return Err(ApiError::invalid("policy", "policy must be an object."));
    }

    // Partner limits.
    if let Some(p) = &partner {
        let n: Count =
            c.p.db()
                .first(tdb::count_live_of_partner(&p.id))
                .await?
                .unwrap_or(Count { n: 0 });
        if n.n >= p.max_tenants {
            return Err(ApiError::new(
                ErrorCode::PartnerTenantLimit,
                "The partner has its maximum number of tenants.",
            )
            .details(json!({ "max_tenants": p.max_tenants })));
        }
        ratelimit::check(c.p.as_ref(), RlBucket::Partner, &p.id).await?;
    }

    // Uniqueness, checked before the write so the answer names the field.
    #[derive(serde::Deserialize)]
    struct Taken {
        slug: String,
        suffix_fold: String,
    }
    let taken: Vec<Taken> =
        c.p.db()
            .query(Stmt::new(
                "SELECT slug, suffix_fold FROM tenants WHERE slug = ?1 OR suffix_fold = ?2",
                vec![slug.into(), suffix_fold.clone().into()],
            ))
            .await?;
    if taken.iter().any(|t| t.slug == slug) {
        return Err(ApiError::new(
            ErrorCode::SlugTaken,
            "Another tenant uses this slug.",
        ));
    }
    if !suffix_fold.is_empty() && taken.iter().any(|t| t.suffix_fold == suffix_fold) {
        return Err(ApiError::new(
            ErrorCode::SuffixTaken,
            "This suffix is confusable with another tenant's.",
        )
        .details(json!({ "field": "address_suffix" })));
    }

    let ids = c.p.ids();
    let tenant_id = ids.new_id(IdPrefix::Tenant);
    let quota_do_id = c.p.objects().new_object_id(DoClass::Quota)?;
    let (policy_json, pc, ptc) =
        cpolicy::apply_write(writer, &json!({}), &patch, &json!({}), &json!({}), &dep);
    let partner_id = partner.as_ref().map(|p| p.id.clone());
    let intent_id = format!("{}:{tenant_id}:init", c.request_id);
    let now = c.now;
    let mut batch = vec![
        Stmt::new(
            // `max_tenants` holds under concurrent creations: the row is written only while the partner is
            // under it, and every other row of the batch only when the tenant row exists (J18).
            "INSERT INTO tenants (id, slug, name, partner_id, mode, status, address_suffix, suffix_fold, timezone, policy_json, policy_ceilings_json, partner_ceilings_json, policy_version, quota_do_id, notify_do_id, events_do_id, created_at, updated_at) SELECT ?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, '', '', ?13, ?13 WHERE ?4 IS NULL OR (SELECT COUNT(*) FROM tenants WHERE partner_id = ?4 AND status <> 'erased') < ?14",
            vec![
                tenant_id.clone().into(),
                slug.into(),
                name.into(),
                partner_id.clone().into(),
                mode.into(),
                suffix.clone().into(),
                suffix_fold.into(),
                timezone.into(),
                policy_json.to_string().into(),
                pc.to_string().into(),
                ptc.to_string().into(),
                quota_do_id.clone().into(),
                now.into(),
                partner.as_ref().map_or(0, |p| p.max_tenants).into(),
            ],
        ),
        Stmt::new(
            "INSERT INTO billing_accounts (tenant_id, mode, plan_id, status, period_start, period_end, updated_at) SELECT ?1, ?2, 'free', 'active', ?3, ?4, ?3 WHERE EXISTS (SELECT 1 FROM tenants WHERE id = ?1)",
            vec![
                tenant_id.clone().into(),
                billing_mode.clone().into(),
                now.into(),
                (now + 30 * 86_400_000).into(),
            ],
        ),
        intents::when_tenant(intents::insert(
            &intent_id,
            "quota",
            &quota_do_id,
            &tenant_id,
            None,
            "init",
            &json!({ "op": "init", "tenant_id": tenant_id }).to_string(),
            now,
        )),
        audit::when_tenant(audit::insert(
            &ids.new_id(IdPrefix::Audit),
            Some(&tenant_id),
            Some(&key.key_id),
            "tenant.create",
            "tenant",
            &tenant_id,
            &json!({ "partner_id": partner_id, "slug": slug }),
            &c.request_id,
            now,
        )),
    ];
    if let Some((e, owner_name)) = &owner {
        batch.push(Stmt::new(
            "INSERT INTO users (id, email, name, status, created_at) SELECT ?1, ?2, ?3, 'active', ?4 WHERE EXISTS (SELECT 1 FROM tenants WHERE id = ?5) ON CONFLICT (email) DO NOTHING",
            vec![ids.new_id(IdPrefix::ConsoleUser).into(), e.clone().into(), owner_name.clone().into(), now.into(), tenant_id.clone().into()],
        ));
        batch.push(Stmt::new(
            "INSERT INTO members (tenant_id, user_id, role, created_at) SELECT ?1, id, 'owner', ?3 FROM users WHERE email = ?2 AND EXISTS (SELECT 1 FROM tenants WHERE id = ?1)",
            vec![tenant_id.clone().into(), e.clone().into(), now.into()],
        ));
    }
    let metas = c.p.db().batch(batch).await.map_err(|e| {
        if e.kind == pylota_identity_platform::PlatformErrorKind::Conflict {
            ApiError::new(
                ErrorCode::SlugTaken,
                "Another tenant uses this slug or suffix.",
            )
        } else {
            e.into()
        }
    })?;
    if metas.first().is_some_and(|m| m.changes == 0) {
        let max = partner.as_ref().map_or(0, |p| p.max_tenants);
        return Err(ApiError::new(
            ErrorCode::PartnerTenantLimit,
            "The partner has its maximum number of tenants.",
        )
        .details(json!({ "max_tenants": max })));
    }
    send_quota_init(c, &tenant_id, &quota_do_id, &intent_id).await;
    let t: tdb::Row =
        c.p.db()
            .first(tdb::get(&tenant_id))
            .await?
            .ok_or_else(ApiError::internal)?;
    Ok(Reply::json(
        201,
        t.json(&policy::effective(c.p.as_ref(), &t)),
    ))
}

pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let key = c.key();
    let limit = c.limit()?;
    let filters = ["status", "mode", "partner_id"];
    let partner_filter = c.query("partner_id");
    if let Some(pid) = partner_filter {
        let visible = match key.level {
            Level::Platform => c.p.db().first::<pdb::Row>(pdb::get(pid)).await?.is_some(),
            _ => key.partner_id.as_deref() == Some(pid),
        };
        if !visible {
            return Err(ApiError::not_found(ErrorCode::PartnerNotFound));
        }
    }
    let scope = if key.level == Level::Partner {
        key.partner_id.as_deref()
    } else {
        None
    };
    let after = c.after(&filters).await?;
    let rows: Vec<tdb::Row> =
        c.p.db()
            .query(tdb::list(
                scope,
                c.query("status"),
                c.query("mode"),
                partner_filter,
                after,
                limit,
            ))
            .await?;
    let p = c.p.clone();
    c.page(
        rows,
        limit,
        &filters,
        |r| (r.created_at, r.id.clone()),
        move |r| r.json(&policy::effective(p.as_ref(), r)),
    )
    .await
}

pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let t = super::tenant_in_scope(c, c.route.param("tenant_id"), false).await?;
    Ok(Reply::json(
        200,
        t.json(&policy::effective(c.p.as_ref(), &t)),
    ))
}

pub async fn update<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let key = c.key().clone();
    let t = super::tenant_in_scope(c, c.route.param("tenant_id"), true).await?;
    if matches!(t.status.as_str(), "erasing" | "erased") {
        return Err(ApiError::new(
            ErrorCode::TenantErased,
            "The tenant is being erased or was erased.",
        ));
    }
    let body = c.json()?;
    let Some(obj) = body.as_object() else {
        return Err(ApiError::invalid("", "The body must be a JSON object."));
    };
    for k in obj.keys() {
        if !matches!(
            k.as_str(),
            "name" | "timezone" | "policy" | "status" | "sending_paused"
        ) {
            return Err(ApiError::invalid(k, format!("{k} cannot be changed.")));
        }
    }
    let name = str_field(&body, "name")?;
    if name.is_some_and(|n| n.trim().is_empty() || n.chars().count() > 200) {
        return Err(ApiError::invalid("name", "name is 1–200 characters."));
    }
    let timezone = str_field(&body, "timezone")?;
    if timezone.is_some_and(|z| !days::valid_timezone(z)) {
        return Err(ApiError::invalid("timezone", "Unknown IANA time zone."));
    }
    let status = str_field(&body, "status")?;
    // A17: the default tenant holds the system identity; no key suspends it.
    if status == Some("suspended") && t.address_suffix.is_empty() {
        return Err(ApiError::new(
            ErrorCode::SystemProtected,
            "The default tenant cannot be suspended.",
        ));
    }
    let by = if key.level == Level::Platform {
        "platform"
    } else {
        "partner"
    };
    let (status_new, suspended_by, suspended_at) = match status {
        None => (t.status.clone(), t.suspended_by.clone(), t.suspended_at),
        Some("suspended") if t.status == "suspended" => {
            // No change, except that a platform suspension replaces a partner's.
            let by_now = if by == "platform" {
                Some("platform".to_owned())
            } else {
                t.suspended_by.clone()
            };
            (t.status.clone(), by_now, t.suspended_at)
        }
        Some("suspended") => ("suspended".to_owned(), Some(by.to_owned()), Some(c.now)),
        Some("active") => {
            if t.status == "suspended"
                && t.suspended_by.as_deref() == Some("platform")
                && by == "partner"
            {
                return Err(ApiError::scope_denied(Some("status")));
            }
            ("active".to_owned(), None, None)
        }
        Some(_) => {
            return Err(ApiError::invalid(
                "status",
                "status is active or suspended.",
            ));
        }
    };
    let sending_paused_at = match body.get("sending_paused") {
        None => t.sending_paused_at,
        Some(_) if key.level != Level::Platform => {
            return Err(ApiError::scope_denied(Some("sending_paused")));
        }
        Some(Value::Bool(false)) => None,
        Some(_) => {
            return Err(ApiError::invalid(
                "sending_paused",
                "Only false is accepted: a pause is never set by hand.",
            ));
        }
    };
    let patch = body.get("policy").cloned().filter(|p| !p.is_null());
    if let Some(patch) = &patch {
        policy::check(c, &t, patch)?;
    }
    let suspending = t.status != "suspended" && status_new == "suspended";
    let resuming = t.status == "suspended" && status_new == "active";
    let mut batch = vec![Stmt::new(
        "UPDATE tenants SET name = ?2, timezone = ?3, status = ?4, suspended_by = ?5, suspended_at = ?6, sending_paused_at = ?7, sending_pause_reason = CASE WHEN ?7 IS NULL THEN NULL ELSE sending_pause_reason END, updated_at = ?8 WHERE id = ?1 AND status NOT IN ('erasing','erased')",
        vec![
            t.id.clone().into(),
            name.unwrap_or(&t.name).into(),
            timezone.unwrap_or(&t.timezone).into(),
            status_new.into(),
            suspended_by.into(),
            suspended_at.into(),
            sending_paused_at.into(),
            c.now.into(),
        ],
    )];
    if suspending {
        batch.extend(tdb::pause_identities(&t.id, &c.request_id, c.now));
    } else if resuming {
        batch.extend(tdb::resume_identities(&t.id, &c.request_id, c.now));
    }
    c.p.db().batch(batch).await?;
    if suspending || resuming {
        let objects: Vec<intents::Object> =
            c.p.db()
                .query(intents::objects_of_request(&c.request_id))
                .await?;
        for o in objects {
            crate::intents::flush_object(c.p.as_ref(), &c.request_id, &o.object_id).await;
        }
    }
    if let Some(patch) = &patch {
        policy::write(c, &t.id, patch).await?;
    }
    let t: tdb::Row =
        c.p.db()
            .first(tdb::get(&t.id))
            .await?
            .ok_or_else(ApiError::internal)?;
    Ok(Reply::json(
        200,
        t.json(&policy::effective(c.p.as_ref(), &t)),
    ))
}
