//! Sending, threads, messages, `wait`, `triage` and `quarantine` ([CLI and setup §18]).
//!
//! [CLI and setup §18]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#18-other-client-side-behaviour

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use super::{Obj, flat, metadata, paging, q_opt, read_file, read_text, time};
use crate::cli::{
    BodyArgs, ForwardArgs, LabelArgs, MessageArg, MessagesCmd, QuarantineCmd, ReplyArgs,
    ResolveArgs, SendArgs, ThreadsCmd, TriageCmd, WaitArgs,
};
use crate::http::{Idem, seg, send_key};
use crate::output::{Col, col, terminal_safe};
use crate::{CliError, Ctx, Output, resolve};

/// Files up to this size are sent inline; larger ones need an upload (M33).
const INLINE_MAX: usize = 1_048_576;

/// A recipient: `Name <addr>` as an object, a bare address as a string.
pub fn recipient(s: &str) -> Value {
    let s = s.trim();
    match (s.rfind('<'), s.ends_with('>')) {
        (Some(i), true) => {
            let name = s[..i].trim().trim_matches('"').trim();
            let address = &s[i + 1..s.len() - 1];
            if name.is_empty() {
                json!(address)
            } else {
                json!({ "name": name, "address": address })
            }
        }
        _ => json!(s),
    }
}

fn recipients(items: &[String]) -> Vec<Value> {
    flat(items).iter().map(|r| recipient(r)).collect()
}

/// The content type of a file name, by its extension.
pub fn content_type(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "ics" => "text/calendar",
        "eml" => "message/rfc822",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

/// Text, HTML and attachments from the body flags.
fn body_fields(ctx: &Ctx, b: &BodyArgs, mut o: Obj, need_body: bool) -> Result<Obj, CliError> {
    let text = match (&b.text, &b.text_file) {
        (Some(t), _) => Some(t.clone()),
        (None, Some(f)) => Some(read_text(ctx, f)?),
        _ => None,
    };
    let html = match (&b.html, &b.html_file) {
        (Some(h), _) => Some(h.clone()),
        (None, Some(f)) => Some(read_text(ctx, f)?),
        _ => None,
    };
    if need_body && text.is_none() && html.is_none() {
        return Err(CliError::usage(
            "Pass --text, --text-file, --html or --html-file.",
        ));
    }
    o = o.opt("text", text).opt("html", html);
    let mut atts = Vec::new();
    for p in &b.attach {
        let bytes = read_file(ctx, p)?;
        if bytes.len() > INLINE_MAX {
            return Err(CliError::usage(format!(
                "{p} is over 1 MiB; large attachments go through uploads, which this deployment's CLI does not support yet."
            )));
        }
        let name = std::path::Path::new(p)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.clone());
        atts.push(json!({
            "filename": name,
            "content_type": content_type(&name),
            "content_base64": STANDARD.encode(&bytes),
        }));
    }
    if !atts.is_empty() {
        o = o.set("attachments", atts);
    }
    Ok(o)
}

/// `X-Name: value` flags as the `headers` object.
fn headers(items: &[String]) -> Result<Option<Value>, CliError> {
    if items.is_empty() {
        return Ok(None);
    }
    let mut m = serde_json::Map::new();
    for h in items {
        let (k, v) = h
            .split_once(':')
            .ok_or_else(|| CliError::usage(format!("--header takes 'Name: value', not {h:?}.")))?;
        m.insert(k.trim().to_owned(), Value::String(v.trim().to_owned()));
    }
    Ok(Some(Value::Object(m)))
}

/// A send's result: the queued message, with `id`, `status` and `deduplicated` on one line for people.
fn sent(doc: Value) -> Output {
    let line = format!(
        "{{ \"id\": {}, \"status\": {}, \"deduplicated\": {} }}\n",
        doc["id"], doc["status"], doc["deduplicated"]
    );
    let id = doc["id"].as_str().unwrap_or("").to_owned();
    Output::object(doc).human(line).quiet(format!("{id}\n"))
}

/// POSTs a send (or its dry run) with the send's idempotency key.
async fn post_send(
    ctx: &Ctx,
    path: &str,
    body: Value,
    key: Option<&str>,
    dry_run: bool,
) -> Result<Output, CliError> {
    let api = ctx.api()?;
    if dry_run {
        let r = api
            .call(
                ctx,
                "POST",
                path,
                &[("dry_run", "true".to_owned())],
                Some(&body),
                Idem::None,
            )
            .await?;
        return Ok(Output::object(r));
    }
    let idem = send_key(ctx, key)?;
    Ok(sent(
        api.call(ctx, "POST", path, &[], Some(&body), idem).await?,
    ))
}

pub async fn send(ctx: &Ctx, a: SendArgs) -> Result<Output, CliError> {
    let idn = resolve::identity(ctx).await?;
    let unsubscribe = a.unsubscribe_url.map(|u| {
        Obj::new()
            .set("url", u)
            .opt("mailto", a.unsubscribe_mailto)
            .value()
    });
    let consent = match (a.consent_basis, a.consent_recorded_at) {
        (Some(b), Some(t)) => Some(json!({ "basis": b, "recorded_at": time(&t)? })),
        _ => None,
    };
    let mut body = Obj::new()
        .set("to", recipients(&a.to))
        .set("subject", a.subject)
        .opt("kind", a.kind)
        .opt("thread_id", a.thread)
        .opt("from_address", a.from_address)
        .list("labels", &a.label)
        .opt("headers", headers(&a.header)?)
        .opt("metadata", metadata(&a.metadata)?)
        .opt("unsubscribe", unsubscribe)
        .opt("consent", consent);
    if !a.cc.is_empty() {
        body = body.set("cc", recipients(&a.cc));
    }
    if !a.bcc.is_empty() {
        body = body.set("bcc", recipients(&a.bcc));
    }
    let body = body_fields(ctx, &a.body, body, true)?.value();
    post_send(
        ctx,
        &format!("/v1/identities/{}/messages", seg(&idn)),
        body,
        a.body.idempotency_key.as_deref(),
        a.dry_run,
    )
    .await
}

pub async fn reply(ctx: &Ctx, a: ReplyArgs, which: &str) -> Result<Output, CliError> {
    let idn = resolve::identity(ctx).await?;
    let body = body_fields(ctx, &a.body, Obj::new().opt("kind", a.kind), true)?.value();
    post_send(
        ctx,
        &format!(
            "/v1/identities/{}/messages/{}/{which}",
            seg(&idn),
            seg(&a.message)
        ),
        body,
        a.body.idempotency_key.as_deref(),
        a.dry_run,
    )
    .await
}

pub async fn forward(ctx: &Ctx, a: ForwardArgs) -> Result<Output, CliError> {
    let idn = resolve::identity(ctx).await?;
    let mut body = Obj::new().set("to", recipients(&a.to)).opt("text", a.text);
    if !a.cc.is_empty() {
        body = body.set("cc", recipients(&a.cc));
    }
    if a.no_attachments {
        body = body.set("include_attachments", false);
    }
    post_send(
        ctx,
        &format!(
            "/v1/identities/{}/messages/{}/forward",
            seg(&idn),
            seg(&a.message)
        ),
        body.value(),
        a.idempotency_key.as_deref(),
        a.dry_run,
    )
    .await
}

pub async fn cancel(ctx: &Ctx, a: MessageArg) -> Result<Output, CliError> {
    let idn = resolve::identity(ctx).await?;
    Ok(Output::object(
        ctx.api()?
            .call(
                ctx,
                "POST",
                &format!(
                    "/v1/identities/{}/messages/{}/cancel",
                    seg(&idn),
                    seg(&a.message)
                ),
                &[],
                None,
                Idem::Generated,
            )
            .await?,
    ))
}

pub async fn resolve(ctx: &Ctx, a: ResolveArgs) -> Result<Output, CliError> {
    let idn = resolve::identity(ctx).await?;
    Ok(Output::object(
        ctx.api()?
            .call(
                ctx,
                "POST",
                &format!(
                    "/v1/identities/{}/messages/{}/resolve",
                    seg(&idn),
                    seg(&a.message)
                ),
                &[],
                Some(&json!({ "outcome": a.outcome })),
                Idem::Generated,
            )
            .await?,
    ))
}

const THREAD_COLS: &[Col] = &[
    col("ID", "id", 30),
    col("LAST", "last_at", 20),
    col("SUBJECT", "subject", 40),
    col("MSGS", "message_count", 4),
    col("UNREAD", "unread_count", 6),
    col("CATEGORY", "category", 18),
    col("NEEDS_REPLY", "needs_reply", 11),
];

const MESSAGE_COLS: &[Col] = &[
    col("ID", "id", 30),
    col("DATE", "received_at", 20),
    col("DIR", "direction", 8),
    col("FROM", "from.address", 32),
    col("SUBJECT", "subject", 40),
    col("STATUS", "status", 10),
];

/// The `labels_add`, `labels_remove`, `read` and `archived` of a label change.
fn label_body(l: &LabelArgs, archive_allowed: bool) -> Result<Value, CliError> {
    let mut o = Obj::new()
        .list("labels_add", &l.add)
        .list("labels_remove", &l.remove);
    if l.read || l.unread {
        o = o.set("read", l.read);
    }
    if l.archive || l.unarchive {
        if !archive_allowed {
            return Err(CliError::usage(
                "--archive and --unarchive apply to threads.",
            ));
        }
        o = o.set("archived", l.archive);
    }
    if o.is_empty() {
        return Err(CliError::usage(
            "Nothing to change: pass --add, --remove, --read or --unread.",
        ));
    }
    Ok(o.value())
}

pub async fn threads(ctx: &Ctx, c: ThreadsCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    let idn = resolve::identity(ctx).await?;
    let base = format!("/v1/identities/{}/threads", seg(&idn));
    match c {
        ThreadsCmd::List {
            label,
            category,
            needs_reply_gte,
            unread,
            direction,
            after,
            before,
            archived,
            paging: p,
        } => {
            let mut q = Vec::new();
            q_opt(&mut q, "label", label);
            q_opt(&mut q, "category", category);
            q_opt(&mut q, "needs_reply_gte", needs_reply_gte);
            if unread {
                q.push(("is_unread", "true".into()));
            }
            q_opt(&mut q, "direction", direction);
            if let Some(t) = after {
                q.push(("after", time(&t)?));
            }
            if let Some(t) = before {
                q.push(("before", time(&t)?));
            }
            if archived {
                q.push(("archived", "true".into()));
            }
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(ctx, &base, &q, p.all).await?,
                THREAD_COLS,
            ))
        }
        ThreadsCmd::Get {
            thread,
            messages_limit,
            include,
        } => {
            let mut q = Vec::new();
            q_opt(&mut q, "messages_limit", messages_limit);
            q_opt(&mut q, "include", include);
            Ok(Output::object(
                api.get(ctx, &format!("{base}/{}", seg(&thread)), &q)
                    .await?,
            ))
        }
        ThreadsCmd::Label { thread, labels } => {
            let body = label_body(&labels, true)?;
            Ok(Output::object(
                api.call(
                    ctx,
                    "PATCH",
                    &format!("{base}/{}", seg(&thread)),
                    &[],
                    Some(&body),
                    Idem::None,
                )
                .await?,
            ))
        }
        ThreadsCmd::Hold {
            thread,
            reason,
            until,
        } => {
            let body = Obj::new()
                .set("reason", reason)
                .opt("until", until.map(|u| time(&u)).transpose()?)
                .value();
            Ok(Output::object(
                api.call(
                    ctx,
                    "POST",
                    &format!("{base}/{}/hold", seg(&thread)),
                    &[],
                    Some(&body),
                    Idem::Generated,
                )
                .await?,
            ))
        }
        ThreadsCmd::Unhold { thread } => Ok(Output::object(
            api.call(
                ctx,
                "DELETE",
                &format!("{base}/{}/hold", seg(&thread)),
                &[],
                None,
                Idem::None,
            )
            .await?,
        )),
    }
}

pub async fn messages(ctx: &Ctx, c: MessagesCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    let idn = resolve::identity(ctx).await?;
    let base = format!("/v1/identities/{}/messages", seg(&idn));
    match c {
        MessagesCmd::List {
            thread,
            direction,
            status,
            label,
            after,
            before,
            paging: p,
        } => {
            let mut q = Vec::new();
            q_opt(&mut q, "thread_id", thread);
            q_opt(&mut q, "direction", direction);
            q_opt(&mut q, "status", status);
            q_opt(&mut q, "label", label);
            if let Some(t) = after {
                q.push(("after", time(&t)?));
            }
            if let Some(t) = before {
                q.push(("before", time(&t)?));
            }
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(ctx, &base, &q, p.all).await?,
                MESSAGE_COLS,
            ))
        }
        MessagesCmd::Get { message, include } => {
            let mut q = Vec::new();
            q_opt(&mut q, "include", include);
            Ok(Output::object(
                api.get(ctx, &format!("{base}/{}", seg(&message)), &q)
                    .await?,
            ))
        }
        MessagesCmd::Raw { message, out } => {
            let r = api
                .send(
                    ctx,
                    "GET",
                    &format!("{base}/{}/raw", seg(&message)),
                    &[],
                    None,
                    Idem::None,
                )
                .await?;
            super::write_bytes(ctx, out.as_deref(), &r.body)
        }
        MessagesCmd::Attachment {
            message,
            attachment,
            out,
        } => {
            let r = api
                .send(
                    ctx,
                    "GET",
                    &format!("{base}/{}/attachments/{}", seg(&message), seg(&attachment)),
                    &[],
                    None,
                    Idem::None,
                )
                .await?;
            super::write_bytes(ctx, out.as_deref(), &r.body)
        }
        MessagesCmd::AttachmentText {
            message,
            attachment,
            pages,
        } => {
            let mut q = Vec::new();
            q_opt(&mut q, "pages", pages);
            let t = api
                .get(
                    ctx,
                    &format!(
                        "{base}/{}/attachments/{}/text",
                        seg(&message),
                        seg(&attachment)
                    ),
                    &q,
                )
                .await?;
            let human = t["pages"]
                .as_array()
                .map(|ps| {
                    ps.iter()
                        .map(|p| {
                            format!(
                                "--- page {} ---\n{}\n",
                                p["page"],
                                terminal_safe(p["text"].as_str().unwrap_or(""))
                            )
                        })
                        .collect::<String>()
                })
                .unwrap_or_else(|| crate::output::pretty(&t));
            Ok(Output::object(t).human(human))
        }
        MessagesCmd::Label { message, labels } => {
            let body = label_body(&labels, false)?;
            Ok(Output::object(
                api.call(
                    ctx,
                    "PATCH",
                    &format!("{base}/{}", seg(&message)),
                    &[],
                    Some(&body),
                    Idem::None,
                )
                .await?,
            ))
        }
        MessagesCmd::Delete { message } => {
            ctx.confirm(&format!("Erase {message}? This cannot be undone."))?;
            Ok(Output::object(
                api.call(
                    ctx,
                    "DELETE",
                    &format!("{base}/{}", seg(&message)),
                    &[],
                    None,
                    Idem::None,
                )
                .await?,
            ))
        }
        MessagesCmd::Related { message, limit } => {
            let mut q = Vec::new();
            q_opt(&mut q, "limit", limit);
            Ok(Output::object(
                api.get(ctx, &format!("{base}/{}/related", seg(&message)), &q)
                    .await?,
            ))
        }
    }
}

/// `idvy wait`: the message, or `timed out` with exit 13; for a verification, the code and link on
/// their own lines.
pub async fn wait(ctx: &Ctx, a: WaitArgs) -> Result<Output, CliError> {
    let idn = resolve::identity(ctx).await?;
    let mut q = Vec::new();
    q_opt(&mut q, "from", a.from);
    q_opt(&mut q, "subject_contains", a.subject_contains);
    q_opt(&mut q, "thread_id", a.thread);
    q_opt(&mut q, "kind", a.kind);
    if let Some(s) = a.since {
        q.push(("since", time(&s)?));
    }
    q_opt(&mut q, "timeout", a.timeout);
    let r = ctx
        .api()?
        .get(ctx, &format!("/v1/identities/{}/wait", seg(&idn)), &q)
        .await?;
    if r["timed_out"] == true {
        return Ok(Output {
            human: Some("timed out\n".into()),
            quiet: Some(String::new()),
            exit: 13,
            ..Output::object(r)
        });
    }
    let v = &r["verification"];
    let lines: String = ["code", "link"]
        .iter()
        .filter_map(|k| v[*k].as_str().map(|s| format!("{}\n", terminal_safe(s))))
        .collect();
    let human = format!("{}{lines}", crate::output::pretty(&r["message"]));
    let quiet = if lines.is_empty() {
        format!("{}\n", r["message"]["id"].as_str().unwrap_or(""))
    } else {
        lines
    };
    Ok(Output::object(r).human(human).quiet(quiet))
}

pub async fn triage(ctx: &Ctx, c: TriageCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    let idn = resolve::identity(ctx).await?;
    match c {
        TriageCmd::List {
            category,
            needs_reply_gte,
            limit,
        } => {
            let mut q = Vec::new();
            q_opt(&mut q, "category", category);
            q_opt(&mut q, "needs_reply_gte", needs_reply_gte);
            q_opt(&mut q, "limit", limit);
            Ok(Output::list(
                api.get(ctx, &format!("/v1/identities/{}/threads", seg(&idn)), &q)
                    .await?,
                THREAD_COLS,
            ))
        }
        TriageCmd::Rerun { message } => Ok(Output::object(
            api.call(
                ctx,
                "POST",
                &format!(
                    "/v1/identities/{}/messages/{}/triage",
                    seg(&idn),
                    seg(&message)
                ),
                &[],
                None,
                Idem::Generated,
            )
            .await?,
        )),
    }
}

pub async fn quarantine(ctx: &Ctx, c: QuarantineCmd) -> Result<Output, CliError> {
    let api = ctx.api()?;
    let idn = resolve::identity(ctx).await?;
    match c {
        QuarantineCmd::List { paging: p } => {
            let mut q = Vec::new();
            paging(&p, &mut q);
            Ok(Output::list(
                api.list(
                    ctx,
                    &format!("/v1/identities/{}/quarantine", seg(&idn)),
                    &q,
                    p.all,
                )
                .await?,
                MESSAGE_COLS,
            ))
        }
        QuarantineCmd::Release { message, reason } => Ok(Output::object(
            api.call(
                ctx,
                "POST",
                &format!(
                    "/v1/identities/{}/messages/{}/release",
                    seg(&idn),
                    seg(&message)
                ),
                &[],
                Some(&json!({ "reason": reason })),
                Idem::Generated,
            )
            .await?,
        )),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn recipients() {
        assert_eq!(super::recipient("a@x.example"), json!("a@x.example"));
        assert_eq!(
            super::recipient("\"Sam Patel\" <sam@x.example>"),
            json!({ "name": "Sam Patel", "address": "sam@x.example" })
        );
        assert_eq!(super::recipient("<b@x.example>"), json!("b@x.example"));
    }

    #[test]
    fn content_types() {
        assert_eq!(super::content_type("INV-88213.PDF"), "application/pdf");
        assert_eq!(super::content_type("blob"), "application/octet-stream");
    }
}
