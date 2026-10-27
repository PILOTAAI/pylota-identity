//! Spikes S1, S2, S3 and S6 (build plan M1): every binding the design relies on, called from Rust through the entry glue
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
            ("POST", path) if path.starts_with("/s6/") => self.s6(path, &body, &req.body).await,
            ("POST", "/s12/smtp") => Ok(s12_smtp(&body).await),
            ("POST", "/s12/tls") => Ok(s12_tls(&body).await),
            ("POST", "/do/txn") | ("POST", "/do/alarm") | ("POST", "/do/sql") | ("GET", "/log") => {
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
        if local.starts_with("reject") {
            EmailOutcome::Reject {
                smtp: "spike s1 reject".to_owned(),
            }
        } else if local.starts_with("tempfail") {
            EmailOutcome::TempFail
        } else {
            EmailOutcome::Accept
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

    /// Spike S6: Vectorize, `AI.run` with the `gateway` option and `AI.toMarkdown` through
    /// `platform::cf::externs`, and Durable Object IDs in the EU jurisdiction.
    async fn s6(&self, path: &str, body: &Value, raw: &[u8]) -> Result<Value, String> {
        use super::externs::{AiBinding, VectorizeIndex, binding};
        let env = self.platform.env();
        let gateway =
            json!({ "gateway": { "id": "pi-spike-6", "collectLog": false, "skipCache": true } });
        let js = |v: &Value| js_sys::JSON::parse(&v.to_string()).map_err(js_err);
        let back = |v: JsValue| -> Result<Value, String> {
            let text = js_sys::JSON::stringify(&v)
                .map_err(js_err)?
                .as_string()
                .unwrap_or_default();
            serde_json::from_str(&text).map_err(|e| e.to_string())
        };
        let wait = |p: Result<js_sys::Promise, JsValue>| async move {
            wasm_bindgen_futures::JsFuture::from(p.map_err(js_err)?)
                .await
                .map_err(js_err)
        };
        match path {
            "/s6/ai" => {
                let ai: AiBinding = binding(env, "AI")?;
                let model = str_field(body, "model")?;
                let options = if body["no_gateway"] == true {
                    json!({})
                } else {
                    gateway
                };
                let out =
                    wait(ai.run_with_options(&model, &js(&body["input"])?, &js(&options)?)).await?;
                back(out)
            }
            "/s6/upsert" => {
                let index: VectorizeIndex = binding(env, "VECTORS")?;
                let vectors = js_sys::Array::new();
                for v in body["vectors"].as_array().into_iter().flatten() {
                    let item = js(v)?;
                    let values: Vec<f32> = v["values"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|x| x.as_f64())
                        .map(|x| x as f32)
                        .collect();
                    js_sys::Reflect::set(
                        &item,
                        &"values".into(),
                        &js_sys::Float32Array::from(values.as_slice()),
                    )
                    .map_err(js_err)?;
                    vectors.push(&item);
                }
                back(wait(index.upsert(&vectors)).await?)
            }
            "/s6/query" => {
                let index: VectorizeIndex = binding(env, "VECTORS")?;
                let values: Vec<f32> = body["vector"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|x| x.as_f64())
                    .map(|x| x as f32)
                    .collect();
                let out = wait(index.query(
                    &js_sys::Float32Array::from(values.as_slice()),
                    &js(&body["options"])?,
                ))
                .await?;
                back(out)
            }
            "/s6/get" | "/s6/delete" => {
                let index: VectorizeIndex = binding(env, "VECTORS")?;
                let ids = js(&body["ids"])?.unchecked_into::<js_sys::Array>();
                let out = if path == "/s6/get" {
                    wait(index.get_by_ids(&ids)).await?
                } else {
                    wait(index.delete_by_ids(&ids)).await?
                };
                back(out)
            }
            "/s6/describe" => {
                let index: VectorizeIndex = binding(env, "VECTORS")?;
                back(wait(index.describe()).await?)
            }
            "/s6/markdown" => {
                let ai: AiBinding = binding(env, "AI")?;
                let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(raw));
                let options = web_sys_blob_options("application/pdf")?;
                let blob = js_sys::Reflect::construct(
                    &js_sys::Reflect::get(&js_sys::global(), &"Blob".into())
                        .map_err(js_err)?
                        .unchecked_into::<js_sys::Function>(),
                    &js_sys::Array::of2(&parts, &options),
                )
                .map_err(js_err)?;
                let file = js(&json!({ "name": "spike.pdf" }))?;
                js_sys::Reflect::set(&file, &"blob".into(), &blob).map_err(js_err)?;
                back(wait(ai.to_markdown(&js_sys::Array::of1(&file), &JsValue::UNDEFINED)).await?)
            }
            "/s6/jurisdiction" => {
                let ns = env.durable_object("SPIKE_OBJECT").map_err(err)?;
                let eu = ns
                    .unique_id_with_jurisdiction("eu")
                    .map_err(err)?
                    .to_string();
                let plain = ns.unique_id().map_err(err)?.to_string();
                let mut out = json!({ "eu_id": eu, "plain_id": plain });
                for (key, id) in [("eu", eu), ("plain", plain)] {
                    let stub = ns
                        .id_from_string(&id)
                        .map_err(err)?
                        .get_stub()
                        .map_err(err)?;
                    let mut response = stub
                        .fetch_with_str("https://object/do/whoami")
                        .await
                        .map_err(err)?;
                    out[key] = response.json().await.map_err(err)?;
                }
                Ok(out)
            }
            other => Err(format!("no route {other}")),
        }
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
            "/do/sql" => self.sql_script(&req.body),
            "/do/whoami" => self.whoami(),
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

    /// Runs `{"statements": [...]}` one by one in the object's SQLite and reports each result or error,
    /// so the SQL checks of spikes S3 and S4 can be driven from outside the Worker.
    fn sql_script(&self, body: &[u8]) -> Result<Value, String> {
        let script: Value = serde_json::from_slice(body).map_err(|e| e.to_string())?;
        let sql = self.state.state().storage().sql();
        let mut out = Vec::new();
        for statement in script["statements"].as_array().into_iter().flatten() {
            let text = statement.as_str().unwrap_or_default();
            let result = sql
                .exec(text, None)
                .and_then(|cursor| cursor.to_array::<Value>());
            out.push(match result {
                Ok(rows) => json!({ "sql": text, "rows": rows }),
                Err(e) => json!({ "sql": text, "error": e.to_string() }),
            });
        }
        Ok(Value::Array(out))
    }

    /// The object's own ID and `ctx.id.jurisdiction` (S6).
    fn whoami(&self) -> Result<Value, String> {
        let id: JsValue = self.state.state().as_raw().id().map_err(js_err)?.into();
        let jurisdiction = js_sys::Reflect::get(&id, &"jurisdiction".into())
            .ok()
            .and_then(|j| j.as_string());
        Ok(json!({ "id": self.state.state().id().to_string(), "jurisdiction": jurisdiction }))
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

fn web_sys_blob_options(mime: &str) -> Result<JsValue, String> {
    let options = js_sys::Object::new();
    js_sys::Reflect::set(&options, &"type".into(), &mime.into()).map_err(js_err)?;
    Ok(options.into())
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

/// The current size of the wasm linear memory, in bytes (spike S4's peak-memory measure: linear memory
/// only grows, so its size after a request is that request's peak or an earlier one's).
pub fn wasm_memory_bytes() -> f64 {
    wasm_bindgen::memory()
        .unchecked_into::<js_sys::WebAssembly::Memory>()
        .buffer()
        .unchecked_into::<js_sys::ArrayBuffer>()
        .byte_length() as f64
}

// ---- Spike S12: SMTP from a Worker -------------------------------------------------------------------

/// One TCP connection, driven through the runtime's streams (`worker::Socket` reads through tokio's
/// traits, which `platform` must not depend on).
struct Conn {
    sock: worker::worker_sys::Socket,
    reader: JsValue,
    writer: JsValue,
    buf: Vec<u8>,
}

const CONNECT_MS: f64 = 10_000.0;
const COMMAND_MS: f64 = 30_000.0;

async fn with_timeout(promise: js_sys::Promise, ms: f64) -> Result<JsValue, String> {
    let timer = js_sys::Promise::new(&mut |resolve, _| {
        let set_timeout: js_sys::Function =
            js_sys::Reflect::get(&js_sys::global(), &"setTimeout".into())
                .expect("setTimeout")
                .unchecked_into();
        // setTimeout(resolve, ms, "__timeout__"): the timer resolves with the marker.
        let _ = set_timeout.call3(
            &JsValue::UNDEFINED,
            &resolve,
            &ms.into(),
            &"__timeout__".into(),
        );
    });
    let raced = js_sys::Promise::race(&js_sys::Array::of2(&promise, &timer));
    let value = wasm_bindgen_futures::JsFuture::from(raced)
        .await
        .map_err(js_err)?;
    if value.as_string().as_deref() == Some("__timeout__") {
        return Err(format!("timeout after {ms} ms"));
    }
    Ok(value)
}

fn call0(target: &JsValue, method: &str) -> Result<JsValue, String> {
    let f: js_sys::Function = js_sys::Reflect::get(target, &method.into())
        .map_err(js_err)?
        .unchecked_into();
    f.call0(target).map_err(js_err)
}

impl Conn {
    async fn open(host: &str, port: u16, transport: &str) -> Result<Self, String> {
        let address = js_sys::Object::new();
        js_sys::Reflect::set(&address, &"hostname".into(), &host.into()).map_err(js_err)?;
        js_sys::Reflect::set(&address, &"port".into(), &port.into()).map_err(js_err)?;
        let options = js_sys::Object::new();
        js_sys::Reflect::set(&options, &"secureTransport".into(), &transport.into())
            .map_err(js_err)?;
        let sock = worker::worker_sys::connect(address.into(), options.into()).map_err(js_err)?;
        with_timeout(sock.opened().map_err(js_err)?, CONNECT_MS).await?;
        Self::attach(sock)
    }

    fn attach(sock: worker::worker_sys::Socket) -> Result<Self, String> {
        let reader = call0(&sock.readable().map_err(js_err)?.into(), "getReader")?;
        let writer = call0(&sock.writable().map_err(js_err)?.into(), "getWriter")?;
        Ok(Self {
            sock,
            reader,
            writer,
            buf: Vec::new(),
        })
    }

    /// Releases the stream locks, upgrades with `startTls()` and re-attaches.
    async fn start_tls(self) -> Result<Self, String> {
        call0(&self.reader, "releaseLock")?;
        call0(&self.writer, "releaseLock")?;
        let upgraded = self.sock.start_tls().map_err(js_err)?;
        with_timeout(upgraded.opened().map_err(js_err)?, CONNECT_MS).await?;
        Self::attach(upgraded)
    }

    async fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        let f: js_sys::Function = js_sys::Reflect::get(&self.writer, &"write".into())
            .map_err(js_err)?
            .unchecked_into();
        let p: js_sys::Promise = f
            .call1(&self.writer, &js_sys::Uint8Array::from(bytes))
            .map_err(js_err)?
            .unchecked_into();
        with_timeout(p, COMMAND_MS).await.map(|_| ())
    }

    /// Reads one SMTP reply (all its lines); returns the code and the text.
    async fn reply(&mut self, timeout_ms: f64) -> Result<(u16, String), String> {
        loop {
            if let Some(end) = complete_reply(&self.buf) {
                let text = String::from_utf8_lossy(&self.buf[..end])
                    .trim_end()
                    .to_owned();
                self.buf.drain(..end);
                let code = text.get(..3).and_then(|c| c.parse().ok()).unwrap_or(0);
                return Ok((code, text));
            }
            let p: js_sys::Promise = call0(&self.reader, "read")?.unchecked_into();
            let chunk = with_timeout(p, timeout_ms).await?;
            if js_sys::Reflect::get(&chunk, &"done".into())
                .map_err(js_err)?
                .is_truthy()
            {
                return Err("connection closed".to_owned());
            }
            let value = js_sys::Reflect::get(&chunk, &"value".into()).map_err(js_err)?;
            self.buf.extend(js_sys::Uint8Array::new(&value).to_vec());
        }
    }

    async fn command(
        &mut self,
        line: &str,
        log: &mut Vec<String>,
        shown: &str,
    ) -> Result<(u16, String), String> {
        self.send(format!("{line}\r\n").as_bytes()).await?;
        let reply = self.reply(COMMAND_MS).await?;
        log.push(format!("C: {shown}"));
        log.push(format!("S: {}", first_line(&reply.1)));
        Ok(reply)
    }
}

/// The end of the first complete reply in `buf`: the line whose fourth byte is a space.
fn complete_reply(buf: &[u8]) -> Option<usize> {
    let mut start = 0;
    while let Some(nl) = buf[start..].iter().position(|&b| b == b'\n') {
        let line = &buf[start..start + nl];
        if line.len() >= 4 && line[3] == b' ' {
            return Some(start + nl + 1);
        }
        start += nl + 1;
    }
    None
}

fn first_line(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    match lines.len() {
        0 | 1 => text.to_owned(),
        n => format!(
            "{} (+{} lines: {})",
            lines[0],
            n - 1,
            lines[1..]
                .iter()
                .map(|l| l.get(4..).unwrap_or(l))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn btoa(text: &str) -> Result<String, String> {
    let f: js_sys::Function = js_sys::Reflect::get(&js_sys::global(), &"btoa".into())
        .map_err(js_err)?
        .unchecked_into();
    f.call1(&JsValue::UNDEFINED, &text.into())
        .map_err(js_err)?
        .as_string()
        .ok_or_else(|| "btoa".to_owned())
}

/// `{host, port, mode: "implicit"|"starttls", user, password, from?, to?}`: connects, authenticates and,
/// when `to` is given, sends one message; returns the redacted transcript and where it stopped.
async fn s12_smtp(body: &Value) -> Value {
    let mut log = Vec::new();
    let started = js_sys::Date::now();
    let outcome = s12_session(body, &mut log).await;
    json!({
        "host": body["host"], "port": body["port"], "mode": body["mode"],
        "ok": outcome.is_ok(), "error": outcome.as_ref().err(), "message_id": outcome.ok(),
        "transcript": log, "elapsed_ms": js_sys::Date::now() - started,
    })
}

async fn s12_session(body: &Value, log: &mut Vec<String>) -> Result<Option<String>, String> {
    let host = str_field(body, "host")?;
    let port = body["port"].as_u64().unwrap_or(587) as u16;
    let implicit = body["mode"] == "implicit";
    let mut conn = Conn::open(&host, port, if implicit { "on" } else { "starttls" }).await?;
    let greeting = conn.reply(COMMAND_MS).await?;
    log.push(format!("S: {}", first_line(&greeting.1)));
    let (code, ehlo) = conn
        .command(
            "EHLO pylota-spike.example",
            log,
            "EHLO pylota-spike.example",
        )
        .await?;
    if code != 250 {
        return Err(format!("EHLO {code}"));
    }
    if !implicit {
        if !ehlo.to_ascii_uppercase().contains("STARTTLS") {
            return Err("smtp_tls_required: no STARTTLS advertised".to_owned());
        }
        let (code, _) = conn.command("STARTTLS", log, "STARTTLS").await?;
        if code != 220 {
            return Err(format!("STARTTLS {code}"));
        }
        conn = conn.start_tls().await?;
        log.push("-- TLS established --".to_owned());
        conn.command(
            "EHLO pylota-spike.example",
            log,
            "EHLO pylota-spike.example",
        )
        .await?;
    }
    let plain = btoa(&format!(
        "\0{}\0{}",
        str_field(body, "user")?,
        str_field(body, "password")?
    ))?;
    let (code, text) = conn
        .command(&format!("AUTH PLAIN {plain}"), log, "AUTH PLAIN [redacted]")
        .await?;
    if code != 235 {
        return Err(format!("smtp_auth_failed: {}", first_line(&text)));
    }
    let Some(to) = body["to"].as_str() else {
        conn.command("QUIT", log, "QUIT").await.ok();
        return Ok(None);
    };
    let from = str_field(body, "from")?;
    let message_id = format!("<s12-{}@pylota-spike.example>", js_sys::Date::now() as u64);
    for (line, expect) in [
        (format!("MAIL FROM:<{from}>"), 250),
        (format!("RCPT TO:<{to}>"), 250),
        ("DATA".to_owned(), 354),
    ] {
        let (code, text) = conn.command(&line, log, &line).await?;
        if code != expect {
            return Err(format!("{line}: {}", first_line(&text)));
        }
    }
    let message = format!(
        "From: <{from}>\r\nTo: <{to}>\r\nSubject: S12 {host}:{port} {}\r\nMessage-ID: {message_id}\r\nDate: Tue, 27 Oct 2026 21:30:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nSpike S12 over {host}:{port} ({}).\r\n.leading dot is stuffed\r\n",
        body["mode"].as_str().unwrap_or(""),
        body["mode"].as_str().unwrap_or("")
    );
    let stuffed = message.replace("\r\n.", "\r\n..");
    conn.send(stuffed.as_bytes()).await?;
    let (code, text) = conn.command(".", log, ".").await?;
    if code != 250 {
        return Err(format!("final dot: {}", first_line(&text)));
    }
    conn.command("QUIT", log, "QUIT").await.ok();
    Ok(Some(message_id))
}

/// `{host, port}`: opens TLS from the start and reports whether the handshake succeeded, to show that the
/// certificate's host name is checked; a closed port shows the connect timeout.
async fn s12_tls(body: &Value) -> Value {
    let started = js_sys::Date::now();
    let host = body["host"].as_str().unwrap_or_default().to_owned();
    let port = body["port"].as_u64().unwrap_or(443) as u16;
    let transport = body["transport"].as_str().unwrap_or("on").to_owned();
    let result = async {
        let mut conn = Conn::open(&host, port, &transport).await?;
        conn.send(
            format!("HEAD / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await?;
        let p: js_sys::Promise = call0(&conn.reader, "read")?.unchecked_into();
        let chunk = with_timeout(p, COMMAND_MS).await?;
        let value = js_sys::Reflect::get(&chunk, &"value".into()).map_err(js_err)?;
        let bytes = if value.is_undefined() {
            Vec::new()
        } else {
            js_sys::Uint8Array::new(&value).to_vec()
        };
        Ok::<String, String>(
            String::from_utf8_lossy(&bytes)
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned(),
        )
    }
    .await;
    json!({ "host": host, "port": port, "ok": result.is_ok(), "first_line": result.as_ref().ok(),
            "error": result.err(), "elapsed_ms": js_sys::Date::now() - started })
}
