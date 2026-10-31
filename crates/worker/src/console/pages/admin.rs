//! API keys, domains, webhooks and the audit log in the console: the same routes as the API, with the
//! session's principal. A key minted here records the person and their role (W36), and can never hold
//! a permission the session lacks (FR-KEY-1).

use maud::html;
use pylota_identity_platform::Platform;
use serde_json::{Value, json};

use super::{done, flash, next_link, table};
use crate::console::Cx;
use crate::console::layout::{self, button, field, post_form, select};
use crate::ctx::Reply;
use crate::errors::ApiError;

/// Permissions an identity-level key never carries (Security § 4.6).
const TENANT_ONLY: &[&str] = &[
    "members:read",
    "members:manage",
    "suppressions:manage",
    "audit:read",
    "usage:read",
    "tenants:erase",
    "keys:manage",
    "policy:write",
    "erasure:manage",
    "webhooks:manage",
    "webhooks:read",
    "domains:write",
];

fn tenant_q<P: Platform>(cx: &Cx<'_, P>) -> Vec<(&'static str, String)> {
    let mut q = vec![
        ("tenant_id", cx.tenant().id.clone()),
        ("limit", "50".into()),
    ];
    if let Some(c) = cx.q("cursor") {
        q.push(("cursor", c.to_owned()));
    }
    q
}

pub async fn keys<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let doc = cx.api("GET", "/v1/keys", &tenant_q(cx), None).await?;
    let rows: Vec<Value> = doc["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|k| k["revoked_at"].is_null())
        .cloned()
        .collect();
    let idns = cx
        .api(
            "GET",
            &format!("/v1/tenants/{}/identities", cx.tenant().id),
            &[("limit", "100".into())],
            None,
        )
        .await?;
    let csrf = cx.csrf();
    let owner = cx.can("tenants:erase");
    let perms: Vec<String> = cx
        .principal
        .as_ref()
        .map(|k| k.permissions.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| owner || p != "tenants:erase")
        .collect();
    let mut idn_opts: Vec<(String, String)> =
        vec![(String::new(), "– (a tenant-level key)".into())];
    idn_opts.extend(
        idns["data"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|i| (layout::text(&i["id"]), layout::text(&i["primary_address"]))),
    );
    let opts: Vec<(&str, &str)> = idn_opts
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    Ok(cx.page(
        "API keys",
        html! {
            (flash(cx))
            @if rows.is_empty() { p class="muted" { "No active keys." } }
            @else {
                table {
                    thead { tr { th scope="col" { "Name" } th scope="col" { "Level" } th scope="col" { "Inbox" } th scope="col" { "Last used" } th scope="col" { "Created by" } th scope="col" { "Action" } } }
                    tbody {
                        @for k in &rows {
                            tr {
                                td { (layout::text(&k["name"])) br; code { (layout::text(&k["id"])) } }
                                td { (layout::text(&k["level"])) }
                                td { (layout::text(&k["identity_id"])) }
                                td { (layout::text(&k["last_used_at"])) }
                                td { (layout::text(&k["created_by_user_id"])) }
                                td { (button(&format!("/console/keys/{}/revoke", layout::text(&k["id"])), &csrf, "Revoke", true)) }
                            }
                        }
                    }
                }
            }
            (next_link("/console/keys", &doc))
            h2 { "New key" }
            (post_form("/console/keys", &csrf, "", html! {
                (field("name", "Name", "text", "", true))
                (select("identity_id", "Inbox (for an identity-level key)", &opts, ""))
                fieldset {
                    legend { "Permissions" }
                    @for p in &perms {
                        @let id = format!("perm-{}", p.replace(':', "-"));
                        div { input type="checkbox" id=(id) name="permissions" value=(p); " " label for=(id) style="display:inline" { (p) } }
                    }
                }
                p { button type="submit" { "Create key" } }
            }))
        },
    ))
}

pub async fn key_create<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let identity = cx.f("identity_id").filter(|s| !s.is_empty());
    let mut perms: Vec<&str> = cx.fs("permissions");
    if identity.is_some() {
        perms.retain(|p| !TENANT_ONLY.contains(p));
    }
    let mut body = json!({
        "name": cx.f("name").unwrap_or(""),
        "level": if identity.is_some() { "identity" } else { "tenant" },
        "tenant_id": cx.tenant().id,
        "permissions": perms,
    });
    if let Some(i) = identity {
        body["identity_id"] = i.into();
    }
    let k = cx.api("POST", "/v1/keys", &[], Some(body)).await?;
    Ok(cx.page(
        "Key created",
        html! {
            (layout::notice("Copy the secret now: it is shown once."))
            p { "Key " code { (layout::text(&k["id"])) } }
            p { label for="secret" { "Secret" } input type="text" id="secret" readonly value=(layout::text(&k["secret"])) size="70"; }
            p { a href="/console/keys" { "Back to the keys" } }
        },
    ))
}

pub async fn key_revoke<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    cx.api(
        "DELETE",
        &format!("/v1/keys/{}", cx.param("key_id")),
        &[],
        None,
    )
    .await?;
    Ok(done("/console/keys", "revoked"))
}

pub async fn domains<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let doc = cx
        .api(
            "GET",
            &format!("/v1/tenants/{}/domains", cx.tenant().id),
            &[("limit", "50".into())],
            None,
        )
        .await?;
    let rows = doc["data"].as_array().cloned().unwrap_or_default();
    let csrf = cx.csrf();
    let link = |r: &Value| format!("/console/domains/{}", layout::text(&r["id"]));
    Ok(cx.page(
        "Domains",
        html! {
            (flash(cx))
            (table(&rows, &[("name", "Domain"), ("state", "State"), ("method", "Connection"), ("inbound", "Inbound")], Some(&link)))
            @if cx.can("domains:write") {
                h2 { "Add a domain" }
                (post_form("/console/domains", &csrf, "", html! {
                    (field("name", "Domain name", "text", "", true))
                    (select("method", "How it connects", &[("cloudflare_zone", "A zone in this Cloudflare account"), ("dns_records", "Records at your DNS host"), ("delegated_subdomain", "A delegated subdomain"), ("send_only", "Sending only")], "cloudflare_zone"))
                    p { button type="submit" { "Add domain" } }
                }))
            }
        },
    ))
}

pub async fn domain<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let id = cx.param("domain_id");
    let d = cx
        .api("GET", &format!("/v1/domains/{id}"), &[], None)
        .await?;
    let records = cx
        .api("GET", &format!("/v1/domains/{id}/records"), &[], None)
        .await
        .unwrap_or(Value::Null);
    let recs = records["data"]
        .as_array()
        .or(records["records"].as_array())
        .cloned()
        .unwrap_or_default();
    let csrf = cx.csrf();
    Ok(cx.page(
        &layout::text(&d["name"]),
        html! {
            (flash(cx))
            dl {
                dt { "State" } dd { (layout::text(&d["state"])) }
                dt { "Connection" } dd { (layout::text(&d["method"])) }
                dt { "Inbound" } dd { (layout::text(&d["inbound"])) }
                dt { "Transport" } dd { (layout::text(&d["transport"])) }
            }
            h2 { "DNS records" }
            (table(&recs, &[("type", "Type"), ("name", "Name"), ("value", "Value"), ("status", "Status")], None))
            @if cx.can("domains:write") {
                p {
                    (button(&format!("/console/domains/{id}/verify"), &csrf, "Verify now", false))
                    " "
                    (button(&format!("/console/domains/{id}/delete"), &csrf, "Remove the domain", true))
                }
            }
        },
    ))
}

pub async fn domain_create<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let d = cx
        .api(
            "POST",
            &format!("/v1/tenants/{}/domains", cx.tenant().id),
            &[],
            Some(json!({ "name": cx.f("name").unwrap_or(""), "method": cx.f("method").unwrap_or("") })),
        )
        .await?;
    Ok(done(
        &format!("/console/domains/{}", layout::text(&d["id"])),
        "created",
    ))
}

pub async fn domain_verify<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let id = cx.param("domain_id");
    cx.api(
        "POST",
        &format!("/v1/domains/{id}/verify"),
        &[],
        Some(json!({})),
    )
    .await?;
    Ok(done(&format!("/console/domains/{id}"), "verifying"))
}

pub async fn domain_delete<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let id = cx.param("domain_id");
    cx.api("DELETE", &format!("/v1/domains/{id}"), &[], None)
        .await?;
    Ok(done("/console/domains", "deleted"))
}

pub async fn webhooks<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let doc = cx
        .api(
            "GET",
            &format!("/v1/tenants/{}/webhooks", cx.tenant().id),
            &[("limit", "50".into())],
            None,
        )
        .await?;
    let rows = doc["data"].as_array().cloned().unwrap_or_default();
    let csrf = cx.csrf();
    Ok(cx.page(
        "Webhooks",
        html! {
            (flash(cx))
            @if rows.is_empty() { p class="muted" { "No endpoints." } }
            @else {
                table {
                    thead { tr { th scope="col" { "URL" } th scope="col" { "Events" } th scope="col" { "Enabled" } th scope="col" { "Action" } } }
                    tbody {
                        @for w in &rows {
                            tr {
                                td { code { (layout::text(&w["url"])) } }
                                td { (layout::text(&w["events"])) }
                                td { (layout::text(&w["enabled"])) @if let Some(r) = w["disabled_reason"].as_str() { " (" (r) ")" } }
                                td { (button(&format!("/console/webhooks/{}/delete", layout::text(&w["id"])), &csrf, "Delete", true)) }
                            }
                        }
                    }
                }
            }
            h2 { "New endpoint" }
            (post_form("/console/webhooks", &csrf, "", html! {
                (field("url", "HTTPS URL", "text", "", true))
                (field("events", "Event types, comma-separated (for example message.received)", "text", "message.received", true))
                p { button type="submit" { "Create endpoint" } }
            }))
        },
    ))
}

pub async fn webhook_create<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let events: Vec<String> = cx
        .f("events")
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    let w = cx
        .api(
            "POST",
            &format!("/v1/tenants/{}/webhooks", cx.tenant().id),
            &[],
            Some(json!({ "url": cx.f("url").unwrap_or(""), "events": events })),
        )
        .await?;
    Ok(cx.page(
        "Endpoint created",
        html! {
            (layout::notice("Copy the signing secret now: it is shown once."))
            p { label for="secret" { "Signing secret" } input type="text" id="secret" readonly value=(layout::text(&w["secret"])) size="60"; }
            p { a href="/console/webhooks" { "Back to the webhooks" } }
        },
    ))
}

pub async fn webhook_delete<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    cx.api(
        "DELETE",
        &format!("/v1/webhooks/{}", cx.param("webhook_id")),
        &[],
        None,
    )
    .await?;
    Ok(done("/console/webhooks", "deleted"))
}

pub async fn audit<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let doc = cx
        .api("GET", "/v1/audit-events", &tenant_q(cx), None)
        .await?;
    let rows = doc["data"].as_array().cloned().unwrap_or_default();
    Ok(cx.page(
        "Audit log",
        html! {
            (table(&rows, &[("created_at", "When"), ("action", "Action"), ("target_type", "Target"), ("target_id", "ID"), ("actor_user_id", "Person"), ("actor_key_id", "Key")], None))
            (next_link("/console/audit", &doc))
        },
    ))
}
