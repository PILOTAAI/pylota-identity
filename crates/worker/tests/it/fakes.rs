//! The fake server on `127.0.0.1:8798` (Testing § 6.3–6.6), hosted by the test process: the fake clock
//! and the fault plan the Worker reads at each invocation, and the queue messages the itest build hands
//! over. A blocking server on `std::net`, enough for the hooks so far.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::sync::{Mutex, Once};

use serde_json::{Value, json};

struct State {
    offset: i64,
    faults: Vec<Value>,
    /// Messages the itest build's queue consumer handed over: `(queue, body)`.
    queued: Vec<(String, Value)>,
}

static STATE: Mutex<State> = Mutex::new(State {
    offset: 0,
    faults: Vec::new(),
    queued: Vec::new(),
});

pub fn start() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let listener = TcpListener::bind("127.0.0.1:8798").expect("port 8798 is free for the fake server");
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let answer = handle(&path, &body);
                let text = answer.to_string();
                let mut out = stream;
                let _ = write!(out, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}", text.len());
            }
        });
    });
}

fn handle(path: &str, body: &Value) -> Value {
    let mut s = STATE.lock().unwrap();
    match path {
        "/state" => json!({ "clock_offset_ms": s.offset }),
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
