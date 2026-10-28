//! Messages ([Inbound › Read path and release]): list, get, label and read-state changes, the raw MIME,
//! attachments and their extracted text. Every call goes to the identity's mailbox after the scope
//! check; the mailbox applies the visibility rule with `review` (`quarantine:review`).
//!
//! [Inbound › Read path and release]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::days::parse_rfc3339;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::{BlobStore as _, ObjectClient as _, Platform};
use serde_json::{Value, json};

use super::identity_in_scope;
use crate::ctx::{Ctx, Reply};
use crate::db::identities as idb;
use crate::errors::ApiError;
use crate::mailbox::MailboxRequest;
use crate::mailbox::read::{Include, ListBoundary, MessageFilters, ReadRequest};
use crate::rpc::{RpcResult, envelope_for};

/// Header fields read for `include=headers`: count, header block bytes, bytes per value.
const HEADER_FIELDS: (usize, usize, usize) = (200, 64 * 1024, 2048);
/// At most this much of an attachment's text per answer.
const TEXT_BYTES: usize = 200 * 1024;
/// An attachment whose object is missing answers `503` while its message is younger than this.
const PUT_GRACE_MS: i64 = 5 * 60_000;

/// One read request to the identity's mailbox; an error answer becomes the same API error.
pub async fn mailbox<P: Platform>(
    c: &Ctx<P>,
    idn: &idb::Row,
    req: ReadRequest,
) -> Result<Value, ApiError> {
    call_mailbox(c, idn, MailboxRequest::Read(req)).await
}

/// One request to the identity's mailbox; its error answer becomes the API error.
pub async fn call_mailbox<P: Platform>(
    c: &Ctx<P>,
    idn: &idb::Row,
    req: MailboxRequest,
) -> Result<Value, ApiError> {
    let env = envelope_for(
        c.p.as_ref(),
        Some(&idn.tenant_id),
        Some(&idn.id),
        &c.request_id,
        None,
        req,
    );
    match c
        .p
        .objects()
        .call::<_, Value>(DoClass::Mailbox, &idn.mailbox_do_id, &env)
        .await?
    {
        RpcResult::Ok(v) => Ok(v),
        RpcResult::Err(e) => {
            let mut err = ApiError::new(e.error.code, e.error.message);
            if let Some(d) = e.error.details {
                err = err.details(serde_json::to_value(d).unwrap_or(Value::Null));
            }
            Err(err)
        }
    }
}

/// Whether the key holds `quarantine:review`.
pub fn review<P: Platform>(c: &Ctx<P>) -> bool {
    c.key().permissions.iter().any(|p| p == "quarantine:review")
}

/// `include`: repeated or comma-separated.
pub fn include<P: Platform>(c: &Ctx<P>) -> Result<Include, ApiError> {
    let mut inc = Include::default();
    for (_, v) in c.query.iter().filter(|(n, _)| n == "include") {
        for part in v.split(',').filter(|p| !p.is_empty()) {
            match part {
                "quoted" => inc.quoted = true,
                "html" => inc.html = true,
                "headers" => inc.headers = true,
                _ => return Err(ApiError::invalid("include", "Unknown include.")),
            }
        }
    }
    Ok(inc)
}

/// An RFC 3339 query parameter as milliseconds.
pub fn time<P: Platform>(c: &Ctx<P>, name: &str) -> Result<Option<i64>, ApiError> {
    match c.query(name) {
        None => Ok(None),
        Some(v) => parse_rfc3339(v)
            .map(Some)
            .ok_or_else(|| ApiError::invalid(name, "Not an RFC 3339 time.")),
    }
}

pub fn boolean<P: Platform>(c: &Ctx<P>, name: &str) -> Result<Option<bool>, ApiError> {
    match c.query(name) {
        None => Ok(None),
        Some("true") => Ok(Some(true)),
        Some("false") => Ok(Some(false)),
        Some(_) => Err(ApiError::invalid(name, "true or false.")),
    }
}

/// A `Label` (`^[a-z0-9][a-z0-9_:-]{0,63}$`).
pub fn valid_label(l: &str) -> bool {
    let b = l.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b[1..].iter().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b':' | b'-')
        })
}

/// `labels_add` and `labels_remove` of a body.
pub fn labels(body: &Value, field: &str) -> Result<Vec<String>, ApiError> {
    match &body[field] {
        Value::Null => Ok(Vec::new()),
        Value::Array(a) if a.len() <= 64 => a
            .iter()
            .map(|v| match v.as_str() {
                Some(l) if valid_label(l) => Ok(l.to_owned()),
                _ => Err(ApiError::invalid(field, "Not a label.")),
            })
            .collect(),
        _ => Err(ApiError::invalid(field, "A list of at most 64 labels.")),
    }
}

pub fn only_fields(body: &Value, allowed: &[&str]) -> Result<(), ApiError> {
    if let Some(o) = body.as_object()
        && let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str()))
    {
        return Err(ApiError::invalid(k, "Unknown field."));
    }
    Ok(())
}

/// The keyset of a list's `cursor`.
pub async fn boundary<P: Platform>(
    c: &Ctx<P>,
    filters: &[&str],
) -> Result<Option<ListBoundary>, ApiError> {
    Ok(c.after(filters).await?.map(|(sort_ms, key)| ListBoundary {
        sort_ms,
        key: key.parse().unwrap_or(0),
    }))
}

/// A page from the mailbox's `{items: [{sort, key, value}]}`.
pub async fn page<P: Platform>(
    c: &Ctx<P>,
    out: Value,
    limit: u32,
    filters: &[&str],
) -> Result<Reply, ApiError> {
    let items = out["items"].as_array().cloned().unwrap_or_default();
    c.page(
        items,
        limit,
        filters,
        |v| {
            (
                v["sort"].as_i64().unwrap_or(0),
                v["key"].as_i64().unwrap_or(0).to_string(),
            )
        },
        |v| v["value"].clone(),
    )
    .await
}

/// Fills `headers` from the raw MIME in R2 and removes the internal `_raw_r2_key`.
pub async fn fill_headers<P: Platform>(c: &Ctx<P>, m: &mut Value) -> Result<(), ApiError> {
    let Some(obj) = m.as_object_mut() else {
        return Ok(());
    };
    let Some(key) = obj.remove("_raw_r2_key") else {
        return Ok(());
    };
    if let Some(key) = key.as_str()
        && let Some(b) = c.p.blobs().get(key).await?
    {
        let (n, bytes, v) = HEADER_FIELDS;
        let fields = pylota_identity_core::mime::header_fields(&b.body.to_vec(), n, bytes, v);
        obj.insert(
            "headers".into(),
            fields
                .into_iter()
                .map(|(name, value)| json!({ "name": name, "value": value }))
                .collect(),
        );
    }
    Ok(())
}

const LIST_FILTERS: &[&str] = &[
    "thread_id",
    "direction",
    "status",
    "screening_status",
    "label",
    "after",
    "before",
];

pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let limit = c.limit()?;
    let cursor = boundary(c, LIST_FILTERS).await?;
    let direction = match c.query("direction") {
        None => None,
        Some(d @ ("inbound" | "outbound")) => Some(d.to_owned()),
        Some(_) => return Err(ApiError::invalid("direction", "inbound or outbound.")),
    };
    let status = match c.query("status") {
        None => None,
        Some(
            s @ ("received" | "quarantined" | "screening" | "hidden" | "throttled" | "queued"
            | "submitted" | "sent" | "failed" | "cancelled" | "scheduled" | "pending_approval"),
        ) => Some(s.to_owned()),
        Some(_) => return Err(ApiError::invalid("status", "Unknown status.")),
    };
    let label = match c.query("label") {
        Some(l) if !valid_label(l) => return Err(ApiError::invalid("label", "Not a label.")),
        l => l.map(str::to_owned),
    };
    let filters = MessageFilters {
        thread_id: c.query("thread_id").map(str::to_owned),
        direction,
        status,
        label,
        after_ms: time(c, "after")?,
        before_ms: time(c, "before")?,
    };
    let out = mailbox(
        c,
        &idn,
        ReadRequest::ListMessages {
            filters,
            limit,
            cursor,
            review: review(c),
        },
    )
    .await?;
    page(c, out, limit, LIST_FILTERS).await
}

pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let mut m = mailbox(
        c,
        &idn,
        ReadRequest::GetMessage {
            message_id: c.route.param("message_id").to_owned(),
            include: include(c)?,
            review: review(c),
        },
    )
    .await?;
    fill_headers(c, &mut m).await?;
    Ok(Reply::json(200, m))
}

pub async fn update<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let body = c.json()?;
    only_fields(&body, &["labels_add", "labels_remove", "read"])?;
    let read = match &body["read"] {
        Value::Null => None,
        Value::Bool(b) => Some(*b),
        _ => return Err(ApiError::invalid("read", "true or false.")),
    };
    let m = mailbox(
        c,
        &idn,
        ReadRequest::UpdateMessage {
            message_id: c.route.param("message_id").to_owned(),
            labels_add: labels(&body, "labels_add")?,
            labels_remove: labels(&body, "labels_remove")?,
            read,
            review: review(c),
        },
    )
    .await?;
    Ok(Reply::json(200, m))
}

pub async fn raw<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let out = mailbox(
        c,
        &idn,
        ReadRequest::Raw {
            message_id: c.route.param("message_id").to_owned(),
            review: review(c),
        },
    )
    .await?;
    let gone = || ApiError::new(ErrorCode::RawExpired, "The raw MIME is past its retention.");
    let key = out["raw_r2_key"].as_str().ok_or_else(gone)?;
    let blob = c.p.blobs().get(key).await?.ok_or_else(gone)?;
    let mut r = Reply::json(200, Value::Null);
    r.body = None;
    r.raw = Some(("message/rfc822", blob.body.to_vec()));
    Ok(r)
}

async fn attachment_row<P: Platform>(c: &Ctx<P>, idn: &idb::Row) -> Result<Value, ApiError> {
    mailbox(
        c,
        idn,
        ReadRequest::Attachment {
            message_id: c.route.param("message_id").to_owned(),
            attachment_id: c.route.param("attachment_id").to_owned(),
            review: review(c),
        },
    )
    .await
}

/// A content-disposition filename: printable ASCII without quotes or backslashes.
fn disposition_name(name: &str) -> String {
    let n: String = name
        .chars()
        .filter(|ch| ch.is_ascii_graphic() || *ch == ' ')
        .filter(|ch| !matches!(ch, '"' | '\\'))
        .collect();
    if n.is_empty() { "attachment".into() } else { n }
}

pub async fn attachment<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let a = attachment_row(c, &idn).await?;
    let key = a["r2_key"].as_str().unwrap_or("");
    let Some(blob) = c.p.blobs().get(key).await? else {
        // Attachment bytes are put after the commit: briefly missing on a new message.
        if c.now - a["received_at"].as_i64().unwrap_or(0) < PUT_GRACE_MS {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "The attachment is still being stored; retry.",
            )
            .header("Retry-After", "5"));
        }
        return Err(ApiError::not_found(ErrorCode::AttachmentNotFound));
    };
    let name = disposition_name(a["filename"].as_str().unwrap_or("attachment"));
    let mut r = Reply::json(200, Value::Null);
    r.body = None;
    r.raw = Some(("application/octet-stream", blob.body.to_vec()));
    r.headers.push((
        "content-disposition".into(),
        format!("attachment; filename=\"{name}\""),
    ));
    r.headers
        .push(("x-content-type-options".into(), "nosniff".into()));
    r.headers.push((
        "content-security-policy".into(),
        "default-src 'none'; sandbox".into(),
    ));
    Ok(r)
}

/// `pages`: `N` or `N-M`.
fn page_range(v: Option<&str>) -> Result<Option<(u32, u32)>, ApiError> {
    let Some(v) = v else {
        return Ok(None);
    };
    let bad = || ApiError::invalid("pages", "N or N-M, from 1.");
    let (a, b) = match v.split_once('-') {
        Some((a, b)) => (a, b),
        None => (v, v),
    };
    let (a, b): (u32, u32) = (a.parse().map_err(|_| bad())?, b.parse().map_err(|_| bad())?);
    if a == 0 || b < a {
        return Err(bad());
    }
    Ok(Some((a, b)))
}

pub async fn attachment_text<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let range = page_range(c.query("pages"))?;
    let a = attachment_row(c, &idn).await?;
    let status = a["text_status"].as_str().unwrap_or("skipped").to_owned();
    let total = a["text_pages"].as_i64();
    let mut pages = Vec::new();
    let mut truncated = false;
    if status == "ready"
        && let Some(key) = a["text_r2_key"].as_str()
        && let Some(blob) = c.p.blobs().get(key).await?
    {
        let md = String::from_utf8_lossy(&blob.body.to_vec()).into_owned();
        let mut used = 0usize;
        for (i, text) in pylota_identity_core::attach_text::split_rendered(&md)
            .into_iter()
            .enumerate()
        {
            let n = i as u32 + 1;
            if range.is_some_and(|(a, b)| n < a || n > b) {
                continue;
            }
            if used + text.len() > TEXT_BYTES {
                let mut cut = TEXT_BYTES - used;
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                pages.push(json!({ "page": n, "text": &text[..cut] }));
                truncated = true;
                break;
            }
            used += text.len();
            pages.push(json!({ "page": n, "text": text }));
        }
    }
    Ok(Reply::json(
        200,
        json!({ "status": status, "pages": pages, "total_pages": total, "truncated": truncated }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_ranges() {
        assert!(valid_label("crm:synced") && valid_label("a") && valid_label("9-x_y"));
        assert!(
            !valid_label("A")
                && !valid_label("")
                && !valid_label("-a")
                && !valid_label(&"a".repeat(65))
        );
        assert_eq!(page_range(Some("2-3")).unwrap(), Some((2, 3)));
        assert_eq!(page_range(Some("4")).unwrap(), Some((4, 4)));
        assert!(page_range(Some("3-2")).is_err() && page_range(Some("0")).is_err());
        assert_eq!(disposition_name("in\"voice\\.pdf"), "invoice.pdf");
    }
}
