//! `/v1/keys` (REST API › API keys, Security § 4.5–4.6, J20, J22, W48).

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::days::parse_rfc3339;
use pylota_identity_core::keys as ck;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform, PlatformErrorKind, Rng as _};
use serde_json::{Value, json};

use super::Count;
use crate::auth::{KeyScope, Level, PERMISSIONS, level_can_hold};
use crate::ctx::{Ctx, Reply};
use crate::db::{audit, keys as kdb, partners as pdb, tenants as tdb};
use crate::errors::ApiError;

/// Active tenant and identity keys per tenant, and partner keys per partner (J22).
pub const TENANT_KEY_LIMIT: i64 = 100;
pub const PARTNER_KEY_LIMIT: i64 = 10;

fn scope_exceeded() -> ApiError {
    ApiError::new(
        ErrorCode::KeyScopeExceeded,
        "A key can only create or manage keys no wider than itself.",
    )
}

/// Whether `caller` may grant `permission` to a key it mints or manages: one it holds, or
/// `identities:sign` for a platform or partner caller.
fn can_grant(caller: &KeyScope, permission: &str) -> bool {
    caller.has(permission) || (permission == "identities:sign" && caller.level >= Level::Partner)
}

/// Reach (Security § 4.6, Managing existing keys, rule 1).
fn reaches(caller: &KeyScope, k: &kdb::Row) -> bool {
    if caller.key_id == k.id {
        return true;
    }
    match caller.level {
        Level::Platform => true,
        Level::Partner => {
            matches!(k.level.as_str(), "tenant" | "identity")
                && caller.partner_id.is_some()
                && k.tenant_partner_id == caller.partner_id
        }
        Level::Tenant => k.tenant_id.is_some() && k.tenant_id == caller.tenant_id,
        Level::Identity => {
            k.level == "identity" && k.identity_id.is_some() && k.identity_id == caller.identity_id
        }
    }
}

/// Not wider (rule 2).
fn not_wider(caller: &KeyScope, k: &kdb::Row) -> bool {
    if caller.key_id == k.id {
        return true;
    }
    Level::parse(&k.level).is_some_and(|l| l <= caller.level)
        && k.permissions().iter().all(|p| can_grant(caller, p))
}

async fn reachable<P: Platform>(c: &Ctx<P>) -> Result<kdb::Row, ApiError> {
    let caller = c.key();
    let k: Option<kdb::Row> = c.p.db().first(kdb::get(c.route.param("key_id"))).await?;
    let k = k
        .filter(|k| reaches(caller, k))
        .ok_or_else(|| ApiError::not_found(ErrorCode::KeyNotFound))?;
    if !not_wider(caller, &k) {
        return Err(scope_exceeded());
    }
    Ok(k)
}

/// Rule 3 (rotate and revoke): a provider-managed key belongs to its provider.
fn provider_managed(caller: &KeyScope, k: &kdb::Row) -> Result<(), ApiError> {
    if caller.key_id != k.id
        && matches!(k.created_by_level.as_str(), "platform" | "partner")
        && caller.level < Level::Partner
    {
        return Err(scope_exceeded().details(json!({ "reason": "provider_managed" })));
    }
    Ok(())
}

fn new_token<P: Platform>(p: &P, mode: ck::Mode, lookup: Option<&str>) -> String {
    let secret_random: [u8; 32] = p.rng().bytes();
    let token = ck::generate(mode, p.rng().bytes(), secret_random);
    match lookup {
        // Rotation keeps the lookup: `pmk_{mode}_{lookup}_` + the new secret.
        Some(l) => format!("pmk_{}_{l}_{}", mode.as_str(), &token[22..]),
        None => token,
    }
}

pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let caller = c.key().clone();
    let body = c.json()?;
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::invalid("", "The body must be a JSON object."))?;
    for k in obj.keys() {
        if !matches!(
            k.as_str(),
            "name"
                | "level"
                | "partner_id"
                | "tenant_id"
                | "identity_id"
                | "permissions"
                | "mcp_tools"
                | "expires_at"
        ) {
            return Err(ApiError::invalid(k, format!("{k} is not a key field.")));
        }
    }
    let name = body["name"]
        .as_str()
        .filter(|n| !n.trim().is_empty() && n.chars().count() <= 100)
        .ok_or_else(|| ApiError::invalid("name", "name is 1–100 characters."))?;
    let level = body["level"]
        .as_str()
        .and_then(Level::parse)
        .ok_or_else(|| {
            ApiError::invalid("level", "level is platform, partner, tenant or identity.")
        })?;
    let opt = |f: &str| -> Result<Option<String>, ApiError> {
        match &body[f] {
            Value::Null => Ok(None),
            Value::String(s) => Ok(Some(s.clone())),
            _ => Err(ApiError::invalid(f, format!("{f} must be a string."))),
        }
    };
    let (partner_id, tenant_id, identity_id) =
        (opt("partner_id")?, opt("tenant_id")?, opt("identity_id")?);
    let permissions: Vec<String> = match &body["permissions"] {
        Value::Array(a) if !a.is_empty() => a
            .iter()
            .map(|p| {
                p.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| ApiError::invalid("permissions", "permissions are strings."))
            })
            .collect::<Result<_, _>>()?,
        _ => {
            return Err(ApiError::invalid(
                "permissions",
                "permissions is a non-empty list: there is no implicit set.",
            ));
        }
    };
    let mut perms = permissions.clone();
    perms.sort_unstable();
    perms.dedup();
    let mcp_tools = match &body["mcp_tools"] {
        Value::Null => None,
        Value::Array(a)
            if a.len() <= 64
                && a.iter().all(|t| {
                    t.as_str().is_some_and(|s| {
                        !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                    })
                }) =>
        {
            Some(Value::Array(a.clone()))
        }
        _ => {
            return Err(ApiError::invalid_reason(
                "unknown_mcp_tool",
                "mcp_tools is a list of at most 64 tool names.",
            ));
        }
    };
    let expires_at = match opt("expires_at")? {
        None => None,
        Some(s) => {
            let t = parse_rfc3339(&s)
                .ok_or_else(|| ApiError::invalid("expires_at", "Not an RFC 3339 time."))?;
            if t <= c.now {
                return Err(ApiError::invalid(
                    "expires_at",
                    "expires_at is in the past.",
                ));
            }
            Some(t)
        }
    };

    // The shape of each level.
    match level {
        Level::Platform if partner_id.is_some() || tenant_id.is_some() || identity_id.is_some() => {
            return Err(ApiError::invalid(
                "level",
                "A platform key names no partner, tenant or identity.",
            ));
        }
        Level::Partner if partner_id.is_none() || tenant_id.is_some() || identity_id.is_some() => {
            return Err(ApiError::invalid(
                "partner_id",
                "A partner key needs partner_id and no tenant_id or identity_id.",
            ));
        }
        Level::Tenant | Level::Identity if partner_id.is_some() => {
            return Err(ApiError::invalid(
                "partner_id",
                "Only a partner key names a partner.",
            ));
        }
        Level::Tenant if tenant_id.is_none() || identity_id.is_some() => {
            return Err(ApiError::invalid(
                "tenant_id",
                "A tenant key needs tenant_id and no identity_id.",
            ));
        }
        Level::Identity if identity_id.is_none() => {
            return Err(ApiError::invalid(
                "identity_id",
                "An identity key needs identity_id.",
            ));
        }
        _ => {}
    }
    // Permission checks come before the scope check (400, not 403).
    for p in &perms {
        if !PERMISSIONS.contains(&p.as_str()) || !level_can_hold(level, p) {
            return Err(ApiError::invalid_reason(
                "permission_not_allowed_for_level",
                format!("A {} key cannot hold {p}.", level.as_str()),
            )
            .details(json!({ "reason": "permission_not_allowed_for_level", "permission": p })));
        }
    }
    if level == Level::Tenant
        && perms.iter().any(|p| p == "tenants:erase")
        && caller.created_by_role.as_deref() != Some("owner")
    {
        return Err(ApiError::invalid_reason(
            "permission_owner_only",
            "Only keys the workspace owner minted can hold tenants:erase.",
        ));
    }

    // Scope: the new key lies within the caller's.
    if level > caller.level || perms.iter().any(|p| !can_grant(&caller, p)) {
        return Err(scope_exceeded());
    }
    let mut tenant: Option<tdb::Row> = None;
    let mut identity_tenant = tenant_id.clone();
    if level == Level::Identity {
        #[derive(serde::Deserialize)]
        struct Idn {
            tenant_id: String,
        }
        let row: Option<Idn> =
            c.p.db()
                .first(Stmt::new(
                    "SELECT tenant_id FROM identities WHERE id = ?1",
                    vec![identity_id.clone().into()],
                ))
                .await?;
        let row = row.ok_or_else(|| ApiError::not_found(ErrorCode::IdentityNotFound))?;
        if tenant_id.as_deref().is_some_and(|t| t != row.tenant_id) {
            return Err(ApiError::invalid(
                "tenant_id",
                "The identity belongs to another tenant.",
            ));
        }
        if caller.level == Level::Identity && caller.identity_id != identity_id {
            return Err(scope_exceeded());
        }
        identity_tenant = Some(row.tenant_id);
    }
    if let Some(t) = &identity_tenant {
        let row: Option<tdb::Row> = c.p.db().first(tdb::get(t)).await?;
        let in_scope = row.as_ref().is_some_and(|r| match caller.level {
            Level::Platform => true,
            Level::Partner => caller.partner_id.is_some() && r.partner_id == caller.partner_id,
            _ => caller.tenant_id.as_deref() == Some(r.id.as_str()),
        });
        match (&row, in_scope) {
            (None, _) if caller.level == Level::Platform => {
                return Err(ApiError::not_found(ErrorCode::TenantNotFound));
            }
            (Some(r), true) if !matches!(r.status.as_str(), "erasing" | "erased") => tenant = row,
            _ => return Err(scope_exceeded()),
        }
    }
    // Only a platform key mints partner keys (J11), even of its own partner.
    if level == Level::Partner && caller.level != Level::Platform {
        return Err(scope_exceeded());
    }
    if level == Level::Partner {
        let pid = partner_id.as_deref().unwrap_or("");
        if c.p
            .db()
            .first::<pdb::Row>(pdb::get(pid))
            .await?
            .is_none_or(|p| p.status == "deleted")
        {
            return Err(ApiError::not_found(ErrorCode::PartnerNotFound));
        }
    }

    // Limits (J22).
    let over = match (&tenant, level) {
        (Some(t), _) => {
            let n: Count =
                c.p.db()
                    .first(kdb::count_active_of_tenant(&t.id, c.now))
                    .await?
                    .unwrap_or(Count { n: 0 });
            (n.n >= TENANT_KEY_LIMIT).then_some(TENANT_KEY_LIMIT)
        }
        (None, Level::Partner) => {
            let n: Count =
                c.p.db()
                    .first(kdb::count_active_of_partner(
                        partner_id.as_deref().unwrap_or(""),
                        c.now,
                    ))
                    .await?
                    .unwrap_or(Count { n: 0 });
            (n.n >= PARTNER_KEY_LIMIT).then_some(PARTNER_KEY_LIMIT)
        }
        _ => None,
    };
    if let Some(limit) = over {
        return Err(ApiError::new(
            ErrorCode::KeyLimitReached,
            "The active key limit is reached.",
        )
        .details(json!({ "limit": limit })));
    }

    let mode = match &tenant {
        Some(t) if t.mode == "test" => ck::Mode::Test,
        _ => ck::Mode::Live,
    };
    let granted_without: Vec<&str> = perms
        .iter()
        .map(String::as_str)
        .filter(|p| !caller.has(p))
        .collect();
    let mut row = kdb::Row {
        id: c.p.ids().new_id(IdPrefix::ApiKey),
        lookup: String::new(),
        name: name.to_owned(),
        level: level.as_str().to_owned(),
        partner_id,
        tenant_id: tenant.as_ref().map(|t| t.id.clone()),
        identity_id,
        mode: mode.as_str().to_owned(),
        permissions_json: serde_json::to_string(&perms).unwrap_or_default(),
        mcp_tools_json: mcp_tools.map(|m| m.to_string()),
        created_by_key_id: Some(caller.key_id.clone()),
        created_by_user_id: caller.created_by_user_id.clone(),
        created_by_role: caller.created_by_role.clone(),
        created_by_level: caller.level.as_str().to_owned(),
        expires_at,
        revoked_at: None,
        last_used_at: None,
        created_at: c.now,
        tenant_partner_id: None,
    };
    let mut token = String::new();
    for attempt in 0..3 {
        token = new_token(c.p.as_ref(), mode, None);
        row.lookup = ck::parse(&token)
            .map(|p| p.lookup.to_owned())
            .unwrap_or_default();
        let hash = ck::hash(&c.p.secrets().key_pepper, &token);
        let aud = audit::insert(
            &c.p.ids().new_id(IdPrefix::Audit),
            row.tenant_id.as_deref(),
            Some(&caller.key_id),
            "key.create",
            "key",
            &row.id,
            &json!({ "level": row.level, "granted_without_holding": granted_without }),
            &c.request_id,
            c.now,
        );
        match c.p.db().batch(vec![kdb::insert(&row, &hash), aud]).await {
            Ok(_) => break,
            // A lookup collision (unique index) is retried with a new value.
            Err(e) if e.kind == PlatformErrorKind::Conflict && attempt < 2 => continue,
            Err(e) => return Err(e.into()),
        }
    }
    let mut out = row.json();
    out["secret"] = Value::from(token);
    Ok(Reply::json(201, out))
}

pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let caller = c.key();
    let limit = c.limit()?;
    let filters = ["tenant_id"];
    let tenant_filter = c.query("tenant_id");
    if let Some(t) = tenant_filter {
        super::tenant_in_scope(c, t, false).await?;
    }
    let (reach, value) = match caller.level {
        Level::Platform => ("platform", String::new()),
        Level::Partner => ("partner", caller.partner_id.clone().unwrap_or_default()),
        Level::Tenant => ("tenant", caller.tenant_id.clone().unwrap_or_default()),
        Level::Identity => ("identity", caller.identity_id.clone().unwrap_or_default()),
    };
    let after = c.after(&filters).await?;
    // Rows that fail "not wider" are skipped; read pages until enough pass.
    let mut out: Vec<kdb::Row> = Vec::new();
    let mut keyset = after;
    loop {
        let rows: Vec<kdb::Row> =
            c.p.db()
                .query(kdb::list(reach, &value, tenant_filter, keyset.clone(), 100))
                .await?;
        let fetched = rows.len();
        for r in rows {
            keyset = Some((r.created_at, r.id.clone()));
            if reaches(caller, &r) && not_wider(caller, &r) {
                out.push(r);
            }
        }
        if out.len() > limit as usize || fetched <= 100 {
            break;
        }
    }
    c.page(
        out,
        limit,
        &filters,
        |r| (r.created_at, r.id.clone()),
        kdb::Row::json,
    )
    .await
}

pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    Ok(Reply::json(200, reachable(c).await?.json()))
}

pub async fn revoke<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let k = reachable(c).await?;
    provider_managed(c.key(), &k)?;
    let aud = audit::insert(
        &c.p.ids().new_id(IdPrefix::Audit),
        k.tenant_id.as_deref(),
        Some(&c.key().key_id),
        "key.revoke",
        "key",
        &k.id,
        &json!({}),
        &c.request_id,
        c.now,
    );
    c.p.db().batch(vec![kdb::revoke(&k.id, c.now), aud]).await?;
    Ok(Reply::no_content())
}

pub async fn rotate<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let caller = c.key().clone();
    let k = reachable(c).await?;
    provider_managed(&caller, &k)?;
    if k.revoked_at.is_some() {
        return Err(ApiError::new(
            ErrorCode::KeyRevoked,
            "A revoked key cannot be rotated.",
        ));
    }
    let body = c.json()?;
    let hours = match &body["overlap_hours"] {
        Value::Null => 24,
        v => v
            .as_i64()
            .filter(|h| (0..=168).contains(h))
            .ok_or_else(|| ApiError::invalid("overlap_hours", "overlap_hours is 0 to 168."))?,
    };
    let mode = ck::Mode::parse(&k.mode).unwrap_or(ck::Mode::Live);
    let token = new_token(c.p.as_ref(), mode, Some(&k.lookup));
    let hash = ck::hash(&c.p.secrets().key_pepper, &token);
    let prev_expires = (hours > 0).then(|| c.now + hours * 3_600_000);
    let (user, role) = if caller.level >= Level::Partner {
        (None, None)
    } else {
        (
            caller.created_by_user_id.clone(),
            caller.created_by_role.clone(),
        )
    };
    let aud = audit::insert(
        &c.p.ids().new_id(IdPrefix::Audit),
        k.tenant_id.as_deref(),
        Some(&caller.key_id),
        "key.rotate",
        "key",
        &k.id,
        &json!({ "overlap_hours": hours }),
        &c.request_id,
        c.now,
    );
    c.p.db()
        .batch(vec![
            kdb::rotate(&k.id, &hash, prev_expires, user.as_deref(), role.as_deref()),
            aud,
        ])
        .await?;
    let k: kdb::Row =
        c.p.db()
            .first(kdb::get(&k.id))
            .await?
            .ok_or_else(ApiError::internal)?;
    let mut out = k.json();
    out["secret"] = Value::from(token);
    Ok(Reply::json(200, out))
}
