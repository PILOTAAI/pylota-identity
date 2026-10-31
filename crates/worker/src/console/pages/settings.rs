//! `/console/settings` (the person's name and sessions; the owner's workspace settings and deleting
//! the workspace) and `/console/settings/policy` ([Workspace policy § 6]).
//!
//! [Workspace policy § 6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/workspace-policy.md#6-the-console-page

use maud::{Markup, html};
use pylota_identity_api_types::ids::IdPrefix;
use pylota_identity_core::policy::get_path;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::{ControlDb as _, Ids as _, Platform};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{done, flash};
use crate::console::layout::{self, button, field, post_form};
use crate::console::{Cx, html as page_html};
use crate::ctx::Reply;
use crate::db::{audit, tenants as tdb};
use crate::errors::ApiError;

#[derive(Deserialize)]
struct S {
    user_agent_hint: Option<String>,
    authenticated_at: i64,
    last_seen_at: i64,
    id_hash: String,
}

pub async fn page<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let s = cx.session();
    let sessions: Vec<S> = cx
        .p()
        .db()
        .query(Stmt::new(
            "SELECT user_agent_hint, authenticated_at, last_seen_at, id_hash FROM sessions WHERE user_id = ?1 AND revoked_at IS NULL AND expires_at > ?2 ORDER BY last_seen_at DESC LIMIT 50",
            vec![s.user_id.clone().into(), cx.now.into()],
        ))
        .await?;
    let csrf = cx.csrf();
    let owner = cx.can("tenants:erase");
    let t = cx.tenant.as_ref();
    Ok(cx.page(
        "Settings",
        html! {
            (flash(cx))
            h2 { "You" }
            (post_form("/console/settings/profile", &csrf, "", html! {
                (field("name", "Your name", "text", s.name.as_deref().unwrap_or(""), false))
                p { button type="submit" { "Save" } }
            }))
            h2 { "Sessions" }
            table {
                thead { tr { th scope="col" { "Browser" } th scope="col" { "Signed in" } th scope="col" { "Last seen" } } }
                tbody {
                    @for x in &sessions {
                        tr {
                            td { (x.user_agent_hint.as_deref().unwrap_or("Other")) @if x.id_hash == s.id_hash { " (this one)" } }
                            td { (pylota_identity_core::days::rfc3339(x.authenticated_at)) }
                            td { (pylota_identity_core::days::rfc3339(x.last_seen_at)) }
                        }
                    }
                }
            }
            (button("/console/settings/sign-out-everywhere", &csrf, "Sign out everywhere else", false))
            @if let (true, Some(t)) = (owner, t) {
                h2 { "Workspace" }
                (post_form("/console/settings/workspace", &csrf, "", html! {
                    (field("name", "Workspace name", "text", &t.name, true))
                    (field("timezone", "Time zone (IANA, for example Europe/London)", "text", &t.timezone, true))
                    p { button type="submit" { "Save the workspace" } }
                }))
                p { a href="/console/settings/policy" { "Workspace policy" } }
                h2 { "Delete the workspace" }
                p { "Deleting the workspace erases its inboxes, mail, keys and members. It cannot be undone." }
                (post_form("/console/settings/delete-workspace", &csrf, "", html! {
                    (field("confirm", "Type the workspace name to confirm", "text", "", true))
                    p { button type="submit" class="danger" { "Delete this workspace" } }
                }))
            }
        },
    ))
}

pub async fn profile<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let name: String = cx.f("name").unwrap_or("").trim().chars().take(78).collect();
    cx.p()
        .db()
        .execute(Stmt::new(
            "UPDATE users SET name = ?2 WHERE id = ?1",
            vec![
                cx.session().user_id.clone().into(),
                (!name.is_empty()).then_some(name).into(),
            ],
        ))
        .await?;
    Ok(done("/console/settings", "saved"))
}

/// The owner's workspace settings: name and time zone, through a console-only handler that changes
/// exactly those columns (Console › Roles, workspace settings).
pub async fn workspace<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let t = cx.tenant();
    let name: String = cx
        .f("name")
        .unwrap_or("")
        .trim()
        .chars()
        .take(100)
        .collect();
    let tz = cx.f("timezone").unwrap_or("").trim().to_owned();
    if name.is_empty() {
        return Err(ApiError::invalid("name", "The workspace needs a name."));
    }
    if !pylota_identity_core::days::valid_timezone(&tz) {
        return Err(ApiError::invalid(
            "timezone",
            "Use an IANA time zone, for example Europe/London.",
        ));
    }
    let uid = cx.session().user_id.clone();
    cx.p()
        .db()
        .batch(vec![
            Stmt::new(
                "UPDATE tenants SET name = ?2, timezone = ?3, updated_at = ?4 WHERE id = ?1",
                vec![t.id.clone().into(), name.into(), tz.into(), cx.now.into()],
            ),
            audit::insert(
                &cx.p().ids().new_id(IdPrefix::Audit),
                Some(&t.id),
                Some(&uid),
                "tenant.settings_update",
                "tenant",
                &t.id,
                &json!({ "fields": ["name", "timezone"] }),
                cx.request_id,
                cx.now,
            ),
        ])
        .await?;
    Ok(done("/console/settings", "saved"))
}

pub async fn sign_out_everywhere<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let s = cx.session();
    cx.p()
        .db()
        .execute(Stmt::new(
            "UPDATE sessions SET revoked_at = ?3 WHERE user_id = ?1 AND id_hash <> ?2 AND revoked_at IS NULL",
            vec![s.user_id.clone().into(), s.id_hash.clone().into(), cx.now.into()],
        ))
        .await?;
    Ok(done("/console/settings", "signed_out"))
}

/// Deleting the workspace: a tenant-scope erasure, which needs `tenants:erase`, held by the owner's
/// session principal alone (W35).
pub async fn delete_workspace<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let t = cx.tenant();
    if cx.f("confirm").map(str::trim) != Some(t.name.as_str()) {
        return Err(ApiError::invalid(
            "confirm",
            "Type the workspace name exactly to confirm.",
        ));
    }
    cx.api(
        "POST",
        "/v1/erasure-requests",
        &[],
        Some(json!({ "scope": "tenant", "tenant_id": t.id })),
    )
    .await?;
    Ok(done("/console/workspaces", "erasing"))
}

fn policy_input(
    field: &str,
    value: &Value,
    writable: bool,
    ceiling: &Value,
    reason: &str,
) -> Markup {
    let id = format!("p-{}", field.replace('.', "-"));
    if !writable {
        return html! {
            div {
                span class="muted" { (field) ": " } code { (layout::text(value)) }
                @if !reason.is_empty() { " " span class="muted" { "(" (reason) ")" } }
            }
        };
    }
    html! {
        div {
            @match value {
                Value::Bool(b) => {
                    input type="hidden" name="_bool" value=(field);
                    input type="checkbox" id=(id) name=(field) value="true" checked[*b];
                    " " label for=(id) style="display:inline" { (field) }
                }
                Value::Number(n) => {
                    label for=(id) { (field) }
                    @if let Some(max) = ceiling.as_f64() {
                        input type="number" id=(id) name=(field) value=(n.to_string()) max=(max) step="any";
                    } @else {
                        input type="number" id=(id) name=(field) value=(n.to_string()) step="any";
                    }
                }
                Value::Array(_) | Value::Object(_) => {
                    label for=(id) { (field) " (JSON)" }
                    textarea id=(id) name=(field) { (value.to_string()) }
                }
                Value::Null => {
                    label for=(id) { (field) " (empty for none)" }
                    input type="text" id=(id) name=(field) value="";
                }
                Value::String(s) => {
                    label for=(id) { (field) }
                    input type="text" id=(id) name=(field) value=(s);
                }
            }
        }
    }
}

fn reason_of(f: &Value) -> &'static str {
    match f["class"].as_str() {
        Some("platform_only") => "set by the platform operator",
        Some("platform_or_partner") => "set by your provider",
        _ => "",
    }
}

/// The policy as the session may see it: from `GET …/policy` when the role may write it, from the
/// effective policy otherwise (every field read-only).
async fn view<P: Platform>(cx: &Cx<'_, P>) -> Result<(Value, bool), ApiError> {
    let t = cx.tenant();
    if cx.can("policy:write") {
        let v = cx
            .api("GET", &format!("/v1/tenants/{}/policy", t.id), &[], None)
            .await?;
        return Ok((v, true));
    }
    let eff = crate::policy::effective(cx.p(), t);
    let fields: Vec<Value> = pylota_identity_core::policy::FIELDS
        .iter()
        .filter(|(f, _)| !f.ends_with(".*"))
        .map(
            |(f, c)| json!({ "field": f, "class": c.as_str(), "writable": false, "ceiling": null }),
        )
        .collect();
    Ok((
        json!({ "policy_version": t.policy_version, "policy": eff, "fields": fields }),
        false,
    ))
}

fn policy_page<P: Platform>(cx: &Cx<'_, P>, v: &Value, can_write: bool, top: Markup) -> Reply {
    let csrf = cx.csrf();
    let fields = v["fields"].as_array().cloned().unwrap_or_default();
    let inputs = html! {
        @for f in &fields {
            @let name = f["field"].as_str().unwrap_or("");
            @let val = get_path(&v["policy"], name).cloned().unwrap_or(Value::Null);
            (policy_input(name, &val, can_write && f["writable"] == true, &f["ceiling"], reason_of(f)))
        }
    };
    cx.page(
        "Workspace policy",
        html! {
            (flash(cx))
            (top)
            @if can_write {
                (post_form("/console/settings/policy", &csrf, "", html! {
                    input type="hidden" name="policy_version" value=(layout::text(&v["policy_version"]));
                    fieldset { legend { "Policy" } (inputs) }
                    p { button type="submit" { "Save the policy" } }
                }))
            } @else {
                (inputs)
            }
        },
    )
}

pub async fn policy<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let (v, w) = view(cx).await?;
    Ok(policy_page(cx, &v, w, html! {}))
}

fn set_path(root: &mut Map<String, Value>, path: &str, v: Value) {
    let mut parts = path.split('.').peekable();
    let mut cur = root;
    while let Some(p) = parts.next() {
        if parts.peek().is_none() {
            cur.insert(p.to_owned(), v);
            return;
        }
        let next = cur
            .entry(p.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if !next.is_object() {
            *next = Value::Object(Map::new());
        }
        cur = next.as_object_mut().expect("an object");
    }
}

/// A submitted value in the type of the field's current value.
fn parse(old: &Value, raw: Option<&str>, is_bool: bool) -> Option<Value> {
    if is_bool {
        return Some(Value::Bool(raw == Some("true")));
    }
    let raw = raw?.trim();
    match old {
        Value::Number(_) => raw.parse::<f64>().ok().map(|f| {
            if f.fract() == 0.0 && f.abs() < 9e15 {
                json!(f as i64)
            } else {
                json!(f)
            }
        }),
        Value::Array(_) | Value::Object(_) => serde_json::from_str(raw).ok(),
        Value::Null => {
            if raw.is_empty() {
                Some(Value::Null)
            } else {
                Some(raw.parse::<i64>().map_or_else(|_| json!(raw), |n| json!(n)))
            }
        }
        _ => Some(json!(raw)),
    }
}

/// Whether a patch deletes mail sooner: `retention.raw_days` or `retention.events_days` lowered, or
/// `retention.message_days` set to a number from `null` or lowered.
fn deletes(old: &Value, patch: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for f in [
        "retention.raw_days",
        "retention.events_days",
        "retention.message_days",
    ] {
        let (Some(new), o) = (get_path(patch, f), get_path(old, f)) else {
            continue;
        };
        let lower = match (o.and_then(Value::as_f64), new.as_f64()) {
            (Some(a), Some(b)) => b < a,
            (None, Some(_)) => true,
            _ => false,
        };
        if lower {
            out.push(format!("{f} to {new}"));
        }
    }
    out
}

/// Saving: only the fields that differ from the effective policy at submit time, as one patch; a stale
/// `policy_version` stores nothing; a change that deletes mail asks for a confirmed second `POST`.
pub async fn policy_save<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let (v, _) = view(cx).await?;
    let t: tdb::Row = cx
        .p()
        .db()
        .first(tdb::get(&cx.tenant().id))
        .await?
        .ok_or_else(ApiError::internal)?;
    if cx.f("policy_version") != Some(t.policy_version.to_string().as_str()) {
        return Ok(policy_page(
            cx,
            &v,
            true,
            layout::notice(
                "The policy changed since you opened this page. Check the values and save again.",
            ),
        ));
    }
    let bools = cx.fs("_bool");
    let mut patch = Map::new();
    for f in v["fields"].as_array().into_iter().flatten() {
        let name = f["field"].as_str().unwrap_or("");
        if f["writable"] != true {
            continue;
        }
        let is_bool = bools.contains(&name);
        if !is_bool && cx.f(name).is_none() {
            continue;
        }
        let old = get_path(&v["policy"], name).cloned().unwrap_or(Value::Null);
        let Some(new) = parse(&old, cx.f(name), is_bool) else {
            return Err(ApiError::invalid(
                name,
                format!("{name} has a value of the wrong kind."),
            ));
        };
        if new != old {
            set_path(&mut patch, name, new);
        }
    }
    if patch.is_empty() {
        return Ok(done("/console/settings/policy", "unchanged"));
    }
    let patch = Value::Object(patch);
    let deleting = deletes(&v["policy"], &patch);
    if !deleting.is_empty() && cx.f("confirm_deletion") != Some("yes") {
        let csrf = cx.csrf();
        let mut r = page_html(
            200,
            layout::page(
                "Confirm the change",
                cx.frame(&csrf).as_ref(),
                html! {
                    p { "This change makes the next daily retention run delete mail sooner: " (deleting.join(", ")) ". Threads under a legal hold are kept." }
                    (post_form("/console/settings/policy", &csrf, "", html! {
                        @for (k, val) in &cx.form { @if k != "_csrf" && k != "confirm_deletion" { input type="hidden" name=(k) value=(val); } }
                        div { input type="checkbox" id="confirm_deletion" name="confirm_deletion" value="yes" required; " " label for="confirm_deletion" style="display:inline" { "I understand that this mail will be deleted" } }
                        p { button type="submit" class="danger" { "Save and delete" } }
                    }))
                },
            ),
        );
        r.status = 200;
        return Ok(r);
    }
    match cx
        .api(
            "PATCH",
            &format!("/v1/tenants/{}/policy", t.id),
            &[],
            Some(patch),
        )
        .await
    {
        Ok(_) => Ok(done("/console/settings/policy", "saved")),
        Err(e) => {
            let msg = format!(
                "{}{}",
                e.message,
                e.fix
                    .as_deref()
                    .map(|f| format!(" {f}"))
                    .unwrap_or_default()
            );
            let field = e
                .details
                .as_ref()
                .and_then(|d| d["field"].as_str())
                .unwrap_or("")
                .to_owned();
            let mut r = policy_page(
                cx,
                &v,
                true,
                layout::error(&if field.is_empty() {
                    msg
                } else {
                    format!("{field}: {msg}")
                }),
            );
            r.status = e.code.http_status();
            Ok(r)
        }
    }
}
