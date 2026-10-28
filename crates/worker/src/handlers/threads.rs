//! Threads ([Inbound › Read path and release], [Threading § 3.4]): list, get with a page of messages,
//! and label, read-state and archive changes. Built from visible mail only.
//!
//! [Inbound › Read path and release]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md
//! [Threading § 3.4]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/threading.md

use pylota_identity_platform::Platform;
use serde_json::Value;

use super::identity_in_scope;
use super::messages::{
    boolean, boundary, fill_headers, include, labels, mailbox, only_fields, page, review, time,
    valid_label,
};
use crate::ctx::{Ctx, Reply};
use crate::errors::ApiError;
use crate::mailbox::read::{ReadRequest, ThreadFilters};

const LIST_FILTERS: &[&str] = &[
    "label",
    "category",
    "needs_reply_gte",
    "is_unread",
    "direction",
    "after",
    "before",
    "archived",
];

pub async fn list<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let limit = c.limit()?;
    let cursor = boundary(c, LIST_FILTERS).await?;
    let needs_reply_gte = match c.query("needs_reply_gte") {
        None => None,
        Some(v) => match v.parse::<f64>() {
            Ok(n) if (0.0..=1.0).contains(&n) => Some(n),
            _ => return Err(ApiError::invalid("needs_reply_gte", "0 to 1.")),
        },
    };
    let direction = match c.query("direction") {
        None => None,
        Some(d @ ("inbound" | "outbound")) => Some(d.to_owned()),
        Some(_) => return Err(ApiError::invalid("direction", "inbound or outbound.")),
    };
    let label = match c.query("label") {
        Some(l) if !valid_label(l) => return Err(ApiError::invalid("label", "Not a label.")),
        l => l.map(str::to_owned),
    };
    let filters = ThreadFilters {
        label,
        category: c.query("category").map(str::to_owned),
        needs_reply_gte,
        is_unread: boolean(c, "is_unread")?,
        direction,
        after_ms: time(c, "after")?,
        before_ms: time(c, "before")?,
        archived: boolean(c, "archived")?.unwrap_or(false),
    };
    let out = mailbox(
        c,
        &idn,
        ReadRequest::ListThreads {
            filters,
            limit,
            cursor,
        },
    )
    .await?;
    page(c, out, limit, LIST_FILTERS).await
}

pub async fn get<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), false).await?;
    let limit = match c.query("messages_limit") {
        None => 20,
        Some(l) => match l.parse::<u32>() {
            Ok(n) if (1..=100).contains(&n) => n,
            _ => return Err(ApiError::invalid("messages_limit", "1 to 100.")),
        },
    };
    const F: &[&str] = &["messages_limit", "include"];
    let cursor = boundary(c, F).await?;
    let out = mailbox(
        c,
        &idn,
        ReadRequest::GetThread {
            thread_id: c.route.param("thread_id").to_owned(),
            messages_limit: limit,
            cursor,
            include: include(c)?,
            review: review(c),
        },
    )
    .await?;
    let mut thread = out["thread"].clone();
    // Messages are oldest first, so the page continues after the last one.
    let paged = page(c, out, limit, F).await?;
    let body = paged.body.unwrap_or(Value::Null);
    let mut messages = body["data"].as_array().cloned().unwrap_or_default();
    for m in &mut messages {
        fill_headers(c, m).await?;
    }
    thread["messages"] = Value::Array(messages);
    thread["next_cursor"] = body["next_cursor"].clone();
    Ok(Reply::json(200, thread))
}

pub async fn update<P: Platform>(c: &Ctx<P>) -> Result<Reply, ApiError> {
    let (idn, _) = identity_in_scope(c, c.route.param("identity_id"), true).await?;
    let body = c.json()?;
    only_fields(&body, &["labels_add", "labels_remove", "read", "archived"])?;
    let flag = |name: &str| match &body[name] {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(*b)),
        _ => Err(ApiError::invalid(name, "true or false.")),
    };
    let out = mailbox(
        c,
        &idn,
        ReadRequest::UpdateThread {
            thread_id: c.route.param("thread_id").to_owned(),
            labels_add: labels(&body, "labels_add")?,
            labels_remove: labels(&body, "labels_remove")?,
            read: flag("read")?,
            archived: flag("archived")?,
        },
    )
    .await?;
    Ok(Reply::json(200, out))
}
