//! Server-sent events of an agentic search ([Search § 11.11]): numbered `evidence`, `step`, `answer`
//! and `done` events, a `: keep-alive` comment after 10 seconds of silence, and the client's disconnect
//! seen at the next write.
//!
//! [Search § 11.11]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use std::cell::Cell;

use pylota_identity_platform::stream::StreamSink;
use pylota_identity_platform::{Clock as _, Platform};
use serde_json::Value;

/// Silence before a keep-alive comment; one second in the itest build, so the suite sees one.
pub fn keep_alive_ms() -> u32 {
    if cfg!(feature = "itest-hooks") {
        1_000
    } else {
        10_000
    }
}

pub struct Sse {
    sink: StreamSink,
    seq: Cell<u32>,
    last_write: Cell<i64>,
    /// The client is still reading.
    alive: Cell<bool>,
    /// The `done` event went out.
    done: Cell<bool>,
}

/// One event's wire form.
pub fn event(id: u32, name: &str, data: &Value) -> String {
    format!("id: {id}\nevent: {name}\ndata: {data}\n\n")
}

impl Sse {
    pub fn new(sink: StreamSink, now: i64) -> Self {
        Self {
            sink,
            seq: Cell::new(0),
            last_write: Cell::new(now),
            alive: Cell::new(true),
            done: Cell::new(false),
        }
    }

    pub fn alive(&self) -> bool {
        self.alive.get()
    }

    pub fn finished(&self) -> bool {
        self.done.get() || !self.alive.get()
    }

    async fn write<P: Platform>(&self, p: &P, text: &str) {
        if !self.alive.get() {
            return;
        }
        if !self.sink.write(text.as_bytes()).await {
            self.alive.set(false);
        }
        self.last_write.set(p.clock().now_ms());
    }

    pub async fn send<P: Platform>(&self, p: &P, name: &str, data: &Value) {
        let id = self.seq.get() + 1;
        self.seq.set(id);
        self.write(p, &event(id, name, data)).await;
    }

    /// The last event; the stream ends.
    pub async fn done<P: Platform>(&self, p: &P, data: &Value) {
        self.send(p, "done", data).await;
        self.done.set(true);
        self.sink.close().await;
    }

    /// Writes `: keep-alive` whenever the stream has been silent for the interval, until `done`.
    pub async fn keep_alive<P: Platform>(&self, p: &P) {
        // The fake clock advances on every sleep: natively the keep-alive would spend the run's budget.
        if cfg!(not(target_arch = "wasm32")) {
            return;
        }
        let every = keep_alive_ms();
        while !self.finished() {
            p.clock().sleep(every / 4).await;
            crate::search::join::yield_now().await;
            if self.finished() {
                break;
            }
            if p.clock().now_ms() - self.last_write.get() >= i64::from(every) {
                self.write(p, ": keep-alive\n\n").await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wire_form() {
        assert_eq!(
            event(3, "step", &json!({ "step": 1, "action": "search" })),
            "id: 3\nevent: step\ndata: {\"action\":\"search\",\"step\":1}\n\n"
        );
    }
}
