//! `search` and `ask` ([CLI and setup §17, §18]).
//!
//! [CLI and setup §17, §18]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#17-ask

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};

use super::{Obj, csv, time};
use crate::cli::{AskArgs, SearchArgs};
use crate::http::{Idem, seg, user_agent};
use crate::output::{Col, col, table, terminal_safe};
use crate::{CliError, Ctx, Mode, Output, config, resolve};

const HIT_COLS: &[Col] = &[
    col("DATE", "date", 20),
    col("DIR", "direction", 8),
    col("FROM", "from.address", 30),
    col("SUBJECT", "subject", 40),
    col("SCORE", "score", 6),
    col("WHY", "why", 30),
];

const TENANT_HIT_COLS: &[Col] = &[
    col("IDENTITY", "identity_id", 30),
    col("DATE", "date", 20),
    col("FROM", "from.address", 30),
    col("SUBJECT", "subject", 40),
    col("SCORE", "score", 6),
];

/// The search's path: the identity's, or the tenant's with `--tenant`.
async fn scope(ctx: &Ctx) -> Result<(String, bool), CliError> {
    if ctx.global.tenant.is_some() && ctx.global.identity.is_none() {
        let t = resolve::tenant(ctx).await?;
        return Ok((format!("/v1/tenants/{}/search", seg(&t)), true));
    }
    let i = resolve::identity(ctx).await?;
    Ok((format!("/v1/identities/{}/search", seg(&i)), false))
}

pub async fn search(ctx: &Ctx, a: SearchArgs) -> Result<Output, CliError> {
    let (path, tenant) = scope(ctx).await?;
    let mut filters = Obj::new()
        .opt("direction", a.direction)
        .list("labels", &a.label);
    if let Some(t) = a.after {
        filters = filters.set("after", time(&t)?);
    }
    if let Some(t) = a.before {
        filters = filters.set("before", time(&t)?);
    }
    let mut body = Obj::new()
        .set("q", a.query)
        .opt("mode", a.mode.clone())
        .opt("group_by", a.group_by)
        .opt("limit", a.limit)
        .opt("snippet_chars", a.snippet_chars)
        .flag("include_quarantined", a.include_quarantined)
        .flag("require_mode", a.require_mode)
        .opt("cursor", a.cursor);
    if a.no_facets {
        body = body.set("facets", false);
    }
    if !filters.is_empty() {
        body = body.set("filters", filters.value());
    }
    if let Some(ids) = a.identity_ids {
        body = body.set("identity_ids", csv(&ids));
    }
    let r = ctx
        .api()?
        .call(ctx, "POST", &path, &[], Some(&body.value()), Idem::None)
        .await?;
    if a.mode.as_deref() == Some("agentic") {
        let human = render_answer(&r);
        return Ok(Output::object(r).human(human));
    }
    let hits = r["hits"].as_array().cloned().unwrap_or_default();
    let mut human = table(&hits, if tenant { TENANT_HIT_COLS } else { HIT_COLS });
    let coverage = r["semantic_coverage"].as_f64().unwrap_or(1.0);
    if r["degraded"] == true || coverage < 0.98 {
        human.push_str(&format!(
            "warning: {}semantic coverage {:.0}%\n",
            if r["degraded"] == true {
                "degraded to keyword search; "
            } else {
                ""
            },
            coverage * 100.0
        ));
    }
    if let Some(c) = r["next_cursor"].as_str() {
        human.push_str(&format!("next page: --cursor {c}\n"));
    }
    let quiet = hits
        .iter()
        .filter_map(|h| h["message_id"].as_str().map(|s| format!("{s}\n")))
        .collect::<String>();
    Ok(Output::object(r).human(human).quiet(quiet))
}

/// An agentic answer for people: citations renumbered in order of first use and listed with date,
/// sender and subject; every string neutralised.
pub fn render_answer(r: &Value) -> String {
    let status = r["status"].as_str().unwrap_or("");
    let evidence: BTreeMap<String, &Value> = r["evidence"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["message_id"].as_str().map(|id| (id.to_owned(), e)))
        .collect();
    let mut order: Vec<String> = Vec::new();
    let mut cite = |id: &str| -> usize {
        if let Some(i) = order.iter().position(|x| x == id) {
            i + 1
        } else {
            order.push(id.to_owned());
            order.len()
        }
    };
    let mut text = String::new();
    match r["answer"]["sentences"]
        .as_array()
        .filter(|s| !s.is_empty())
    {
        Some(sentences) => {
            for s in sentences {
                text.push_str(&terminal_safe(s["text"].as_str().unwrap_or("")));
                for c in s["citations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    text.push_str(&format!("[{}]", cite(c)));
                }
                text.push(' ');
            }
        }
        None => text.push_str(&terminal_safe(r["answer"]["text"].as_str().unwrap_or(""))),
    }
    let mut out = String::new();
    match status {
        "insufficient_evidence" => out.push_str("Not enough evidence to answer.\n"),
        "budget_exhausted" if !text.trim().is_empty() => {
            out.push_str(&format!("(partial) {}\n", text.trim()));
        }
        _ => out.push_str(&format!("{}\n", text.trim())),
    }
    if order.is_empty() && status == "insufficient_evidence" {
        order = evidence.keys().take(3).cloned().collect();
    }
    if !order.is_empty() {
        out.push('\n');
    }
    for (i, id) in order.iter().enumerate() {
        let e = evidence.get(id).copied().unwrap_or(&Value::Null);
        let from = &e["from"];
        out.push_str(&format!(
            "[{}] {}  {}  {} <{}>  \"{}\"\n",
            i + 1,
            id,
            e["date"].as_str().unwrap_or("").get(..10).unwrap_or(""),
            terminal_safe(from["name"].as_str().unwrap_or("")),
            terminal_safe(from["address"].as_str().unwrap_or("")),
            terminal_safe(e["subject"].as_str().unwrap_or("")),
        ));
    }
    let steps = r["trace"].as_array().map_or(0, Vec::len);
    let ms = r["usage"]["ms"]
        .as_f64()
        .or_else(|| r["usage"]["elapsed_ms"].as_f64());
    out.push_str(&format!(
        "\n{status}{}{}{}\n",
        r["answer"]["confidence"]
            .as_f64()
            .map(|c| format!(" · confidence {c:.2}"))
            .unwrap_or_default(),
        format_args!(" · {steps} steps"),
        ms.map(|m| format!(" · {:.1} s", m / 1000.0))
            .unwrap_or_default(),
    ));
    out
}

/// One progress line for a `step` event.
fn step_line(d: &Value) -> String {
    let mut s = format!(
        "⋯ step {}  {}",
        d["step"],
        d["action"].as_str().unwrap_or("")
    );
    if let Some(q) = d["q"].as_str() {
        s.push_str(&format!(" \"{}\"", terminal_safe(q)));
    }
    if let Some(m) = d["mode"].as_str() {
        s.push_str(&format!(" ({m})"));
    }
    if let Some(h) = d["hits"].as_u64() {
        s.push_str(&format!(" · {h} hits"));
    }
    if let Some(t) = d["thread_id"].as_str() {
        s.push_str(&format!(" {t}"));
    }
    if let Some(ms) = d["ms"].as_u64() {
        s.push_str(&format!(" · {ms} ms"));
    }
    s
}

/// `idvy ask`: an agentic search, streamed by default.
pub async fn ask(ctx: &Ctx, a: AskArgs) -> Result<Output, CliError> {
    let (path, _) = scope(ctx).await?;
    let budget = Obj::new()
        .opt("max_steps", a.max_steps)
        .opt("max_seconds", a.max_seconds);
    let stream = !a.no_stream;
    let mut body = Obj::new()
        .set("q", a.question)
        .set("mode", "agentic")
        .flag("include_quarantined", a.include_quarantined)
        .set("stream", stream);
    if !budget.is_empty() {
        body = body.set("budget", budget.value());
    }
    let body = body.value();
    if !stream {
        let r = ctx
            .api()?
            .call(ctx, "POST", &path, &[], Some(&body), Idem::None)
            .await?;
        let human = render_answer(&r);
        return Ok(Output::object(r).human(human));
    }
    let resolved = config::resolve(ctx)?;
    let (Some(url), Some(key)) = (resolved.url, resolved.key) else {
        ctx.api()?;
        unreachable!("api() fails without a URL or key");
    };
    let mut resp = ctx
        .http
        .post(format!("{}{path}", url.value))
        .header("authorization", format!("Bearer {}", key.value))
        .header("accept", "text/event-stream")
        .header("content-type", "application/json")
        .header("user-agent", user_agent())
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| CliError::Network {
            message: e.to_string(),
            idempotency_key: None,
        })?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.unwrap_or_default();
        return Err(pylota_identity::Error::from_response(status, &bytes).into());
    }
    let ndjson = ctx.mode == Mode::Json && a.stream;
    let show_steps = a.show_trace || (ctx.stderr_tty && ctx.mode == Mode::Human);
    let mut parser = crate::sse::Parser::default();
    loop {
        let chunk = tokio::time::timeout(Duration::from_secs(30), resp.chunk())
            .await
            .map_err(|_| CliError::Network {
                message: "no data for 30 s".into(),
                idempotency_key: None,
            })?
            .map_err(|e| CliError::Network {
                message: e.to_string(),
                idempotency_key: None,
            })?;
        let Some(chunk) = chunk else {
            return Err(CliError::Network {
                message: "the stream ended before its done event".into(),
                idempotency_key: None,
            });
        };
        for e in parser.push(&chunk) {
            if ndjson {
                let id = e.id.as_deref().and_then(|i| i.parse::<u64>().ok());
                ctx.print(&format!(
                    "{}\n",
                    json!({ "event": e.event, "id": id, "data": e.data })
                ));
            } else if e.event == "step" && show_steps {
                ctx.note(&step_line(&e.data));
            }
            if e.event == "done" {
                if let Some(err) = e.data.get("error").filter(|v| v.is_object()) {
                    let env = json!({ "error": err });
                    let status = err["status"].as_u64().unwrap_or(500) as u16;
                    return Err(pylota_identity::Error::from_response(
                        status,
                        env.to_string().as_bytes(),
                    )
                    .into());
                }
                if ndjson {
                    return Ok(Output {
                        doc: Value::Null,
                        human: Some(String::new()),
                        quiet: Some(String::new()),
                        ..Output::default()
                    }
                    .streamed());
                }
                let human = render_answer(&e.data);
                return Ok(Output::object(e.data).human(human));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// Citations are renumbered in order of first use and listed from the evidence; hostile text is
    /// neutralised.
    #[test]
    fn answer_rendering() {
        let r = json!({
            "status": "answered",
            "answer": { "text": "x", "confidence": 0.86, "sentences": [
                { "text": "Yes.", "citations": ["msg_B", "msg_A"] },
                { "text": "Photos \u{1b}[2Jsent.", "citations": ["msg_B"] },
            ]},
            "evidence": [
                { "message_id": "msg_A", "date": "2026-09-28T10:00:00Z", "from": { "name": "Acme", "address": "c@acme.example" }, "subject": "Photos" },
                { "message_id": "msg_B", "date": "2026-10-02T10:00:00Z", "from": { "name": "Admiral", "address": "claims@admiral.example" }, "subject": "Decision" },
            ],
            "trace": [{}, {}, {}],
        });
        let s = super::render_answer(&r);
        assert!(
            s.starts_with("Yes.[1][2] Photos \u{fffd}[2Jsent.[1]"),
            "{s}"
        );
        assert!(
            s.contains("[1] msg_B  2026-10-02  Admiral <claims@admiral.example>  \"Decision\"")
        );
        assert!(s.contains("answered · confidence 0.86 · 3 steps"));
    }
}
