//! The fake server on `127.0.0.1:8798` (Testing § 6.3–6.6), hosted by the test process: the fake clock
//! and the fault plan the Worker reads at each invocation, the queue messages the itest build hands
//! over, the hosts the Worker's outbound requests are rerouted to (`/__http/{host}/…`: the two DoH
//! resolvers, answering from the corpus zones in `crates/conformance/corpus/dns/zones.toml`, the
//! Cloudflare API and the webhook receivers), and the bindings the itest build routes here: Email
//! Sending (`/__mail/send`), Workers AI (`/__ai/run`) and Vectorize (`/__vec/{binding}/{op}`). A blocking
//! server on `std::net`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::sync::{Mutex, Once};

use serde_json::{Value, json};

struct State {
    offset: i64,
    /// Worker variables and secrets read instead of the configured ones (`None` unsets one).
    config: BTreeMap<String, Option<String>>,
    /// The D1 size every statement reports instead of the real one (J30).
    d1_size: Option<u64>,
    faults: Vec<Value>,
    /// Messages the itest build's queue consumer handed over: `(queue, body)`.
    queued: Vec<(String, Value)>,
    /// TXT records by name; `None` answers SERVFAIL.
    txt: BTreeMap<String, Option<Vec<String>>>,
    /// Requests the receivers got: `(host and path, headers, body)`.
    received: Vec<Received>,
    /// Scripted statuses per receiver path, taken in order; the last one repeats.
    statuses: BTreeMap<String, Vec<u16>>,
    /// Scripted answers of the Email Sending binding by recipient; the last one repeats.
    mail_script: Vec<(String, Vec<Value>)>,
    /// Every structured send the binding got.
    mail_sent: Vec<Value>,
    /// Email Sending suppressions by address (`result` items of the API).
    cf_suppressions: BTreeMap<String, Vec<Value>>,
    /// Workers AI: statuses to answer instead, per model substring, taken in order.
    ai_fail: Vec<(String, Vec<u16>)>,
    /// Workers AI: scripted answers per model substring, taken in order (`{"__delay_ms": n}` waits
    /// first, `{"__status": n}` fails).
    ai_script: Vec<(String, Vec<Value>)>,
    /// The inputs of the calls of scripted models, in order.
    ai_inputs: Vec<(String, Value)>,
    /// Vectorize by binding, by vector ID.
    vectors: BTreeMap<String, BTreeMap<String, Rec>>,
    /// `describe()` reports this count instead of the stored one.
    vec_count: Option<u64>,
    /// Vectorize operations to fail, by op name, with the number of failures left.
    vec_fail: BTreeMap<String, u32>,
    /// Vector IDs that `deleteByIds` leaves in place (an index that keeps a vector, F6).
    vec_keep: BTreeSet<String>,
}

/// One request a fake webhook receiver got.
#[derive(Clone, Debug)]
pub struct Received {
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Received {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

/// Receiver hosts: any name under this suffix resolves to a public address and answers here.
pub const RECEIVERS: &str = ".hooks.example.com";

static STATE: Mutex<State> = Mutex::new(State {
    offset: 0,
    config: BTreeMap::new(),
    d1_size: None,
    faults: Vec::new(),
    queued: Vec::new(),
    txt: BTreeMap::new(),
    received: Vec::new(),
    statuses: BTreeMap::new(),
    mail_script: Vec::new(),
    mail_sent: Vec::new(),
    cf_suppressions: BTreeMap::new(),
    ai_fail: Vec::new(),
    ai_script: Vec::new(),
    ai_inputs: Vec::new(),
    vectors: BTreeMap::new(),
    vec_count: None,
    vec_fail: BTreeMap::new(),
    vec_keep: BTreeSet::new(),
});

/// The corpus zones, loaded once.
fn load_zones(s: &mut State) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../conformance/corpus/dns/zones.toml");
    let text = std::fs::read_to_string(path).expect("the corpus zones");
    let t: toml::Table = text.parse().expect("zones.toml parses");
    if let Some(txt) = t.get("txt").and_then(|v| v.as_table()) {
        for (name, records) in txt {
            let rs = records
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|r| r.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            s.txt.insert(name.to_ascii_lowercase(), Some(rs));
        }
    }
    if let Some(names) = t
        .get("temperror")
        .and_then(|v| v.get("names"))
        .and_then(|v| v.as_array())
    {
        for n in names.iter().filter_map(|n| n.as_str()) {
            s.txt.insert(n.to_ascii_lowercase(), None);
        }
    }
}

/// A DoH JSON answer (`application/dns-json`) for `name` and `type`.
fn doh(s: &State, query: &str) -> Value {
    let mut name = String::new();
    let mut rtype = String::new();
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("name", v)) => name = v.replace("%5F", "_").to_ascii_lowercase(),
            Some(("type", v)) => rtype = v.to_ascii_uppercase(),
            _ => {}
        }
    }
    let name = name.trim_end_matches('.').to_owned();
    let resolver = if query.contains("__resolver=dns.google") {
        "dns.google"
    } else {
        "cloudflare-dns.com"
    };
    if let Some(v) = crate::cloud::dns(resolver, &name, &rtype, query.contains("cd=1")) {
        return v;
    }
    if name.ends_with(RECEIVERS) {
        // A public address that is never contacted (the Worker's requests come here by host), or
        // a private one for names starting `private.`, for the SSRF checks.
        let a = if name.starts_with("private.") {
            "10.0.0.5"
        } else {
            "93.184.216.34"
        };
        return match rtype.as_str() {
            "A" => {
                json!({ "Status": 0, "Answer": [{ "name": name, "type": 1, "TTL": 60, "data": a }] })
            }
            _ => json!({ "Status": 0, "Answer": [] }),
        };
    }
    match (rtype.as_str(), s.txt.get(&name)) {
        ("TXT", Some(None)) => json!({ "Status": 2, "Answer": [] }),
        ("TXT", Some(Some(records))) => json!({
            "Status": 0,
            "Answer": records.iter().map(|r| {
                // DoH quotes TXT data and splits it into strings of at most 255 bytes.
                let parts: Vec<String> = r.as_bytes().chunks(255)
                    .map(|c| format!("\"{}\"", String::from_utf8_lossy(c).replace('"', "\\\"")))
                    .collect();
                json!({ "name": name, "type": 16, "TTL": 300, "data": parts.join(" ") })
            }).collect::<Vec<_>>()
        }),
        _ => json!({ "Status": 3, "Answer": [] }),
    }
}

pub fn start() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        load_zones(&mut STATE.lock().unwrap());
        let listener = TcpListener::bind("127.0.0.1:8798").expect("port 8798 is free for the fake server");
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut len = 0usize;
                let mut headers = Vec::new();
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                        break;
                    }
                    if let Some((n, v)) = h.trim_end().split_once(':') {
                        headers.push((n.trim().to_ascii_lowercase(), v.trim().to_owned()));
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut raw = vec![0u8; len];
                let _ = reader.read_exact(&mut raw);
                let method = line.split_whitespace().next().unwrap_or("GET").to_owned();
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let (status, text) = match path.strip_prefix("/__http/").filter(|t| {
                    t.split(['/', '?']).next().is_some_and(|h| h.ends_with(RECEIVERS))
                }) {
                    Some(target) => (receive(target, headers, raw), String::new()),
                    None => {
                        let body: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
                        if let Some((st, v)) = suppressions(&method, &path)
                            .or_else(|| crate::cloud::cf(&method, &path, &body))
                            .or_else(|| crate::cloud::ses(&method, &path, &body))
                            .or_else(|| crate::cloud::rdap_call(&path))
                        {
                            let text = v.to_string();
                            let mut out = stream;
                            let _ = write!(out, "HTTP/1.1 {st} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}", text.len());
                            continue;
                        }
                        let v = handle(&path, &body);
                        match v["__status"].as_u64() {
                            Some(st) => (st as u16, String::new()),
                            None => (200, v.to_string()),
                        }
                    }
                };
                let mut out = stream;
                let _ = write!(out, "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}", text.len());
            }
        });
    });
}

fn handle(path: &str, body: &Value) -> Value {
    let mut s = STATE.lock().unwrap();
    match path {
        "/state" => {
            json!({ "clock_offset_ms": s.offset, "faults_armed": !s.faults.is_empty(), "config_overrides": s.config, "d1_size_after": s.d1_size })
        }
        "/faults/take" => {
            let pos = s.faults.iter().position(|f| {
                f["target"] == body["target"]
                    && (f["binding"].is_null() || f["binding"] == body["binding"])
            });
            match pos {
                Some(i) => {
                    let mode = s.faults[i]["mode"].clone();
                    let left = s.faults[i]["count"].as_i64().unwrap_or(1) - 1;
                    if left <= 0 {
                        s.faults.remove(i);
                    } else {
                        s.faults[i]["count"] = left.into();
                    }
                    json!({ "mode": mode })
                }
                None => json!({}),
            }
        }
        q if q.starts_with("/__http/cloudflare-dns.com/dns-query?")
            || q.starts_with("/__http/dns.google/resolve?") =>
        {
            let resolver = if q.starts_with("/__http/dns.google/") {
                "&__resolver=dns.google"
            } else {
                ""
            };
            doh(
                &s,
                &format!("{}{resolver}", q.split_once('?').map_or("", |(_, q)| q)),
            )
        }
        "/__mail/send" => {
            s.mail_sent.push(body.clone());
            let rcpts: Vec<String> = ["to", "cc", "bcc"]
                .iter()
                .flat_map(|f| body[*f].as_array().cloned().unwrap_or_default())
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
            let hit = s
                .mail_script
                .iter_mut()
                .find(|(r, _)| rcpts.iter().any(|a| a == r));
            match hit {
                Some((_, answers)) if answers.len() > 1 => answers.remove(0),
                Some((_, answers)) if !answers.is_empty() => answers[0].clone(),
                _ => {
                    let domain = body["from"]
                        .as_str()
                        .unwrap_or("")
                        .rsplit('@')
                        .next()
                        .unwrap_or("")
                        .to_owned();
                    json!({ "messageId": format!("<cf{}@{domain}>", s.mail_sent.len()) })
                }
            }
        }
        q if q.starts_with("/__http/api.cloudflare.com/") => {
            let email = q
                .split("email=")
                .nth(1)
                .unwrap_or("")
                .replace("%40", "@")
                .replace("%2B", "+");
            json!({ "success": true, "result": s.cf_suppressions.get(&email).cloned().unwrap_or_default() })
        }
        "/__ai/run" => ai_run(&mut s, body),
        q if q.starts_with("/__vec/") => {
            let mut parts = q["/__vec/".len()..].splitn(2, '/');
            let binding = parts.next().unwrap_or("").to_owned();
            let op = parts.next().unwrap_or("").to_owned();
            vec_op(&mut s, &binding, &op, body)
        }
        q if q.starts_with("/queues/") => {
            s.queued
                .push((q["/queues/".len()..].to_owned(), body.clone()));
            json!({})
        }
        _ => json!({}),
    }
}

/// The messages of `queue` the Worker has consumed so far.
pub fn queued(queue: &str) -> Vec<Value> {
    STATE
        .lock()
        .unwrap()
        .queued
        .iter()
        .filter(|(q, _)| q == queue)
        .map(|(_, b)| b.clone())
        .collect()
}

/// Moves the Worker's clock forward.
pub fn advance(ms: i64) {
    start();
    STATE.lock().unwrap().offset += ms;
}

/// The Worker's clock now, in ms.
pub fn worker_now() -> i64 {
    start();
    let real = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    real + STATE.lock().unwrap().offset
}

/// Moves the Worker's clock forward to the next `minute` of the UTC day (`0..1440`).
pub fn advance_to_minute(minute: i64) {
    let day = 86_400_000;
    let now = worker_now();
    let target = now.div_euclid(day) * day + minute * 60_000 + 30_000;
    advance((target - now).rem_euclid(day));
}

/// Arms a fault: `target` (`do.call`), `binding` (`QUOTA`), `mode`, `count`.
pub fn fault(target: &str, binding: &str, mode: &str, count: u32) {
    start();
    STATE
        .lock()
        .unwrap()
        .faults
        .push(json!({ "target": target, "binding": binding, "mode": mode, "count": count }));
}

/// A receiver's request: recorded, answered with its path's next scripted status (200 by default).
fn receive(target: &str, headers: Vec<(String, String)>, body: Vec<u8>) -> u16 {
    let mut s = STATE.lock().unwrap();
    let key = target.split('?').next().unwrap_or("").to_owned();
    let status = match s.statuses.get_mut(&key) {
        Some(list) if list.len() > 1 => list.remove(0),
        Some(list) => list.first().copied().unwrap_or(200),
        None => 200,
    };
    s.received.push(Received {
        target: key,
        headers,
        body,
    });
    status
}

/// Scripts the statuses of a receiver path (`host/path`), in order; the last repeats.
pub fn respond(target: &str, statuses: &[u16]) {
    start();
    STATE
        .lock()
        .unwrap()
        .statuses
        .insert(target.to_owned(), statuses.to_vec());
}

/// The requests a receiver path (`host/path`) got so far.
pub fn received(target: &str) -> Vec<Received> {
    STATE
        .lock()
        .unwrap()
        .received
        .iter()
        .filter(|r| r.target == target)
        .cloned()
        .collect()
}

/// Arms a fault on `target` for any binding.
pub fn fault_any(target: &str, mode: &str, count: u32) {
    start();
    STATE
        .lock()
        .unwrap()
        .faults
        .push(json!({ "target": target, "binding": null, "mode": mode, "count": count }));
}

/// Overrides a Worker variable or secret from the next invocation on (`None` unsets it); objects keep
/// the configuration they started with. `Reset` restores the configured values.
pub fn set_config(name: &str, value: Option<&str>) {
    start();
    STATE
        .lock()
        .unwrap()
        .config
        .insert(name.to_owned(), value.map(str::to_owned));
}

/// Makes every D1 statement report `bytes` as the database's size (`meta.size_after`), or the real
/// size again with `None`.
pub fn set_d1_size(bytes: Option<u64>) {
    start();
    STATE.lock().unwrap().d1_size = bytes;
}

/// Resets the clock, the configuration overrides and the faults (a guard in each test that changes
/// them).
pub fn reset() {
    let mut s = STATE.lock().unwrap();
    s.offset = 0;
    s.config.clear();
    s.d1_size = None;
    s.faults.clear();
    s.ai_fail.clear();
    s.ai_script.clear();
    s.vec_fail.clear();
    s.vec_keep.clear();
    s.vec_count = None;
}

pub struct Reset;

impl Drop for Reset {
    fn drop(&mut self) {
        reset();
        // A test that panicked with failures still armed must not fail the next one.
        crate::cloud::unfail();
    }
}

/// Scripts the Email Sending binding for sends that include `recipient`: `{messageId}`,
/// `{code, message}`, `{exception}` or `{timeout: true}`, in order; the last repeats.
pub fn mail_script(recipient: &str, answers: Vec<Value>) {
    start();
    let mut s = STATE.lock().unwrap();
    s.mail_script.retain(|(r, _)| r != recipient);
    s.mail_script.push((recipient.to_owned(), answers));
}

/// The structured sends that included `recipient`.
pub fn mail_sent(recipient: &str) -> Vec<Value> {
    start();
    STATE
        .lock()
        .unwrap()
        .mail_sent
        .iter()
        .filter(|m| {
            ["to", "cc", "bcc"].iter().any(|f| {
                m[*f]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == recipient))
            })
        })
        .cloned()
        .collect()
}

/// An Email Sending suppression of `address` with `scope` (`account` or a sending domain).
pub fn cf_suppress(address: &str, scope: &str) {
    cf_suppress_with(address, scope, false);
}

/// As [`cf_suppress`], `read_only` like Cloudflare's own `policy` entries.
pub fn cf_suppress_with(address: &str, scope: &str, read_only: bool) {
    start();
    let mut s = STATE.lock().unwrap();
    let n: usize = s.cf_suppressions.values().map(Vec::len).sum();
    let id = format!("sup{n}{}", crate::slug(address));
    let item = if scope == "account" {
        json!({ "id": id, "email": address, "scope": "account", "reason": "hard_bounce", "read_only": read_only })
    } else {
        json!({ "id": id, "email": address, "scope": "sending_domain", "domain": scope, "reason": "hard_bounce", "read_only": read_only })
    };
    s.cf_suppressions
        .entry(address.to_owned())
        .or_default()
        .push(item);
}

/// The Email Sending suppressions of `address` still held by the fake.
pub fn cf_suppressions_of(address: &str) -> Vec<Value> {
    start();
    STATE
        .lock()
        .unwrap()
        .cf_suppressions
        .get(address)
        .cloned()
        .unwrap_or_default()
}

/// `GET …/email/sending/suppressions?email=` or `?scope_type=sending_domain&scope_value=`, and
/// `DELETE …/suppressions/{id}`.
fn suppressions(method: &str, path: &str) -> Option<(u16, Value)> {
    let rest = path.strip_prefix("/__http/api.cloudflare.com/client/v4/accounts/")?;
    let i = rest.find("/email/sending/suppressions")?;
    let tail = &rest[i + "/email/sending/suppressions".len()..];
    let mut s = STATE.lock().unwrap();
    if method == "DELETE" {
        let id = tail.trim_start_matches('/').to_owned();
        let mut found = false;
        for list in s.cf_suppressions.values_mut() {
            let before = list.len();
            list.retain(|x| x["id"] != id.as_str() || x["read_only"] == true);
            found |= list.len() != before;
        }
        return Some(if found {
            (200, json!({ "success": true, "result": {} }))
        } else {
            (
                404,
                json!({ "success": false, "errors": [{ "code": 1000, "message": "not found" }] }),
            )
        });
    }
    let q = tail.split_once('?').map_or("", |(_, q)| q);
    let param = |k: &str| {
        q.split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(|v| v.replace("%40", "@").replace("%2B", "+"))
    };
    let items: Vec<Value> = match (param("email"), param("scope_value")) {
        (Some(e), _) => s.cf_suppressions.get(&e).cloned().unwrap_or_default(),
        (None, Some(d)) => s
            .cf_suppressions
            .values()
            .flatten()
            .filter(|x| x["domain"] == d.as_str())
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    Some((200, json!({ "success": true, "result": items })))
}

// ---- Workers AI and Vectorize (Testing § 6.3) ------------------------------------------------------

/// Lower-case runs of letters and digits.
fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// A deterministic 1,024-dimension embedding: each token hashed (FNV-1a) into a bucket, normalised.
/// Texts that share words have a positive cosine; unrelated texts about zero.
pub fn embed_text(text: &str) -> Vec<f64> {
    let mut v = vec![0.0f64; 1024];
    for t in tokens(text) {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in t.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        v[(h % 1024) as usize] += 1.0;
    }
    let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if n == 0.0 {
        v[0] = 1.0;
    } else {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

fn ai_run(s: &mut State, body: &Value) -> Value {
    let model = body["model"].as_str().unwrap_or("").to_owned();
    let raw_input = body["input"].to_string();
    if let Some((_, statuses)) = s.ai_fail.iter_mut().find(|(m, st)| {
        let (m, marker) = m.split_once("@@").unwrap_or((m, ""));
        model.contains(m) && raw_input.contains(marker) && !st.is_empty()
    }) {
        let st = statuses.remove(0);
        return json!({ "__status": st });
    }
    let input = &body["input"];
    let text = input.to_string();
    // A script key is a model substring, or `model@@marker`: then only inputs containing the marker.
    let applies = |key: &str| {
        let (m, marker) = key.split_once("@@").unwrap_or((key, ""));
        model.contains(m) && text.contains(marker)
    };
    if model.contains("qwen")
        || model.contains("gpt-oss")
        || s.ai_script.iter().any(|(m, _)| applies(m))
    {
        s.ai_inputs.push((model.clone(), input.clone()));
    }
    if let Some((_, answers)) = s
        .ai_script
        .iter_mut()
        .find(|(m, a)| applies(m) && !a.is_empty())
    {
        let mut answer = answers.remove(0);
        if let Some(ms) = answer["__delay_ms"].as_u64() {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            if let Some(o) = answer.as_object_mut() {
                o.remove("__delay_ms");
            }
        }
        return answer;
    }
    if model.contains("qwen") {
        // Unscripted planner: stop at once; an answer call finds nothing.
        let content = if input["tool_choice"] == "none" {
            json!({ "status": "insufficient_evidence", "sentences": [], "confidence": 0.0, "not_found": ["unscripted"] }).to_string()
        } else {
            "READY".to_owned()
        };
        return json!({ "choices": [ { "message": { "role": "assistant", "content": content }, "finish_reason": "stop" } ] });
    }
    if model.contains("gpt-oss") {
        // Unscripted triage model: a valid classification in the JSON Mode shape.
        return json!({ "response": TRIAGE_DEFAULT.clone(), "usage": { "prompt_tokens": 900, "completion_tokens": 60 } });
    }
    if model.contains("rerank") {
        let q = tokens(input["query"].as_str().unwrap_or(""));
        let response: Vec<Value> = input["contexts"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let ct = tokens(c["text"].as_str().unwrap_or(""));
                let hit = q.iter().filter(|t| ct.contains(t)).count();
                let score = if q.is_empty() {
                    0.0
                } else {
                    hit as f64 / q.len() as f64
                };
                json!({ "id": i, "score": score })
            })
            .collect();
        return json!({ "response": response });
    }
    let texts: Vec<String> = match &input["text"] {
        Value::String(t) => vec![t.clone()],
        Value::Array(a) => a
            .iter()
            .map(|t| t.as_str().unwrap_or("").to_owned())
            .collect(),
        _ => Vec::new(),
    };
    let data: Vec<Vec<f64>> = texts.iter().map(|t| embed_text(t)).collect();
    json!({ "shape": [data.len(), 1024], "data": data })
}

fn cosine(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

fn matches(meta: &Value, filter: &Value) -> bool {
    let Some(f) = filter.as_object() else {
        return true;
    };
    f.iter().all(|(k, cond)| {
        let v = &meta[k];
        match cond {
            Value::Object(ops) => ops.iter().all(|(op, arg)| match op.as_str() {
                "$eq" => v == arg,
                "$ne" => v != arg,
                "$in" => arg.as_array().is_some_and(|a| a.contains(v)),
                "$nin" => arg.as_array().is_some_and(|a| !a.contains(v)),
                "$gte" | "$gt" | "$lte" | "$lt" => match (v.as_f64(), arg.as_f64()) {
                    (Some(x), Some(y)) => match op.as_str() {
                        "$gte" => x >= y,
                        "$gt" => x > y,
                        "$lte" => x <= y,
                        _ => x < y,
                    },
                    _ => false,
                },
                _ => false,
            }),
            other => v == other,
        }
    })
}

/// One stored vector, its values parsed once.
#[derive(Clone)]
struct Rec {
    namespace: String,
    values: Vec<f64>,
    metadata: Value,
}

fn values(v: &Value) -> Vec<f64> {
    v.as_array()
        .map(|a| a.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default()
}

fn vec_op(s: &mut State, binding: &str, op: &str, body: &Value) -> Value {
    if let Some(left) = s.vec_fail.get_mut(op).filter(|n| **n > 0) {
        *left -= 1;
        return json!({ "__status": 503 });
    }
    let fixed = s.vec_count;
    let keep = s.vec_keep.clone();
    let store = s.vectors.entry(binding.to_owned()).or_default();
    let query = |store: &BTreeMap<String, Rec>, vector: &[f64], options: &Value| {
        let ns = options["namespace"].as_str().unwrap_or("");
        let k = options["topK"].as_u64().unwrap_or(10) as usize;
        let mut m: Vec<(String, f64)> = store
            .iter()
            .filter(|(_, r)| r.namespace == ns && matches(&r.metadata, &options["filter"]))
            .map(|(id, r)| (id.clone(), cosine(vector, &r.values)))
            .collect();
        m.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        m.truncate(k);
        json!({ "count": m.len(), "matches": m.into_iter().map(|(id, score)| json!({ "id": id, "score": score })).collect::<Vec<_>>() })
    };
    match op {
        "upsert" => {
            for v in body["vectors"].as_array().cloned().unwrap_or_default() {
                if let Some(id) = v["id"].as_str() {
                    store.insert(
                        id.to_owned(),
                        Rec {
                            namespace: v["namespace"].as_str().unwrap_or("").to_owned(),
                            values: values(&v["values"]),
                            metadata: v["metadata"].clone(),
                        },
                    );
                }
            }
            json!({ "mutationId": "fake" })
        }
        "query" => query(store, &values(&body["vector"]), &body["options"]),
        "query_by_id" => match store.get(body["id"].as_str().unwrap_or("")) {
            Some(r) => {
                let v = r.values.clone();
                query(store, &v, &body["options"])
            }
            None => json!({ "count": 0, "matches": [] }),
        },
        "delete" => {
            for id in body["ids"].as_array().cloned().unwrap_or_default() {
                let id = id.as_str().unwrap_or("");
                if !keep.contains(id) {
                    store.remove(id);
                }
            }
            json!({ "mutationId": "fake" })
        }
        "get" => Value::Array(
            body["ids"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|id| id.as_str())
                .filter(|id| store.contains_key(*id))
                .map(|id| json!({ "id": id }))
                .collect(),
        ),
        "describe" => {
            let n = store.len() as f64;
            let count = fixed.unwrap_or(n as u64);
            json!({ "dimensions": 1024, "vectorCount": count })
        }
        _ => json!({}),
    }
}

/// `deleteByIds` leaves `id` in place until the next reset.
pub fn vector_keep(id: &str) {
    start();
    STATE.lock().unwrap().vec_keep.insert(id.to_owned());
}

/// What the unscripted triage model answers.
pub static TRIAGE_DEFAULT: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| {
    json!({ "category": "other", "needs_reply": 0.5, "urgency": 1,
        "summary": "Someone writes about a matter.", "language": "en", "risk_flags": [] })
});

/// Workers AI answers `statuses` (in order) to the next calls of models containing `model`.
pub fn ai_fail(model: &str, statuses: &[u16]) {
    start();
    STATE
        .lock()
        .unwrap()
        .ai_fail
        .push((model.to_owned(), statuses.to_vec()));
}

/// Scripts the next answers of models whose name contains `model` (a chat completion, or
/// `{"__status": n}`, or either with `"__delay_ms": n`).
pub fn ai_script(model: &str, answers: Vec<Value>) {
    start();
    let mut s = STATE.lock().unwrap();
    s.ai_script.retain(|(m, _)| m != model);
    s.ai_script.push((model.to_owned(), answers));
}

/// The inputs of the agent model's calls so far.
pub fn ai_inputs(model: &str) -> Vec<Value> {
    start();
    STATE
        .lock()
        .unwrap()
        .ai_inputs
        .iter()
        .filter(|(m, _)| m.contains(model))
        .map(|(_, i)| i.clone())
        .collect()
}

/// The stored vectors of `binding`: `id → { namespace, metadata }`.
pub fn vectors(binding: &str) -> BTreeMap<String, Value> {
    start();
    STATE
        .lock()
        .unwrap()
        .vectors
        .get(binding)
        .map(|store| {
            store
                .iter()
                .map(|(id, r)| {
                    (
                        id.clone(),
                        json!({ "namespace": r.namespace, "metadata": r.metadata }),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Removes a stored vector (as if Vectorize lost it).
pub fn vector_lose(binding: &str, id: &str) {
    start();
    if let Some(store) = STATE.lock().unwrap().vectors.get_mut(binding) {
        store.remove(id);
    }
}

/// `describe()` reports `count` vectors (`None`: the stored count).
pub fn vector_count(count: Option<u64>) {
    start();
    STATE.lock().unwrap().vec_count = count;
}

/// The next `n` calls of Vectorize operation `op` (`upsert`, `query`, `query_by_id`, `delete`, `get`,
/// `describe`) fail.
pub fn vector_fail(op: &str, n: u32) {
    start();
    STATE.lock().unwrap().vec_fail.insert(op.to_owned(), n);
}

/// Stores the vectors of bulk-seeded rows (`{id, thread_id, text, sent_at}`) as `Embed` would.
pub fn seed_vectors(namespace: &str, identity_id: &str, rows: &[Value]) {
    start();
    let mut s = STATE.lock().unwrap();
    let store = s.vectors.entry("VECTORS".to_owned()).or_default();
    for r in rows {
        let id = format!("{}:0", r["id"].as_str().unwrap_or(""));
        store.insert(
            id,
            Rec {
                namespace: namespace.to_owned(),
                values: embed_text(r["text"].as_str().unwrap_or("")),
                metadata: json!({
                    "identity_id": identity_id, "thread_id": r["thread_id"], "sent_at": r["sent_at"],
                    "sender_domain": "", "direction": "inbound", "has_attachment": false,
                    "verdict": "none", "kind": "body",
                }),
            },
        );
    }
}
