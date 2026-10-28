//! The fake server on `127.0.0.1:8798` (Testing § 6.3–6.6), hosted by the test process: the fake clock
//! and the fault plan the Worker reads at each invocation, the queue messages the itest build hands
//! over, and the hosts the Worker's outbound requests are rerouted to (`/__http/{host}/…`): so far the
//! two DoH resolvers, answering from the corpus zones (`crates/conformance/corpus/dns/zones.toml`). A blocking server on `std::net`.

use std::collections::BTreeMap;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::sync::{Mutex, Once};

use serde_json::{Value, json};

struct State {
    offset: i64,
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
    faults: Vec::new(),
    queued: Vec::new(),
    txt: BTreeMap::new(),
    received: Vec::new(),
    statuses: BTreeMap::new(),
    mail_script: Vec::new(),
    mail_sent: Vec::new(),
    cf_suppressions: BTreeMap::new(),
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
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let (status, text) = match path.strip_prefix("/__http/").filter(|t| {
                    t.split(['/', '?']).next().is_some_and(|h| h.ends_with(RECEIVERS))
                }) {
                    Some(target) => (receive(target, headers, raw), String::new()),
                    None => {
                        let body: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
                        (200, handle(&path, &body).to_string())
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
        "/state" => json!({ "clock_offset_ms": s.offset, "faults_armed": !s.faults.is_empty() }),
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
            doh(&s, q.split_once('?').map_or("", |(_, q)| q))
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

/// Resets the clock and the faults (a guard in each test that changes them).
pub fn reset() {
    let mut s = STATE.lock().unwrap();
    s.offset = 0;
    s.faults.clear();
}

pub struct Reset;

impl Drop for Reset {
    fn drop(&mut self) {
        reset();
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
    start();
    let item = if scope == "account" {
        json!({ "email": address, "scope": "account", "reason": "hard_bounce" })
    } else {
        json!({ "email": address, "scope": "sending_domain", "domain": scope, "reason": "hard_bounce" })
    };
    STATE
        .lock()
        .unwrap()
        .cf_suppressions
        .entry(address.to_owned())
        .or_default()
        .push(item);
}
