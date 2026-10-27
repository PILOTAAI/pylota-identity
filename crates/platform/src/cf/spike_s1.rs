//! Spike S1 (build plan M1): every binding the design relies on, called from Rust through the entry glue
//! of [`export_worker!`](crate::export_worker). Not shipped: `crates/worker` never enables `spike-s1`,
//! and M4 deletes this module once its findings are in the platform traits.
//!
//! Every route needs the header `x-spike-key` equal to the secret `SPIKE_KEY`. The Worker keeps its log
//! in one Durable Object (`SpikeObject`, SQLite), so a run can be read back with `GET /log`.

use serde_json::{Value, json};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;

use super::externs::transaction_sync;
use super::glue::{
    CfObjectState, CfPlatform, EmailIn, EmailOutcome, HttpRequestIn, HttpResponseOut, Incoming,
    ObjectApp, ScheduledIn, WorkerApp,
};

pub struct SpikeApp {
    platform: CfPlatform,
}

impl WorkerApp for SpikeApp {
    fn new(platform: CfPlatform) -> Self {
        Self { platform }
    }

    async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
        let key = self
            .platform
            .env()
            .secret("SPIKE_KEY")
            .map(|s| s.to_string());
        let given = header(&req, "x-spike-key");
        if key.is_err() || given.as_deref() != key.as_ref().ok().map(String::as_str) {
            return reply(403, &json!({ "error": "forbidden" }));
        }
        let path = req
            .url
            .split("://")
            .nth(1)
            .and_then(|r| r.find('/').map(|i| &r[i..]));
        let path = path
            .unwrap_or("/")
            .split('?')
            .next()
            .unwrap_or("/")
            .to_owned();
        let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        let result = match (req.method.as_str(), path.as_str()) {
            ("POST", "/send/structured") => self.send_structured(&body).await,
            ("POST", "/send/raw") => self.send_raw(&body).await,
            ("POST", "/queue/send") => self.queue_send(&body).await,
            ("POST", "/d1/batch") => self.d1_batch().await,
            ("POST", "/do/txn") | ("POST", "/do/alarm") | ("GET", "/log") => {
                self.object(&req.method, &path, &req.body).await
            }
            _ => Err(format!("no route {} {path}", req.method)),
        };
        match result {
            Ok(value) => reply(200, &value),
            Err(error) => reply(500, &json!({ "error": error })),
        }
    }

    async fn email(&self, msg: EmailIn) -> EmailOutcome {
        let local = msg.to.split('@').next().unwrap_or_default().to_owned();
        let pick = |names: &[&str]| -> Value {
            let mut out = serde_json::Map::new();
            for (name, value) in &msg.headers {
                if names.contains(&name.as_str()) {
                    out.insert(name.clone(), Value::String(value.clone()));
                }
            }
            Value::Object(out)
        };
        let entry = json!({
            "from": msg.from,
            "to": msg.to,
            "raw_size": msg.raw_size,
            "raw_len": msg.raw.len(),
            "headers": pick(&["message-id", "subject", "from", "to", "content-type",
                              "authentication-results", "dkim-signature", "in-reply-to",
                              "references", "auto-submitted", "reply-to", "x-spike"]),
            "header_names": msg.headers.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
            "raw_head": String::from_utf8_lossy(&msg.raw[..msg.raw.len().min(60_000)]),
            "local": local,
        });
        let _ = self.append("email", &entry).await;
        match local.as_str() {
            "reject" => EmailOutcome::Reject {
                smtp: "spike s1 reject".to_owned(),
            },
            "tempfail" => EmailOutcome::TempFail,
            _ => EmailOutcome::Accept,
        }
    }

    async fn queue(&self, queue: String, batch: Vec<Incoming>) {
        for message in &batch {
            let body: Value = serde_json::from_str(&message.body_json).unwrap_or(Value::Null);
            let retries = body["retries"].as_u64().unwrap_or(0) as u32;
            let _ = self
                .append(
                    "queue",
                    &json!({
                        "queue": queue,
                        "id": message.id,
                        "timestamp_ms": message.timestamp_ms,
                        "attempts": message.attempts,
                        "seen_ms": js_sys::Date::now(),
                        "body": body,
                    }),
                )
                .await;
            if message.attempts <= retries {
                message.retry(Some(body["retry_delay"].as_u64().unwrap_or(1) as u32));
            } else {
                message.ack();
            }
        }
    }

    async fn scheduled(&self, ev: ScheduledIn) {
        let _ = self
            .append(
                "scheduled",
                &json!({ "cron": ev.cron, "at": ev.scheduled_time_ms }),
            )
            .await;
    }
}

impl SpikeApp {
    /// The structured `send()` with every field the design uses.
    async fn send_structured(&self, body: &Value) -> Result<Value, String> {
        let from = str_field(body, "from")?;
        let to: Vec<String> = strings(&body["to"]);
        let builder = worker::SendEmailBuilder::new_with_str_and_slice(
            &from,
            &to,
            &format!("S1 structured {}", js_sys::Date::now()),
        );
        builder.set_text("Plain body of the S1 structured send.");
        builder.set_html("<p>HTML body of the S1 structured send. <img src=\"cid:dot@s1\"></p>");
        if let Some(reply_to) = body["reply_to"].as_str() {
            builder.set_reply_to(reply_to);
        }
        let headers = js_sys::Object::new();
        for (name, value) in [
            ("In-Reply-To", "<parent-s1@pylotamail.com>"),
            (
                "References",
                "<root-s1@pylotamail.com> <parent-s1@pylotamail.com>",
            ),
            ("Auto-Submitted", "auto-generated"),
            ("X-Spike", "s1-structured"),
        ] {
            js_sys::Reflect::set(&headers, &name.into(), &value.into()).map_err(js_err)?;
        }
        builder.set_headers(headers.unchecked_ref());
        // A 1x1 transparent GIF, inline with a contentId, and a text file as an attachment.
        let gif = "R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
        let attachments = [
            worker::EmailAttachment::new_inline("dot@s1", "dot.gif", "image/gif", gif),
            worker::EmailAttachment::new_attachment(
                "notes.txt",
                "text/plain",
                "UzEgYXR0YWNobWVudA==",
            ),
        ];
        builder.set_attachments(&attachments);
        let result = self
            .binding()?
            .send_with_builder(&builder)
            .await
            .map_err(|e| js_err(e.into()))?;
        let entry = json!({ "kind": "structured", "to": to, "message_id": result.message_id() });
        self.append("send", &entry).await?;
        Ok(entry)
    }

    /// A raw `EmailMessage`: `mode` "one" sends one message whose envelope `to` lists every recipient,
    /// "each" sends one message per recipient. The body is `raw` as given.
    async fn send_raw(&self, body: &Value) -> Result<Value, String> {
        let from = str_field(body, "from")?;
        let raw = str_field(body, "raw")?;
        let to = strings(&body["to"]);
        let envelopes = match body["mode"].as_str() {
            Some("one") => vec![to.join(", ")],
            _ => to.clone(),
        };
        let mut results = Vec::new();
        for envelope in envelopes {
            let outcome = match worker::email::EmailMessage::new(&from, &envelope, &raw) {
                Ok(message) => match self.binding()?.send(&message).await {
                    Ok(r) => json!({ "to": envelope, "message_id": r.message_id() }),
                    Err(e) => json!({ "to": envelope, "send_error": js_err(e.into()) }),
                },
                Err(e) => json!({ "to": envelope, "construct_error": js_err(e.into()) }),
            };
            results.push(outcome);
        }
        let entry = json!({ "kind": "raw", "mode": body["mode"], "results": results });
        self.append("send", &entry).await?;
        Ok(entry)
    }

    async fn queue_send(&self, body: &Value) -> Result<Value, String> {
        let queue: worker::Queue = self.platform.env().queue("SPIKE_QUEUE").map_err(err)?;
        let delay = body["delay_seconds"].as_u64().unwrap_or(0) as u32;
        let message = json!({
            "sent_ms": js_sys::Date::now(),
            "retries": body["retries"].as_u64().unwrap_or(2),
            "retry_delay": body["retry_delay"].as_u64().unwrap_or(2),
            "delay_seconds": delay,
        });
        // `Queue::send` with a `serde_json::Value` arrives as `{}`: serde_wasm_bindgen turns a JSON object
        // into a JS `Map`, which JSON serialisation drops. A body parsed with `JSON.parse` is a plain
        // object (finding for M4's QueueProducer).
        let plain = js_sys::JSON::parse(&message.to_string()).map_err(js_err)?;
        let send = worker::RawMessageBuilder::new(plain)
            .delay_seconds(delay)
            .build_with_content_type(worker::QueueContentType::Json);
        queue.send_raw(send).await.map_err(err)?;
        let map_form = worker::MessageBuilder::new(json!({ "map_form": true })).build();
        queue.send(map_form).await.map_err(err)?;
        Ok(message)
    }

    /// `D1Database::batch` is one transaction: a batch whose last statement fails leaves nothing.
    async fn d1_batch(&self) -> Result<Value, String> {
        let db = self.platform.env().d1("SPIKE_DB").map_err(err)?;
        db.exec("CREATE TABLE IF NOT EXISTS s1 (k TEXT PRIMARY KEY, v INTEGER)")
            .await
            .map_err(err)?;
        db.exec("DELETE FROM s1").await.map_err(err)?;
        let ok = db
            .batch(vec![
                db.prepare("INSERT INTO s1 (k, v) VALUES ('a', 1)"),
                db.prepare("INSERT INTO s1 (k, v) VALUES ('b', 2)"),
            ])
            .await
            .map_err(err)?;
        let failed = db
            .batch(vec![
                db.prepare("INSERT INTO s1 (k, v) VALUES ('c', 3)"),
                db.prepare("INSERT INTO s1 (k, v) VALUES ('a', 9)"), // duplicate key
            ])
            .await;
        let rows: Vec<Value> = db
            .prepare("SELECT k, v FROM s1 ORDER BY k")
            .all()
            .await
            .map_err(err)?
            .results()
            .map_err(err)?;
        Ok(json!({
            "first_batch_results": ok.len(),
            "second_batch_error": failed.err().map(|e| e.to_string()),
            "rows": rows,
        }))
    }

    async fn object(&self, method: &str, path: &str, body: &[u8]) -> Result<Value, String> {
        let ns = self
            .platform
            .env()
            .durable_object("SPIKE_OBJECT")
            .map_err(err)?;
        let stub = ns
            .id_from_name("spike-s1")
            .map_err(err)?
            .get_stub()
            .map_err(err)?;
        let mut init = worker::RequestInit::new();
        init.with_method(if method == "GET" {
            worker::Method::Get
        } else {
            worker::Method::Post
        });
        if method != "GET" {
            init.with_body(Some(js_sys::Uint8Array::from(body).into()));
        }
        let request =
            worker::Request::new_with_init(&format!("https://object{path}"), &init).map_err(err)?;
        let mut response = stub.fetch_with_request(request).await.map_err(err)?;
        response.json().await.map_err(err)
    }

    async fn append(&self, kind: &str, entry: &Value) -> Result<Value, String> {
        let line = json!({ "kind": kind, "entry": entry });
        self.object("POST", "/append", line.to_string().as_bytes())
            .await
    }

    fn binding(&self) -> Result<worker::SendEmail, String> {
        self.platform
            .env()
            .get_binding::<worker::SendEmail>("SEND")
            .map_err(err)
    }
}

/// One object holds the log and runs the SQLite and alarm checks.
pub struct SpikeObject {
    state: CfObjectState,
}

impl ObjectApp for SpikeObject {
    fn new(state: CfObjectState, _platform: CfPlatform) -> Self {
        let sql = state.state().storage().sql();
        let _ = sql.exec(
            "CREATE TABLE IF NOT EXISTS log (n INTEGER PRIMARY KEY, kind TEXT, at REAL, entry TEXT)",
            None,
        );
        let _ = sql.exec("CREATE TABLE IF NOT EXISTS t (x INTEGER)", None);
        Self { state }
    }

    async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
        let path = req.url.trim_start_matches("https://object");
        let result = match path.split('?').next().unwrap_or_default() {
            "/append" => self.append(&req.body),
            "/log" => self.read_log(),
            "/do/txn" => self.txn(),
            "/do/alarm" => self.set_alarm().await,
            other => Err(format!("object: no route {other}")),
        };
        match result {
            Ok(value) => reply(200, &value),
            Err(error) => reply(500, &json!({ "error": error })),
        }
    }

    async fn alarm(&self) {
        let _ = self.append(
            json!({ "kind": "alarm", "entry": { "fired_ms": js_sys::Date::now() } })
                .to_string()
                .as_bytes(),
        );
    }
}

impl SpikeObject {
    fn append(&self, body: &[u8]) -> Result<Value, String> {
        let line: Value = serde_json::from_slice(body).map_err(|e| e.to_string())?;
        self.state
            .state()
            .storage()
            .sql()
            .exec(
                "INSERT INTO log (kind, at, entry) VALUES (?, ?, ?)",
                vec![
                    line["kind"].as_str().unwrap_or("?").into(),
                    js_sys::Date::now().into(),
                    line["entry"].to_string().into(),
                ],
            )
            .map_err(err)?;
        Ok(json!({ "ok": true }))
    }

    fn read_log(&self) -> Result<Value, String> {
        let rows: Vec<Value> = self
            .state
            .state()
            .storage()
            .sql()
            .exec("SELECT n, kind, at, entry FROM log ORDER BY n", None)
            .map_err(err)?
            .to_array()
            .map_err(err)?;
        Ok(Value::Array(rows))
    }

    /// `transactionSync`: a callback that inserts and then fails leaves no row; one that succeeds
    /// leaves its row.
    fn txn(&self) -> Result<Value, String> {
        let storage = self.state.state().storage();
        let sql = storage.sql();
        sql.exec("DELETE FROM t", None).map_err(err)?;
        let inner = sql.clone();
        let failed = transaction_sync(&storage, move || -> Result<(), String> {
            inner
                .exec("INSERT INTO t (x) VALUES (1)", None)
                .map_err(err)?;
            Err("deliberate failure after the insert".to_owned())
        });
        let after_failure = count(&sql)?;
        let inner = sql.clone();
        let succeeded = transaction_sync(&storage, move || -> Result<u32, String> {
            inner
                .exec("INSERT INTO t (x) VALUES (2)", None)
                .map_err(err)?;
            Ok(2)
        });
        let after_success = count(&sql)?;
        Ok(json!({
            "failed_callback": failed.err(),
            "rows_after_failed_transaction": after_failure,
            "succeeded_callback": succeeded.ok(),
            "rows_after_successful_transaction": after_success,
        }))
    }

    async fn set_alarm(&self) -> Result<Value, String> {
        let at = js_sys::Date::now() + 2_000.0;
        self.state
            .state()
            .storage()
            .set_alarm(worker::ScheduledTime::new(js_sys::Date::new(&at.into())))
            .await
            .map_err(err)?;
        Ok(json!({ "alarm_at_ms": at }))
    }
}

fn count(sql: &worker::SqlStorage) -> Result<i64, String> {
    let rows: Vec<Value> = sql
        .exec("SELECT COUNT(*) AS n FROM t", None)
        .map_err(err)?
        .to_array()
        .map_err(err)?;
    Ok(rows.first().and_then(|r| r["n"].as_i64()).unwrap_or(-1))
}

fn header(req: &HttpRequestIn, name: &str) -> Option<String> {
    req.headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
}

fn str_field(body: &Value, name: &str) -> Result<String, String> {
    body[name]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("`{name}` is required"))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn reply(status: u16, value: &Value) -> HttpResponseOut {
    HttpResponseOut::Http {
        status,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: value.to_string().into_bytes(),
    }
}

fn err(e: worker::Error) -> String {
    e.to_string()
}

fn js_err(e: JsValue) -> String {
    let code = js_sys::Reflect::get(&e, &"code".into())
        .ok()
        .and_then(|c| c.as_string());
    let message = js_sys::Reflect::get(&e, &"message".into())
        .ok()
        .and_then(|m| m.as_string())
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"));
    match code {
        Some(code) => format!("{code}: {message}"),
        None => message,
    }
}
