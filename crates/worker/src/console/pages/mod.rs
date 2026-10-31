//! The console's screens ([Console › Screens]). Each page reads through the same REST routes as the
//! API, with the session's principal; each form posts to a console route that calls the same route.
//! Screens of later milestones (drafts, plan, accounts, deliverability, notifications, the Overview)
//! arrive with them.
//!
//! [Console › Screens]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md#screens

pub mod admin;
pub mod inboxes;
pub mod mail;
pub mod members;
pub mod settings;

use maud::{Markup, html};
use pylota_identity_platform::Platform;
use serde_json::Value;

use super::layout::{self, CSS};
use super::router::Page;
use super::{Cx, see_other, signin};
use crate::ctx::Reply;
use crate::errors::ApiError;

pub async fn dispatch<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    use Page as G;
    match cx.m.route.page {
        G::Css => Ok(Reply {
            status: 200,
            body: None,
            raw: Some(("text/css; charset=utf-8", CSS.as_bytes().to_vec())),
            headers: vec![("cache-control".into(), "public, max-age=3600".into())],
            stream: None,
        }),
        G::SignIn => signin::sign_in_page(cx).await,
        G::SignInRequest => signin::request(cx).await,
        G::SignInLink => signin::link_page(cx).await,
        G::SignInLinkUse => signin::link_use(cx).await,
        G::SignInCode => signin::code_use(cx).await,
        G::SignOut => signin::sign_out(cx).await,
        G::InvitationAccept => signin::accept_page(cx).await,
        G::InvitationAcceptPost => signin::accept_post(cx).await,
        G::Invitations => signin::invitations_page(cx).await,
        G::InvitationAcceptMine => signin::accept_mine(cx).await,
        G::Reauth => signin::reauth_page(cx).await,
        G::ReauthSend => signin::reauth_send(cx).await,
        G::ReauthCode => signin::reauth_code(cx).await,
        G::Workspaces => signin::workspaces(cx).await,
        G::WorkspaceSwitch => signin::switch(cx).await,
        G::Home => home(cx).await,
        G::Inboxes => inboxes::list(cx).await,
        G::InboxCreate => inboxes::create(cx).await,
        G::Inbox => inboxes::show(cx).await,
        G::InboxStatus => inboxes::status(cx).await,
        G::KeyCreateIdentity => inboxes::key_create(cx).await,
        G::KeyRotateIdentity => inboxes::key_rotate(cx).await,
        G::KeyRevokeIdentity => inboxes::key_revoke(cx).await,
        G::Thread => mail::thread(cx).await,
        G::Attachment => mail::attachment(cx).await,
        G::Search => mail::search(cx).await,
        G::Quarantine => mail::quarantine(cx).await,
        G::QuarantineRelease => mail::release(cx).await,
        G::Keys => admin::keys(cx).await,
        G::KeyCreate => admin::key_create(cx).await,
        G::KeyRevoke => admin::key_revoke(cx).await,
        G::Domains => admin::domains(cx).await,
        G::Domain => admin::domain(cx).await,
        G::DomainCreate => admin::domain_create(cx).await,
        G::DomainVerify => admin::domain_verify(cx).await,
        G::DomainDelete => admin::domain_delete(cx).await,
        G::Webhooks => admin::webhooks(cx).await,
        G::WebhookCreate => admin::webhook_create(cx).await,
        G::WebhookDelete => admin::webhook_delete(cx).await,
        G::Audit => admin::audit(cx).await,
        G::Members => members::page(cx).await,
        G::MemberInvite => members::invite(cx).await,
        G::InvitationRevoke => members::revoke(cx).await,
        G::MemberRole => members::role(cx).await,
        G::MemberRemove => members::remove(cx).await,
        G::OwnershipTransfer => members::transfer(cx).await,
        G::Leave => members::leave(cx).await,
        G::Settings => settings::page(cx).await,
        G::SettingsProfile => settings::profile(cx).await,
        G::SettingsWorkspace => settings::workspace(cx).await,
        G::SignOutEverywhere => settings::sign_out_everywhere(cx).await,
        G::DeleteWorkspace => settings::delete_workspace(cx).await,
        G::Policy => settings::policy(cx).await,
        G::PolicySave => settings::policy_save(cx).await,
    }
}

/// The workspace home: its name, the frame and a link to each screen the role may open (M24 replaces
/// it with the Overview).
async fn home<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let t = cx.tenant();
    let links: [(&str, &str, &str, &str); 9] = [
        (
            "/console/inboxes",
            "Inboxes",
            "identities:read",
            "Identities, their mail and signing keys",
        ),
        (
            "/console/search",
            "Search",
            "search:read",
            "Search the workspace's mail",
        ),
        (
            "/console/quarantine",
            "Quarantine",
            "quarantine:review",
            "Held mail, and release",
        ),
        (
            "/console/domains",
            "Domains",
            "domains:read",
            "Domains, health and DNS records",
        ),
        (
            "/console/keys",
            "API keys",
            "keys:manage",
            "Keys for agents and integrations",
        ),
        (
            "/console/webhooks",
            "Webhooks",
            "webhooks:manage",
            "Endpoints that receive events",
        ),
        (
            "/console/members",
            "Members",
            "members:read",
            "People and invitations",
        ),
        (
            "/console/audit",
            "Audit log",
            "audit:read",
            "Who changed what",
        ),
        (
            "/console/settings/policy",
            "Policy",
            "members:read",
            "The workspace policy",
        ),
    ];
    Ok(cx.page(
        &t.name,
        html! {
            @if t.status == "suspended" { (layout::error("This workspace is suspended: its identities cannot send or sign.")) }
            p { "Signed in as " (cx.session().email) ", " (cx.role().as_str()) " of this workspace." }
            ul {
                @for (href, label, perm, what) in links {
                    @if cx.can(perm) { li { a href=(href) { (label) } " – " (what) } }
                }
            }
        },
    ))
}

/// A table of API objects: one column per `(field, heading)`; `link` makes the first column a link.
pub fn table(
    rows: &[Value],
    cols: &[(&str, &str)],
    link: Option<&dyn Fn(&Value) -> String>,
) -> Markup {
    html! {
        @if rows.is_empty() { p class="muted" { "Nothing here yet." } }
        @else {
            table {
                thead { tr { @for (_, h) in cols { th scope="col" { (h) } } } }
                tbody {
                    @for r in rows {
                        tr {
                            @for (i, (f, _)) in cols.iter().enumerate() {
                                td {
                                    @match (i, link) {
                                        (0, Some(l)) => { a href=(l(r)) { (layout::text(&r[*f])) } }
                                        _ => { (layout::text(&r[*f])) }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A "next page" link carrying the API's cursor.
pub fn next_link(base: &str, doc: &Value) -> Markup {
    html! {
        @if let Some(c) = doc["next_cursor"].as_str() {
            p { a href=(format!("{base}{}cursor={}", if base.contains('?') { "&" } else { "?" }, super::urlencode(c))) { "Next page" } }
        }
    }
}

/// After a form: back to `to` with a notice code in the query.
pub fn done(to: &str, code: &str) -> Reply {
    let sep = if to.contains('?') { "&" } else { "?" };
    see_other(&format!("{to}{sep}done={code}"))
}

/// The notices a redirect may name; any other value shows nothing, so a link cannot put text on a page.
const NOTICES: &[(&str, &str)] = &[
    ("created", "Created."),
    ("saved", "Saved."),
    ("revoked", "Revoked."),
    (
        "rotated",
        "Rotated: the previous key keeps verifying until its date.",
    ),
    ("deleted", "Deleted."),
    ("released", "Released."),
    ("invited", "Invitation sent."),
    ("removed", "Removed."),
    ("verifying", "Verification started."),
    ("transferred", "Ownership transferred."),
    ("paused", "Paused."),
    ("resumed", "Resumed."),
    ("signed_out", "Every other session is signed out."),
    ("unchanged", "Nothing changed."),
    ("erasing", "The workspace is being deleted."),
];

/// The notice a redirect carried.
pub fn flash<P: Platform>(cx: &Cx<'_, P>) -> Markup {
    let text = cx
        .q("done")
        .and_then(|c| NOTICES.iter().find(|(k, _)| *k == c))
        .map(|(_, t)| *t);
    html! { @if let Some(t) = text { (layout::notice(t)) } }
}
