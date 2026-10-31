//! Mail in the console ([Console › Showing untrusted mail]): a thread in text view, HTML on request in a
//! token-less sandboxed `srcdoc` frame with `cid:` images inlined as `data:` URIs, attachments served
//! from the console host with the API's serving headers, search, and quarantine with release.
//!
//! [Console › Showing untrusted mail]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md#showing-untrusted-mail

use maud::{Markup, html};
use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::{BlobStore as _, ControlDb as _, Platform};
use serde_json::{Value, json};

use super::{done, flash};
use crate::console::layout::{self, field, post_form};
use crate::console::{CSP, Cx};
use crate::ctx::Reply;
use crate::db::identities as idb;
use crate::errors::ApiError;
use crate::mailbox::MailboxRequest;
use crate::mailbox::read::ReadRequest;

const INLINE_MAX: i64 = 2 * 1024 * 1024;
const INLINE_TOTAL: i64 = 8 * 1024 * 1024;
const INLINE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// The identity, if it is in the session's workspace (the same `404` as a missing one otherwise).
async fn identity<P: Platform>(cx: &Cx<'_, P>, id: &str) -> Result<idb::Row, ApiError> {
    let not_found = || ApiError::not_found(ErrorCode::IdentityNotFound);
    let row: idb::Row = cx
        .p()
        .db()
        .first(idb::get(id))
        .await?
        .ok_or_else(not_found)?;
    if row.tenant_id != cx.tenant().id || row.is_system == 1 || row.mailbox_do_id.is_empty() {
        return Err(not_found());
    }
    Ok(row)
}

async fn mailbox<P: Platform>(
    cx: &Cx<'_, P>,
    idn: &idb::Row,
    req: ReadRequest,
) -> Result<Value, ApiError> {
    use pylota_identity_platform::ObjectClient as _;
    use pylota_identity_platform::durable::{DoClass, RpcResult};
    let env = crate::rpc::envelope_for(
        cx.p(),
        Some(&idn.tenant_id),
        Some(&idn.id),
        cx.request_id,
        None,
        MailboxRequest::Read(req),
    );
    match cx
        .p()
        .objects()
        .call::<_, Value>(DoClass::Mailbox, &idn.mailbox_do_id, &env)
        .await?
    {
        RpcResult::Ok(v) => Ok(v),
        RpcResult::Err(e) => Err(ApiError::new(e.error.code, e.error.message)),
    }
}

/// The message's sanitised HTML with its `cid:` images as `data:` URIs (each ≤ 2 MiB, ≤ 8 MiB per
/// message, sniffed as an image); the rest are listed as links to the attachment route.
async fn inline_html<P: Platform>(
    cx: &Cx<'_, P>,
    idn: &idb::Row,
    message_id: &str,
    html_in: &str,
) -> Result<(String, Vec<(String, String)>), ApiError> {
    let parts = mailbox(
        cx,
        idn,
        ReadRequest::InlineParts {
            message_id: message_id.to_owned(),
            review: false,
        },
    )
    .await?;
    let mut out = html_in.to_owned();
    let mut skipped = Vec::new();
    let mut total = 0;
    for p in parts["parts"].as_array().into_iter().flatten() {
        let cid = p["content_id"].as_str().unwrap_or("");
        let cid = cid.trim_start_matches('<').trim_end_matches('>');
        let reference = format!("cid:{cid}");
        if cid.is_empty() || !out.contains(&reference) {
            continue;
        }
        let size = p["size"].as_i64().unwrap_or(i64::MAX);
        let kind = p["sniffed_type"].as_str().unwrap_or("");
        let att = layout::text(&p["id"]);
        if INLINE_TYPES.contains(&kind) && size <= INLINE_MAX && total + size <= INLINE_TOTAL {
            let key = p["r2_key"].as_str().unwrap_or("");
            if let Some(blob) = cx.p().blobs().get(key).await? {
                total += size;
                let uri = format!(
                    "data:{kind};base64,{}",
                    pylota_identity_core::links::encode_base64(&blob.body.to_vec())
                );
                out = out.replace(&reference, &uri);
                continue;
            }
        }
        out = out.replace(&reference, "");
        skipped.push((
            att.clone(),
            format!(
                "/console/inboxes/{}/messages/{message_id}/attachments/{att}",
                idn.id
            ),
        ));
    }
    Ok((out, skipped))
}

fn addr(v: &Value) -> String {
    match (v["name"].as_str(), v["address"].as_str()) {
        (Some(n), Some(a)) if !n.is_empty() => format!("{n} <{a}>"),
        (_, Some(a)) => a.to_owned(),
        _ => "–".into(),
    }
}

pub async fn thread<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let id = cx.param("identity_id");
    let thr = cx.param("thread_id");
    let doc = cx
        .api(
            "GET",
            &format!("/v1/identities/{id}/threads/{thr}"),
            &[],
            None,
        )
        .await?;
    let idn = identity(cx, id).await?;
    let html_for = cx.q("html");
    let remote = cx.q("images") == Some("remote");
    let mut blocks: Vec<Markup> = Vec::new();
    for m in doc["messages"].as_array().into_iter().flatten() {
        let mid = layout::text(&m["id"]);
        let held = matches!(m["status"].as_str(), Some("quarantined" | "screening"));
        let body = m["text"]
            .as_str()
            .or(m["extracted_text"].as_str())
            .unwrap_or("");
        let show_html = !held && html_for == Some(mid.as_str()) && m["html"].is_string();
        let (srcdoc, skipped) = if show_html {
            inline_html(cx, &idn, &mid, m["html"].as_str().unwrap_or("")).await?
        } else {
            (String::new(), Vec::new())
        };
        let base = format!("/console/inboxes/{id}/threads/{thr}");
        blocks.push(html! {
            article {
                h2 { (layout::text(&m["subject"])) }
                p class="muted" {
                    "From " (addr(&m["from"])) " · " (layout::text(&m["received_at"])) " · " (layout::text(&m["status"]))
                    @if m["flags"].as_array().is_some_and(|f| f.iter().any(|x| x == "arc_rescued")) {
                        " · Authenticated by the forwarder (ARC)"
                    }
                }
                @if held {
                    p class="notice" { "Held: " (layout::text(&m["quarantine_reason"])) ". Only the text is shown." }
                }
                @if show_html {
                    iframe class="mail" sandbox="" title="Message HTML" srcdoc=(srcdoc) {}
                    @if !skipped.is_empty() {
                        p { "Inline images not shown: " @for (att, href) in &skipped { a href=(href) { (att) } " " } }
                    }
                    p {
                        a href=(base.clone()) { "Show text" }
                        @if !remote {
                            " · " a href=(format!("{base}?html={mid}&images=remote")) { "Load remote images" }
                            " (the sender may learn that the mail was opened)"
                        }
                    }
                } @else {
                    pre class="mail" { (body) }
                    @if !held && m["html"].is_string() { p { a href=(format!("{base}?html={mid}")) { "Show HTML" } } }
                }
                @if !held {
                    @let atts = m["attachments"].as_array().cloned().unwrap_or_default();
                    @if !atts.is_empty() {
                        ul {
                            @for a in &atts {
                                li {
                                    a href=(format!("/console/inboxes/{id}/messages/{mid}/attachments/{}", layout::text(&a["id"]))) {
                                        (a["filename"].as_str().unwrap_or("attachment"))
                                    }
                                    " (" (layout::text(&a["content_type"])) ", " (layout::text(&a["size"])) " bytes)"
                                }
                            }
                        }
                    }
                }
            }
        });
    }
    let mut r = cx.page(
        &layout::text(&doc["subject"]),
        html! {
            p { a href=(format!("/console/inboxes/{id}")) { "Back to the inbox" } }
            @for b in &blocks { (b) }
        },
    );
    if remote && html_for.is_some() {
        r.headers.push((
            "content-security-policy".into(),
            CSP.replace("img-src 'self' data:", "img-src 'self' data: https:"),
        ));
    }
    Ok(r)
}

/// `GET /console/inboxes/{idn}/messages/{msg}/attachments/{att}`: the API's attachment route and its
/// serving headers, so the page never loads anything from the API host.
pub async fn attachment<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let path = format!(
        "/v1/identities/{}/messages/{}/attachments/{}",
        cx.param("identity_id"),
        cx.param("message_id"),
        cx.param("attachment_id")
    );
    let key = cx.principal.clone().ok_or_else(ApiError::internal)?;
    let call = crate::app::Call {
        method: "GET".into(),
        path,
        query: Vec::new(),
        headers: Vec::new(),
        body: None,
    };
    cx.app.serve_as(key, call, cx.request_id).await
}

pub async fn search<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let q = cx.q("q").unwrap_or("").trim().to_owned();
    let scope = cx.q("identity").unwrap_or("").to_owned();
    let idns = cx
        .api(
            "GET",
            &format!("/v1/tenants/{}/identities", cx.tenant().id),
            &[("limit", "100".into())],
            None,
        )
        .await?;
    let options: Vec<(String, String)> = std::iter::once((String::new(), "Every inbox".to_owned()))
        .chain(
            idns["data"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|i| (layout::text(&i["id"]), layout::text(&i["primary_address"]))),
        )
        .collect();
    let results = if q.is_empty() {
        Value::Null
    } else if scope.is_empty() {
        cx.api(
            "POST",
            &format!("/v1/tenants/{}/search", cx.tenant().id),
            &[],
            Some(json!({ "q": q, "limit": 25 })),
        )
        .await?
    } else {
        cx.api(
            "POST",
            &format!("/v1/identities/{scope}/search"),
            &[],
            Some(json!({ "q": q, "limit": 25 })),
        )
        .await?
    };
    let hits = results["hits"].as_array().cloned().unwrap_or_default();
    let opts: Vec<(&str, &str)> = options
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    Ok(cx.page(
        "Search",
        html! {
            form method="get" action="/console/search" role="search" {
                (field("q", "Search for", "search", &q, true))
                (layout::select("identity", "In", &opts, &scope))
                p { button type="submit" { "Search" } }
            }
            @if !q.is_empty() {
                h2 { (hits.len()) " results" }
                @if hits.is_empty() { p class="muted" { "No messages match." } }
                ul {
                    @for h in &hits {
                        li {
                            a href=(format!("/console/inboxes/{}/threads/{}", layout::text(&h["identity_id"]), layout::text(&h["thread_id"]))) {
                                (h["subject"].as_str().unwrap_or("(no subject)"))
                            }
                            " – " (addr(&h["from"])) ", " (layout::text(&h["date"]))
                            br;
                            span class="muted" { (layout::text(&h["snippet"])) }
                        }
                    }
                }
            }
        },
    ))
}

pub async fn quarantine<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let idns = cx
        .api(
            "GET",
            &format!("/v1/tenants/{}/identities", cx.tenant().id),
            &[("limit", "100".into())],
            None,
        )
        .await?;
    let csrf = cx.csrf();
    let mut sections = Vec::new();
    for i in idns["data"].as_array().into_iter().flatten() {
        let id = layout::text(&i["id"]);
        let q = cx
            .api(
                "GET",
                &format!("/v1/identities/{id}/quarantine"),
                &[("limit", "50".into())],
                None,
            )
            .await?;
        let items = q["data"].as_array().cloned().unwrap_or_default();
        if items.is_empty() {
            continue;
        }
        sections.push(html! {
            h2 { (layout::text(&i["primary_address"])) }
            table {
                thead { tr { th scope="col" { "From" } th scope="col" { "Subject" } th scope="col" { "Reason" } th scope="col" { "Received" } th scope="col" { "Release" } } }
                tbody {
                    @for m in &items {
                        @let mid = layout::text(&m["id"]);
                        tr {
                            td { (addr(&m["from"])) }
                            td { (layout::text(&m["subject"])) }
                            td { (layout::text(&m["quarantine_reason"])) }
                            td { (layout::text(&m["received_at"])) }
                            td {
                                (post_form(&format!("/console/quarantine/{id}/{mid}/release"), &csrf, "", html! {
                                    (field(&format!("reason-{mid}"), "Why release it", "text", "", true))
                                    button type="submit" { "Release" }
                                }))
                            }
                        }
                    }
                }
            }
        });
    }
    Ok(cx.page(
        "Quarantine",
        html! {
            (flash(cx))
            @if sections.is_empty() { p class="muted" { "Nothing is held." } }
            @for s in &sections { (s) }
        },
    ))
}

pub async fn release<P: Platform>(cx: &Cx<'_, P>) -> Result<Reply, ApiError> {
    let id = cx.param("identity_id");
    let mid = cx.param("message_id");
    let reason = cx
        .f(&format!("reason-{mid}"))
        .or(cx.f("reason"))
        .unwrap_or("")
        .to_owned();
    cx.api(
        "POST",
        &format!("/v1/identities/{id}/messages/{mid}/release"),
        &[],
        Some(json!({ "reason": reason })),
    )
    .await?;
    Ok(done("/console/quarantine", "released"))
}
