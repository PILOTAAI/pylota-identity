//! `/v1/partners` (REST API › Partners, FR-KEY-4): platform keys with `partners:manage` only.

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform};
use serde_json::{Value, json};

use super::Count;
use crate::ctx::{Ctx, Reply};
use crate::db::{audit, partners as pdb, tenants as tdb};
use crate::errors::ApiError;

fn apply(r: &mut pdb::Row, body: &Value, create: bool) -> Result<(), ApiError> {
    let Some(obj) = body.as_object() else {
        return Err(ApiError::invalid("", "The body must be a JSON object."));
    };
    for (k, v) in obj {
        match (k.as_str(), v) {
            ("name", Value::String(s)) if !s.trim().is_empty() && s.chars().count() <= 200 => {
                r.name = s.clone()
            }
            ("name", _) => return Err(ApiError::invalid("name", "name is 1–200 characters.")),
            ("default_billing_mode", Value::String(m))
                if matches!(m.as_str(), "exempt" | "metered") =>
            {
                r.default_billing_mode = m.clone()
            }
            ("default_billing_mode", _) => {
                return Err(ApiError::invalid(
                    "default_billing_mode",
                    "exempt or metered.",
                ));
            }
            ("max_tenants", v) if v.as_i64().is_some_and(|n| (0..=100_000).contains(&n)) => {
                r.max_tenants = v.as_i64().unwrap_or(25)
            }
            ("max_tenants", _) => {
                return Err(ApiError::invalid(
                    "max_tenants",
                    "max_tenants is 0 to 100,000.",
                ));
            }
            ("ramp_exempt", Value::Bool(b)) => r.ramp_exempt = i64::from(*b),
            ("ramp_exempt", _) => {
                return Err(ApiError::invalid(
                    "ramp_exempt",
                    "ramp_exempt is a boolean.",
                ));
            }
            ("status", Value::String(s))
                if !create && matches!(s.as_str(), "active" | "suspended") =>
            {
                r.status = s.clone()
            }
            ("status", _) if !create => {
                return Err(ApiError::invalid(
                    "status",
                    "status is active or suspended.",
                ));
            }
            (k, _) => return Err(ApiError::invalid(k, format!("{k} is not a partner field."))),
        }
    }
    Ok(())
}

pub async fn create<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let body = c.json()?;
    if body.get("name").is_none() {
        return Err(ApiError::invalid("name", "name is required."));
    }
    let mut r = pdb::Row {
        id: c.p.ids().new_id(IdPrefix::Partner),
        name: String::new(),
        status: "active".into(),
        default_billing_mode: "metered".into(),
        max_tenants: 25,
        ramp_exempt: 0,
        created_at: c.now,
        updated_at: c.now,
    };
    apply(&mut r, &body, true)?;
    let aud = audit::insert(
        &c.p.ids().new_id(IdPrefix::Audit),
        None,
        Some(&c.key().key_id),
        "partner.create",
        "partner",
        &r.id,
        &json!({}),
        &c.request_id,
        c.now,
    );
    c.p.db().batch(vec![pdb::insert(&r), aud]).await?;
    Ok(Reply::json(201, r.json()))
}

pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let limit = c.limit()?;
    let filters = ["status"];
    let after = c.after(&filters).await?;
    let rows: Vec<pdb::Row> =
        c.p.db()
            .query(pdb::list(c.query("status"), after, limit))
            .await?;
    c.page(
        rows,
        limit,
        &filters,
        |r| (r.created_at, r.id.clone()),
        pdb::Row::json,
    )
    .await
}

async fn load<P: Platform>(c: &Ctx<P>) -> Result<pdb::Row, ApiError> {
    c.p.db()
        .first(pdb::get(c.route.param("partner_id")))
        .await?
        .ok_or_else(|| ApiError::not_found(ErrorCode::PartnerNotFound))
}

pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    Ok(Reply::json(200, load(c).await?.json()))
}

pub async fn update<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let mut r = load(c).await?;
    if r.status == "deleted" {
        return Err(ApiError::not_found(ErrorCode::PartnerNotFound));
    }
    let body = c.json()?;
    apply(&mut r, &body, false)?;
    r.updated_at = c.now;
    let fields: Vec<&String> = body
        .as_object()
        .map(|o| o.keys().collect())
        .unwrap_or_default();
    let aud = audit::insert(
        &c.p.ids().new_id(IdPrefix::Audit),
        None,
        Some(&c.key().key_id),
        "partner.update",
        "partner",
        &r.id,
        &json!({ "fields": fields }),
        &c.request_id,
        c.now,
    );
    c.p.db().batch(vec![pdb::update(&r), aud]).await?;
    Ok(Reply::json(200, r.json()))
}

pub async fn delete<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let r = load(c).await?;
    if r.status == "deleted" {
        return Err(ApiError::not_found(ErrorCode::PartnerNotFound));
    }
    let n: Count =
        c.p.db()
            .first(tdb::count_live_of_partner(&r.id))
            .await?
            .unwrap_or(Count { n: 0 });
    if n.n > 0 {
        return Err(ApiError::new(
            ErrorCode::PartnerHasTenants,
            "Erase the partner's tenants first.",
        )
        .details(json!({ "tenants": n.n })));
    }
    let mut batch = pdb::soft_delete(&r.id, c.now);
    batch.push(audit::insert(
        &c.p.ids().new_id(IdPrefix::Audit),
        None,
        Some(&c.key().key_id),
        "partner.delete",
        "partner",
        &r.id,
        &json!({}),
        &c.request_id,
        c.now,
    ));
    let metas = c.p.db().batch(batch).await?;
    if metas.get(metas.len() - 2).is_some_and(|m| m.changes == 0) {
        // A tenant was created between the count and the batch: nothing changed.
        let n: Count =
            c.p.db()
                .first(tdb::count_live_of_partner(&r.id))
                .await?
                .unwrap_or(Count { n: 0 });
        return Err(ApiError::new(
            ErrorCode::PartnerHasTenants,
            "Erase the partner's tenants first.",
        )
        .details(json!({ "tenants": n.n })));
    }
    Ok(Reply::no_content())
}
