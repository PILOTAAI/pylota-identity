//! `/console/members` ([Console › Members], [Invitations]): members, pending invitations and seats;
//! invite, revoke, change role, remove, transfer ownership and leave. The services are the REST
//! routes' own (`crate::members`), with the signed-in person as the actor.
//!
//! [Console › Members]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md#members
//! [Invitations]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md#invitations

use maud::html;
use pylota_identity_core::console::Role;
use pylota_identity_platform::Platform;

use super::{done, flash};
use crate::console::layout::{self, button, field, post_form, select};
use crate::console::{Cx, see_other};
use crate::ctx::Reply;
use crate::errors::ApiError;
use crate::members::{self, Actor, invitations};

fn actor<'a, P: Platform>(cx: &'a Cx<'_, P>) -> Actor<'a> {
    Actor {
        id: &cx.session().user_id,
        request_id: cx.request_id,
    }
}

const ROLES: &[(&str, &str)] = &[
    ("admin", "Admin"),
    ("member", "Member"),
    ("viewer", "Viewer"),
];

pub async fn page<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let t = cx.tenant();
    let doc = members::list(cx.p(), &t.id, cx.now).await?;
    let me = &cx.session().user_id;
    let role = cx.role();
    let manage = cx.can("members:manage");
    let csrf = cx.csrf();
    let admins: Vec<(String, String)> = doc["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["role"] == "admin")
        .map(|m| (layout::text(&m["user_id"]), layout::text(&m["email"])))
        .collect();
    let admin_opts: Vec<(&str, &str)> = admins
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    Ok(cx.page(
        "Members",
        html! {
            (flash(cx))
            p { "Seats used: " (layout::text(&doc["seats"]["used"])) @if let Some(g) = doc["seats"]["granted"].as_i64() { " of " (g) } }
            table {
                thead { tr { th scope="col" { "Email" } th scope="col" { "Name" } th scope="col" { "Role" } th scope="col" { "Last sign-in" } @if manage { th scope="col" { "Action" } } } }
                tbody {
                    @for m in doc["data"].as_array().into_iter().flatten() {
                        @let uid = layout::text(&m["user_id"]);
                        @let is_owner = m["role"] == "owner";
                        tr {
                            td { (layout::text(&m["email"])) }
                            td { (layout::text(&m["name"])) }
                            td { (layout::text(&m["role"])) }
                            td { (layout::text(&m["last_login_at"])) }
                            @if manage {
                                td {
                                    @if !is_owner && uid != *me {
                                        (post_form(&format!("/console/members/{uid}/role"), &csrf, "inline", html! {
                                            (select(&format!("role-{uid}"), "Role", ROLES, m["role"].as_str().unwrap_or("")))
                                            button type="submit" class="secondary" { "Change role" }
                                        }))
                                        " "
                                        (button(&format!("/console/members/{uid}/remove"), &csrf, "Remove", true))
                                    }
                                }
                            }
                        }
                    }
                }
            }
            h2 { "Pending invitations" }
            @let invs = doc["invitations"].as_array().cloned().unwrap_or_default();
            @if invs.is_empty() { p class="muted" { "None." } }
            @else {
                table {
                    thead { tr { th scope="col" { "Email" } th scope="col" { "Role" } th scope="col" { "Expires" } @if manage { th scope="col" { "Action" } } } }
                    tbody {
                        @for i in &invs {
                            tr {
                                td { (layout::text(&i["email"])) }
                                td { (layout::text(&i["role"])) }
                                td { (layout::text(&i["expires_at"])) }
                                @if manage { td { (button(&format!("/console/members/invitations/{}/revoke", layout::text(&i["id"])), &csrf, "Revoke", true)) } }
                            }
                        }
                    }
                }
            }
            @if manage {
                h2 { "Invite someone" }
                (post_form("/console/members/invite", &csrf, "", html! {
                    (field("email", "Email address", "email", "", true))
                    (select("role", "Role", ROLES, "member"))
                    p { button type="submit" { "Send invitation" } }
                }))
            }
            @if role == Role::Owner {
                h2 { "Transfer ownership" }
                @if admin_opts.is_empty() { p class="muted" { "Make someone an admin first." } }
                @else {
                    (post_form("/console/members/transfer", &csrf, "", html! {
                        (select("user_id", "New owner (an admin)", &admin_opts, ""))
                        p { button type="submit" class="danger" { "Transfer ownership" } }
                    }))
                }
            } @else {
                h2 { "Leave the workspace" }
                (button("/console/members/leave", &csrf, "Leave this workspace", true))
            }
        },
    ))
}

pub async fn invite<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let a = actor(cx);
    invitations::create(
        cx.p(),
        cx.tenant(),
        cx.f("email").unwrap_or(""),
        cx.f("role").unwrap_or(""),
        Some(&cx.session().user_id),
        &a,
        cx.now,
    )
    .await?;
    Ok(done("/console/members", "invited"))
}

pub async fn revoke<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    invitations::revoke(
        cx.p(),
        cx.tenant(),
        cx.param("invitation_id"),
        &actor(cx),
        cx.now,
    )
    .await?;
    Ok(done("/console/members", "revoked"))
}

pub async fn role<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let uid = cx.param("user_id");
    let to = cx
        .f(&format!("role-{uid}"))
        .or(cx.f("role"))
        .and_then(Role::parse)
        .ok_or_else(|| ApiError::invalid("role", "role is admin, member or viewer."))?;
    members::change_role(cx.p(), cx.tenant(), uid, to, cx.role(), &actor(cx), cx.now).await?;
    Ok(done("/console/members", "saved"))
}

pub async fn remove<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    members::remove(
        cx.p(),
        cx.tenant(),
        cx.param("user_id"),
        false,
        &actor(cx),
        cx.now,
    )
    .await?;
    Ok(done("/console/members", "removed"))
}

pub async fn transfer<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let me = cx.session().user_id.clone();
    members::transfer(
        cx.p(),
        cx.tenant(),
        &me,
        cx.f("user_id").unwrap_or(""),
        &actor(cx),
        cx.now,
    )
    .await?;
    Ok(done("/console/members", "transferred"))
}

/// Leaving: the owner cannot (`409 owner_required`); the session ends with the membership.
pub async fn leave<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let me = cx.session().user_id.clone();
    members::remove(cx.p(), cx.tenant(), &me, true, &actor(cx), cx.now).await?;
    Ok(see_other("/console/workspaces"))
}
