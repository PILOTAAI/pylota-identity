//! `/console/inboxes` and `/console/inboxes/{idn}`: identities, one identity's threads, and its agent
//! signing keys ([Console › Identity signing keys]).
//!
//! [Console › Identity signing keys]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md#identity-signing-keys

use maud::html;
use pylota_identity_platform::Platform;
use serde_json::{Value, json};

use super::{done, flash, next_link, table};
use crate::console::layout::{self, button, field, post_form};
use crate::console::{Cx, urlencode};
use crate::ctx::Reply;
use crate::errors::ApiError;

fn cursor<P: Platform>(cx: &Cx<'_, P>) -> Vec<(&'static str, String)> {
    let mut q = vec![("limit", "25".to_owned())];
    if let Some(c) = cx.q("cursor") {
        q.push(("cursor", c.to_owned()));
    }
    q
}

pub async fn list<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let t = cx.tenant();
    let doc = cx
        .api(
            "GET",
            &format!("/v1/tenants/{}/identities", t.id),
            &cursor(cx),
            None,
        )
        .await?;
    let rows = doc["data"].as_array().cloned().unwrap_or_default();
    let csrf = cx.csrf();
    let link = |r: &Value| format!("/console/inboxes/{}", layout::text(&r["id"]));
    Ok(cx.page(
        "Inboxes",
        html! {
            (flash(cx))
            (table(&rows, &[("primary_address", "Address"), ("display_name", "Name"), ("status", "Status")], Some(&link)))
            (next_link("/console/inboxes", &doc))
            @if cx.can("identities:write") {
                h2 { "New inbox" }
                (post_form("/console/inboxes", &csrf, "", html! {
                    (field("username", "Username (the part before @)", "text", "", true))
                    (field("display_name", "Display name", "text", "", true))
                    p { button type="submit" { "Create inbox" } }
                }))
            }
        },
    ))
}

pub async fn create<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let t = cx.tenant();
    let r = cx
        .api(
            "POST",
            &format!("/v1/tenants/{}/identities", t.id),
            &[],
            Some(json!({
                "username": cx.f("username").unwrap_or(""),
                "display_name": cx.f("display_name").unwrap_or(""),
            })),
        )
        .await?;
    Ok(done(
        &format!("/console/inboxes/{}", layout::text(&r["id"])),
        "created",
    ))
}

pub async fn show<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let id = cx.param("identity_id");
    let idn = cx
        .api("GET", &format!("/v1/identities/{id}"), &[], None)
        .await?;
    let threads = if cx.can("messages:read") {
        cx.api(
            "GET",
            &format!("/v1/identities/{id}/threads"),
            &cursor(cx),
            None,
        )
        .await?
    } else {
        Value::Null
    };
    let keys = cx
        .api("GET", &format!("/v1/identities/{id}/keys"), &[], None)
        .await?;
    let key_rows = keys["data"].as_array().cloned().unwrap_or_default();
    let thread_rows = threads["data"].as_array().cloned().unwrap_or_default();
    let csrf = cx.csrf();
    let api_host = &cx.p().config().api_host;
    let jwks = format!("https://{api_host}/.well-known/jwks/{id}.json");
    let paused = idn["status"] == "paused";
    let manage = cx.can("identities:write");
    let thread_link =
        |r: &Value| format!("/console/inboxes/{id}/threads/{}", layout::text(&r["id"]));
    Ok(cx.page(
        &layout::text(&idn["display_name"]),
        html! {
            (flash(cx))
            dl {
                dt { "Address" } dd { (layout::text(&idn["primary_address"])) }
                dt { "Status" } dd { (layout::text(&idn["status"])) }
                dt { "ID" } dd { code { (id) } }
            }
            @if manage {
                (post_form(&format!("/console/inboxes/{id}/status"), &csrf, "", html! {
                    input type="hidden" name="status" value=(if paused { "active" } else { "paused" });
                    button type="submit" class="secondary" { @if paused { "Resume this inbox" } @else { "Pause this inbox" } }
                }))
            }
            @if cx.can("messages:read") {
                h2 { "Threads" }
                (table(&thread_rows, &[("subject", "Subject"), ("message_count", "Messages"), ("last_at", "Last message")], Some(&thread_link)))
                (next_link(&format!("/console/inboxes/{id}"), &threads))
            }
            h2 { "Signing keys" }
            p { "Published key set: " a href=(jwks) { (jwks) } }
            @if paused { p class="notice" { "This inbox is paused: its key set is withdrawn and it cannot sign. Keys can still be revoked." } }
            @if key_rows.is_empty() { p class="muted" { "No signing key yet. One is created on the first signing request." } }
            @else {
                table {
                    thead { tr { th scope="col" { "Key ID" } th scope="col" { "Status" } th scope="col" { "Created" } th scope="col" { "Verifies until" } th scope="col" { "Retired" } @if manage { th scope="col" { "Action" } } } }
                    tbody {
                        @for k in &key_rows {
                            tr {
                                td { code { (layout::text(&k["kid"])) } }
                                td { (layout::text(&k["status"])) }
                                td { (layout::text(&k["created_at"])) }
                                td { (layout::text(&k["verify_until"])) }
                                td { (layout::text(&k["retired_at"])) }
                                @if manage {
                                    td {
                                        @if k["status"] != "retired" {
                                            (button(&format!("/console/inboxes/{id}/keys/{}/revoke", urlencode(&layout::text(&k["kid"]))), &csrf, "Revoke", true))
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            @if manage {
                p {
                    @if key_rows.iter().all(|k| k["status"] != "active") {
                        (button(&format!("/console/inboxes/{id}/keys"), &csrf, "Create a signing key", false))
                    } @else {
                        (button(&format!("/console/inboxes/{id}/keys/rotate"), &csrf, "Rotate the signing key", false))
                    }
                }
            }
        },
    ))
}

pub async fn status<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let id = cx.param("identity_id");
    let status = match cx.f("status") {
        Some("paused") => "paused",
        _ => "active",
    };
    cx.api(
        "PATCH",
        &format!("/v1/identities/{id}"),
        &[],
        Some(json!({ "status": status })),
    )
    .await?;
    Ok(done(
        &format!("/console/inboxes/{id}"),
        if status == "paused" {
            "paused"
        } else {
            "resumed"
        },
    ))
}

pub async fn key_create<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let id = cx.param("identity_id");
    cx.api(
        "POST",
        &format!("/v1/identities/{id}/keys"),
        &[],
        Some(json!({})),
    )
    .await?;
    Ok(done(&format!("/console/inboxes/{id}"), "created"))
}

pub async fn key_rotate<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let id = cx.param("identity_id");
    cx.api(
        "POST",
        &format!("/v1/identities/{id}/keys/rotate"),
        &[],
        Some(json!({})),
    )
    .await?;
    Ok(done(&format!("/console/inboxes/{id}"), "rotated"))
}

pub async fn key_revoke<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let id = cx.param("identity_id");
    let kid = cx.param("kid");
    cx.api(
        "POST",
        &format!("/v1/identities/{id}/keys/{kid}/revoke"),
        &[],
        Some(json!({})),
    )
    .await?;
    Ok(done(&format!("/console/inboxes/{id}"), "revoked"))
}
