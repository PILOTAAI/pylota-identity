//! Passwordless sign-in ([Console › Sign-in]): one request writes a `login_tokens` row with a link
//! token and a six-digit code (keyed hashes only) and, after the response, emails both from the system
//! identity, only to a known person or a pending invitation. The link's `GET` changes nothing; its
//! `POST`, or the code, signs in once. Also sign-out, re-authentication, the workspace picker and
//! invitation acceptance.
//!
//! [Console › Sign-in]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md#sign-in

use maud::html;
use pylota_identity_api_types::ErrorCode;
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::console as cc;
use pylota_identity_core::days::rfc3339;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{Background as _, ControlDb as _, Ids as _, Platform, Rng as _};
use serde::Deserialize;

use super::layout::{self, field, public_form};
use super::{Cx, html as page_html, see_other, session};
use crate::ctx::Reply;
use crate::errors::ApiError;
use crate::handlers::Count;
use crate::keyring::{self, Purpose};
use crate::members::invitations;

const TEN_MIN: i64 = 600_000;
const DAY_MS: i64 = 86_400_000;

fn public_page<P: Platform>(cx: &Cx<'_, P>, title: &str, body: maud::Markup) -> Reply {
    if cx.session.is_some() {
        return cx.page(title, body);
    }
    page_html(200, layout::page(title, None, body))
}

fn with_cookie(mut r: Reply, cookie: String) -> Reply {
    r.headers.push(("set-cookie".into(), cookie));
    r
}

pub async fn sign_in_page<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    if cx.session.is_some() {
        return Ok(see_other("/console"));
    }
    Ok(public_page(
        cx,
        "Sign in",
        html! {
            p { "We send a sign-in link and a code to your email address." }
            (public_form("/console/sign-in", html! {
                (field("email", "Email address", "email", "", true))
                p { button type="submit" { "Email me a link" } }
            }))
        },
    ))
}

#[derive(Clone, Debug, Deserialize)]
struct Token {
    id: String,
    email: String,
    purpose: String,
    code_hash: String,
    key_kid: String,
}

fn draw_code<P: Platform>(p: &P) -> String {
    loop {
        let draws: Vec<u32> = (0..4)
            .map(|_| u32::from_le_bytes(p.rng().bytes::<4>()))
            .collect();
        if let Some(c) = cc::code_from(&draws) {
            return c;
        }
    }
}

/// Writes a sign-in token for `email`: `(token id, link token, code)`, or `None` past the limit of 3
/// per 10 minutes for the address.
async fn new_token<P: Platform>(
    p: &P,
    email: &str,
    now: i64,
) -> Result<Option<(String, String, String)>, ApiError> {
    let recent: Option<Count> = p
        .db()
        .first(Stmt::new(
            "SELECT COUNT(*) AS n FROM login_tokens WHERE email = ?1 AND created_at > ?2",
            vec![email.into(), (now - TEN_MIN).into()],
        ))
        .await?;
    if recent.is_some_and(|n| n.n >= 3) {
        return Ok(None);
    }
    let key = keyring::current(p, Purpose::Link).await?;
    let token = cc::token(&key.kid, &p.rng().bytes::<32>());
    let code = draw_code(p);
    let id = p
        .ids()
        .new_id(IdPrefix::Request)
        .replacen("req_", "ltk_", 1);
    p.db()
        .execute(Stmt::new(
            "INSERT INTO login_tokens (id, email, purpose, token_hash, code_hash, key_kid, expires_at, created_at) VALUES (?1, ?2, 'sign_in', ?3, ?4, ?5, ?6, ?7)",
            vec![
                id.clone().into(),
                email.into(),
                cc::keyed_hash(&key.secret, &token).into(),
                cc::code_hash(&key.secret, email, &code).into(),
                key.kid.into(),
                (now + TEN_MIN).into(),
                now.into(),
            ],
        ))
        .await?;
    Ok(Some((id, token, code)))
}

/// Whether sign-in mail may go to `email`: an active person, or a pending invitation (W15).
async fn may_mail<P: Platform>(p: &P, email: &str, now: i64) -> bool {
    let n: Option<Count> = p
        .db()
        .first(Stmt::new(
            "SELECT (SELECT COUNT(*) FROM users WHERE email = ?1 AND status = 'active') + (SELECT COUNT(*) FROM invitations WHERE email = ?1 AND status = 'pending' AND expires_at > ?2) AS n",
            vec![email.into(), now.into()],
        ))
        .await
        .ok()
        .flatten();
    n.is_some_and(|n| n.n > 0)
}

/// Sends the sign-in email after the response, so the response is the same for every address.
fn send_later<P: Platform>(cx: &Cx<'_, P>, email: String, id: String, token: String, code: String) {
    let p = cx.app.p.clone();
    let rid = cx.request_id.to_owned();
    let now = cx.now;
    cx.p().background().wait_until(async move {
        if !may_mail(p.as_ref(), &email, now).await {
            return;
        }
        let host = p.config().console_host.clone();
        let text = format!(
            "Sign in to Pylota Identity.\n\nOpen this link:\nhttps://{host}/console/sign-in/link?t={token}\n\nOr enter this code: {code}\n\nRequested at {}. The link and the code work once, for 10 minutes. If you did not ask to sign in, ignore this email.\n",
            rfc3339(now)
        );
        let _ = crate::system_mail::send(
            p.as_ref(),
            &email,
            "Your sign-in link",
            &text,
            &format!("signin:{id}"),
            &rid,
        )
        .await;
    });
}

fn check_email_page<P: Platform>(cx: &Cx<'_, P>, email: &str, action: &str) -> Reply {
    public_page(
        cx,
        "Check your email",
        html! {
            p { "If the address can sign in, a link and a code are on their way. They work for 10 minutes." }
            (public_form(action, html! {
                input type="hidden" name="email" value=(email);
                (field("code", "Six-digit code", "text", "", true))
                p { button type="submit" { "Sign in with the code" } }
            }))
        },
    )
}

fn too_many<P: Platform>(cx: &Cx<'_, P>) -> Reply {
    let mut r = public_page(
        cx,
        "Too many requests",
        html! { p { "Too many sign-in requests for this address. Wait 10 minutes, then try again." } },
    );
    r.status = 429;
    r
}

/// `POST /console/sign-in`.
pub async fn request<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let Some(email) = cx.f("email").and_then(invitations::email) else {
        let mut r = public_page(
            cx,
            "Sign in",
            html! { (layout::error("Enter a valid email address.")) p { a href="/console/sign-in" { "Back" } } },
        );
        r.status = 400;
        return Ok(r);
    };
    match new_token(cx.p(), &email, cx.now).await? {
        None => Ok(too_many(cx)),
        Some((id, token, code)) => {
            send_later(cx, email.clone(), id, token, code);
            Ok(check_email_page(cx, &email, "/console/sign-in/code"))
        }
    }
}

/// `GET /console/sign-in/link?t=…`: changes nothing, so a mail scanner cannot burn the token.
pub async fn link_page<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let t = cx.q("t").unwrap_or("");
    Ok(public_page(
        cx,
        "Sign in",
        html! {
            p { "Continue to sign in to Pylota Identity." }
            (public_form("/console/sign-in/link", html! {
                input type="hidden" name="t" value=(t);
                button type="submit" { "Sign in" }
            }))
        },
    ))
}

fn invalid<P: Platform>(cx: &Cx<'_, P>) -> Reply {
    let mut r = public_page(
        cx,
        "Sign-in failed",
        html! {
            (layout::error("This link or code does not work any more."))
            p { a href="/console/sign-in" { "Request a new one" } }
        },
    );
    r.status = 400;
    r
}

/// `POST /console/sign-in/link`.
pub async fn link_use<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let t = cx.f("t").unwrap_or("");
    let Some(hash) = invitations::hash_of(cx.p(), t).await? else {
        return Ok(invalid(cx));
    };
    let row: Option<Token> = cx
        .p()
        .db()
        .first(Stmt::new(
            "SELECT id, email, purpose, code_hash, key_kid FROM login_tokens WHERE token_hash = ?1 AND used_at IS NULL AND expires_at > ?2 AND attempts < 10",
            vec![hash.into(), cx.now.into()],
        ))
        .await?;
    match row {
        Some(r) => complete(cx, &r).await,
        None => Ok(invalid(cx)),
    }
}

/// `POST /console/sign-in/code`: at most 10 attempts per token and 30 failed codes per address per
/// UTC day (W43).
pub async fn code_use<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let email = cx
        .f("email")
        .and_then(invitations::email)
        .unwrap_or_default();
    let code = cx.f("code").unwrap_or("").trim().to_owned();
    match check_code(cx, &email, &code).await? {
        Ok(row) => complete(cx, &row).await,
        Err(locked) => Ok(if locked {
            let mut r = public_page(
                cx,
                "Code sign-in locked",
                html! { p { "Too many wrong codes for this address today. Use the link in the email instead." } },
            );
            r.status = 429;
            r
        } else {
            invalid(cx)
        }),
    }
}

/// The token a code matches, or `Err(locked)`.
async fn check_code<P: Platform>(
    cx: &Cx<'_, P>,
    email: &str,
    code: &str,
) -> Result<Result<Token, bool>, ApiError> {
    let p = cx.p();
    let day = cx.now.div_euclid(DAY_MS) * DAY_MS;
    let failed: Option<Count> = p
        .db()
        .first(Stmt::new(
            "SELECT COALESCE(SUM(attempts), 0) AS n FROM login_tokens WHERE email = ?1 AND created_at >= ?2",
            vec![email.into(), day.into()],
        ))
        .await?;
    if failed.is_some_and(|n| n.n >= 30) {
        return Ok(Err(true));
    }
    let rows: Vec<Token> = p
        .db()
        .query(Stmt::new(
            "SELECT id, email, purpose, code_hash, key_kid FROM login_tokens WHERE email = ?1 AND used_at IS NULL AND expires_at > ?2 AND attempts < 10 ORDER BY created_at DESC LIMIT 3",
            vec![email.into(), cx.now.into()],
        ))
        .await?;
    let keys = keyring::keys(p, Purpose::Link).await?;
    let valid = code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit());
    for r in &rows {
        if let Some(k) = keys.iter().find(|k| k.kid == r.key_kid)
            && valid
            && cc::same(&cc::code_hash(&k.secret, email, code), &r.code_hash)
        {
            return Ok(Ok(r.clone()));
        }
    }
    for r in &rows {
        p.db()
            .execute(Stmt::new(
                "UPDATE login_tokens SET attempts = attempts + 1 WHERE id = ?1",
                vec![r.id.clone().into()],
            ))
            .await?;
    }
    Ok(Err(false))
}

/// Uses a token once (link or code burn each other) and signs the person in.
async fn complete<P: Platform>(cx: &Cx<'_, P>, row: &Token) -> Result<Reply, ApiError> {
    let p = cx.p();
    let used = p
        .db()
        .execute(Stmt::new(
            "UPDATE login_tokens SET used_at = ?2 WHERE id = ?1 AND used_at IS NULL",
            vec![row.id.clone().into(), cx.now.into()],
        ))
        .await?;
    if used.changes == 0 || row.purpose != "sign_in" {
        return Ok(invalid(cx));
    }
    // A person who only had a pending invitation gets a `users` row; signing in accepts nothing.
    let pending: Vec<invitations::Row> = p
        .db()
        .query(invitations::for_email(&row.email, cx.now))
        .await?;
    if !pending.is_empty() {
        p.db()
            .execute(Stmt::new(
                "INSERT INTO users (id, email, status, created_at) VALUES (?1, ?2, 'active', ?3) ON CONFLICT (email) DO NOTHING",
                vec![
                    p.ids().new_id(IdPrefix::ConsoleUser).into(),
                    row.email.clone().into(),
                    cx.now.into(),
                ],
            ))
            .await?;
    }
    #[derive(Deserialize)]
    struct U {
        id: String,
    }
    let Some(u) = p
        .db()
        .first::<U>(Stmt::new(
            "SELECT id FROM users WHERE email = ?1 AND status = 'active'",
            vec![row.email.clone().into()],
        ))
        .await?
    else {
        return Ok(invalid(cx));
    };
    sign_in_as(cx, &u.id, !pending.is_empty()).await
}

/// Creates a session for `user_id` (revoking the browser's current one) and lands the person.
pub async fn sign_in_as<P: Platform>(
    cx: &Cx<'_, P>,
    user_id: &str,
    has_invitations: bool,
) -> Result<Reply, ApiError> {
    let p = cx.p();
    if let Some(old) = &cx.session {
        p.db()
            .execute(session::revoke(&old.id_hash, cx.now))
            .await?;
    }
    let tenant = session::landing_tenant(p, user_id).await?;
    let value = session::create(
        p,
        user_id,
        tenant.as_deref(),
        cx.header("user-agent"),
        cx.now,
    )
    .await?;
    let to = match (&tenant, has_invitations) {
        (None, true) => "/console/invitations".to_owned(),
        (None, false) => "/console/workspaces".to_owned(),
        (Some(_), _) => cx
            .q("next")
            .and_then(cc::safe_next)
            .unwrap_or("/console")
            .to_owned(),
    };
    Ok(with_cookie(see_other(&to), session::set_cookie(&value)))
}

/// `POST /console/sign-out`.
pub async fn sign_out<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    cx.p()
        .db()
        .execute(session::revoke(&cx.session().id_hash, cx.now))
        .await?;
    Ok(with_cookie(
        see_other("/console/sign-in"),
        session::clear_cookie(),
    ))
}

/// `GET /console/reauth?next=…`.
pub async fn reauth_page<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let next = cx.q("next").and_then(cc::safe_next).unwrap_or("/console");
    Ok(cx.page(
        "Confirm it is you",
        html! {
            p { "This action needs a recent sign-in. We send a code to " strong { (cx.session().email) } "." }
            (layout::post_form("/console/reauth", &cx.csrf(), "", html! {
                input type="hidden" name="next" value=(next);
                button type="submit" { "Email me a code" }
            }))
        },
    ))
}

/// `POST /console/reauth`: a new sign-in token for the person, counted in the same limits.
pub async fn reauth_send<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let email = cx.session().email.clone();
    let next = cx
        .f("next")
        .and_then(cc::safe_next)
        .unwrap_or("/console")
        .to_owned();
    match new_token(cx.p(), &email, cx.now).await? {
        None => Ok(too_many(cx)),
        Some((id, token, code)) => {
            send_later(cx, email.clone(), id, token, code);
            Ok(cx.page(
                "Confirm it is you",
                html! {
                    p { "Enter the code we sent to " strong { (email) } "." }
                    (layout::post_form("/console/reauth/code", &cx.csrf(), "", html! {
                        input type="hidden" name="next" value=(next);
                        (field("code", "Six-digit code", "text", "", true))
                        p { button type="submit" { "Confirm" } }
                    }))
                },
            ))
        }
    }
}

/// `POST /console/reauth/code`: a correct code replaces the session with a new one
/// (`authenticated_at = now`) and returns to `next`; the person submits the action again.
pub async fn reauth_code<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let s = cx.session().clone();
    let code = cx.f("code").unwrap_or("").trim().to_owned();
    let next = cx
        .f("next")
        .and_then(cc::safe_next)
        .unwrap_or("/console")
        .to_owned();
    match check_code(cx, &s.email, &code).await? {
        Ok(row) => {
            let used = cx
                .p()
                .db()
                .execute(Stmt::new(
                    "UPDATE login_tokens SET used_at = ?2 WHERE id = ?1 AND used_at IS NULL",
                    vec![row.id.clone().into(), cx.now.into()],
                ))
                .await?;
            if used.changes == 0 {
                return Ok(invalid(cx));
            }
            cx.p()
                .db()
                .execute(session::revoke(&s.id_hash, cx.now))
                .await?;
            let value = session::create(
                cx.p(),
                &s.user_id,
                s.tenant_id.as_deref(),
                cx.header("user-agent"),
                cx.now,
            )
            .await?;
            Ok(with_cookie(see_other(&next), session::set_cookie(&value)))
        }
        Err(_) => Ok(invalid(cx)),
    }
}

#[derive(Deserialize)]
struct Ws {
    tenant_id: String,
    name: String,
    role: String,
    mode: String,
}

/// `GET /console/workspaces`: the person's workspaces and pending invitations.
pub async fn workspaces<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let s = cx.session();
    let list: Vec<Ws> = cx
        .p()
        .db()
        .query(Stmt::new(
            "SELECT m.tenant_id, t.name, m.role, t.mode FROM members m JOIN tenants t ON t.id = m.tenant_id WHERE m.user_id = ?1 AND t.status IN ('active','suspended') ORDER BY t.name",
            vec![s.user_id.clone().into()],
        ))
        .await?;
    let pending: Vec<invitations::Row> = cx
        .p()
        .db()
        .query(invitations::for_email(&s.email, cx.now))
        .await?;
    let csrf = cx.csrf();
    Ok(cx.page(
        "Workspaces",
        html! {
            @if list.is_empty() { p { "You are not a member of a workspace yet." } }
            @else {
                table {
                    thead { tr { th scope="col" { "Workspace" } th scope="col" { "Role" } th scope="col" { span class="skip" { "Action" } } } }
                    tbody {
                        @for w in &list {
                            tr {
                                td { (w.name) @if w.mode == "test" { " " span class="badge" { "Test" } } }
                                td { (w.role) }
                                td {
                                    @if s.tenant_id.as_deref() == Some(w.tenant_id.as_str()) { "Current" }
                                    @else {
                                        (layout::post_form("/console/workspaces/switch", &csrf, "inline", html! {
                                            input type="hidden" name="tenant_id" value=(w.tenant_id);
                                            button type="submit" class="secondary" { "Switch to " (w.name) }
                                        }))
                                    }
                                }
                            }
                        }
                    }
                }
            }
            @if !pending.is_empty() {
                p { a href="/console/invitations" { (pending.len()) " pending invitation(s)" } }
            }
        },
    ))
}

/// `POST /console/workspaces/switch`: only to a workspace the person is a member of.
pub async fn switch<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let s = cx.session();
    let tid = cx.f("tenant_id").unwrap_or("");
    let member: Option<Count> = cx
        .p()
        .db()
        .first(Stmt::new(
            "SELECT COUNT(*) AS n FROM members WHERE tenant_id = ?1 AND user_id = ?2",
            vec![tid.into(), s.user_id.clone().into()],
        ))
        .await?;
    if member.is_none_or(|m| m.n == 0) {
        return Err(ApiError::not_found(ErrorCode::TenantNotFound));
    }
    cx.p()
        .db()
        .batch(vec![
            session::switch(&s.id_hash, tid),
            Stmt::new(
                "UPDATE users SET last_tenant_id = ?2 WHERE id = ?1",
                vec![s.user_id.clone().into(), tid.into()],
            ),
        ])
        .await?;
    Ok(see_other("/console"))
}

/// `GET /console/invitations`: the person's pending invitations, each with its own Accept form.
pub async fn invitations_page<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let s = cx.session();
    let pending: Vec<invitations::Row> = cx
        .p()
        .db()
        .query(invitations::for_email(&s.email, cx.now))
        .await?;
    let names = tenant_names(cx.p(), &pending).await?;
    let csrf = cx.csrf();
    Ok(cx.page(
        "Invitations",
        html! {
            @if pending.is_empty() { p { "You have no pending invitations." } }
            @for (inv, name) in pending.iter().zip(&names) {
                section {
                    h2 { (name) }
                    p { "Role: " (inv.role) ". Expires " (rfc3339(inv.expires_at)) "." }
                    (layout::button(&format!("/console/invitations/{}/accept", inv.id), &csrf, &format!("Accept the invitation to {name}"), false))
                }
            }
        },
    ))
}

async fn tenant_names<P: Platform>(
    p: &P,
    invs: &[invitations::Row],
) -> Result<Vec<String>, ApiError> {
    let mut out = Vec::new();
    for i in invs {
        let t: Option<crate::db::tenants::Row> =
            p.db().first(crate::db::tenants::get(&i.tenant_id)).await?;
        out.push(t.map(|t| t.name).unwrap_or_default());
    }
    Ok(out)
}

/// `POST /console/invitations/{invitation_id}/accept`: in the current session, for the session's own
/// address only.
pub async fn accept_mine<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let s = cx.session().clone();
    let id = cx.param("invitation_id");
    let inv = cx
        .p()
        .db()
        .first::<invitations::Row>(Stmt::new(
            "SELECT id, tenant_id, email, role, invited_by, status, expires_at, created_at FROM invitations WHERE id = ?1 AND status = 'pending' AND expires_at > ?2",
            vec![id.into(), cx.now.into()],
        ))
        .await?
        .filter(|i| i.email == s.email)
        .ok_or_else(|| ApiError::not_found(ErrorCode::InvitationNotFound))?;
    invitations::accept(cx.p(), &inv, cx.request_id, cx.now).await?;
    cx.p()
        .db()
        .execute(session::switch(&s.id_hash, &inv.tenant_id))
        .await?;
    Ok(see_other("/console"))
}

/// `GET /console/invitations/accept?t=…`: shows the workspace and role with an Accept button.
pub async fn accept_page<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let t = cx.q("t").unwrap_or("");
    let Some(inv) = invitations::by_token(cx.p(), t, cx.now).await? else {
        return Ok(gone(cx));
    };
    let name = tenant_names(cx.p(), std::slice::from_ref(&inv))
        .await?
        .remove(0);
    Ok(public_page(
        cx,
        "Accept the invitation",
        html! {
            p { "You are invited to the workspace " strong { (name) } " as " (inv.role) "." }
            (public_form("/console/invitations/accept", html! {
                input type="hidden" name="t" value=(t);
                button type="submit" { "Accept" }
            }))
        },
    ))
}

fn gone<P: Platform>(cx: &Cx<'_, P>) -> Reply {
    let mut r = public_page(
        cx,
        "Invitation not valid",
        html! { (layout::error("This invitation was used, revoked or has expired. Ask for a new one.")) },
    );
    r.status = 404;
    r
}

/// `POST /console/invitations/accept`: accepts for the invited address. In a browser signed in as the
/// invited person it is accepted in that session; signed in as someone else, nothing is accepted;
/// otherwise the membership and a session are created, as a sign-in link would. With the console off
/// no session is created.
pub async fn accept_post<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let t = cx.f("t").unwrap_or("");
    let Some(inv) = invitations::by_token(cx.p(), t, cx.now).await? else {
        return Ok(gone(cx));
    };
    if let Some(s) = &cx.session
        && s.email != inv.email
    {
        return Ok(public_page(
            cx,
            "Signed in as someone else",
            html! {
                p { "This browser is signed in as " strong { (s.email) } ", not the invited address. Nothing was accepted." }
                (layout::button("/console/sign-out", &cx.csrf(), "Sign out", false))
            },
        ));
    }
    let user_id = invitations::accept(cx.p(), &inv, cx.request_id, cx.now).await?;
    if cx.p().config().console.is_none() {
        let name = tenant_names(cx.p(), std::slice::from_ref(&inv))
            .await?
            .remove(0);
        return Ok(page_html(
            200,
            layout::page(
                "Invitation accepted",
                None,
                html! { p { "You are now a member of " (name) "." } },
            ),
        ));
    }
    if let Some(s) = &cx.session {
        cx.p()
            .db()
            .execute(session::switch(&s.id_hash, &inv.tenant_id))
            .await?;
        return Ok(see_other("/console"));
    }
    let value = session::create(
        cx.p(),
        &user_id,
        Some(&inv.tenant_id),
        cx.header("user-agent"),
        cx.now,
    )
    .await?;
    Ok(with_cookie(
        see_other("/console"),
        session::set_cookie(&value),
    ))
}

/// The sign-in link of a new workspace's owner (`POST /v1/tenants` with `owner`), sent like any
/// sign-in email, so the owner can sign in.
pub async fn owner_welcome<P: Platform>(
    p: std::rc::Rc<P>,
    email: String,
    request_id: String,
    now: i64,
) {
    if p.config().console.is_none() {
        return;
    }
    let Ok(Some((id, token, code))) = new_token(p.as_ref(), &email, now).await else {
        return;
    };
    let host = p.config().console_host.clone();
    let text = format!(
        "A Pylota Identity workspace was created for you.\n\nSign in:\nhttps://{host}/console/sign-in/link?t={token}\n\nOr enter this code: {code}\n\nThe link and the code work once, for 10 minutes. Afterwards, sign in at https://{host}/console/sign-in.\n"
    );
    let _ = crate::system_mail::send(
        p.as_ref(),
        &email,
        "Your Pylota Identity workspace",
        &text,
        &format!("signin:{id}"),
        &request_id,
    )
    .await;
}
