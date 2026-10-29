//! A server-sent events reader ([Search §11.11]): `id:`, `event:` and `data:` lines, a blank line ends an
//! event, and lines starting with `:` (keep-alives) are ignored. `data` is parsed as JSON.
//!
//! [Search §11.11]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md#1111-streaming

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub id: Option<String>,
    pub event: String,
    pub data: Value,
}

/// Feeds bytes in and takes complete events out.
#[derive(Default)]
pub struct Parser {
    buf: Vec<u8>,
    id: Option<String>,
    event: Option<String>,
    data: Vec<String>,
}

impl Parser {
    /// The events completed by `bytes`.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(i) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=i).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1])
                .trim_end_matches('\r')
                .to_owned();
            if line.is_empty() {
                if let Some(e) = self.finish() {
                    out.push(e);
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = match line.split_once(':') {
                Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
                None => (line.as_str(), ""),
            };
            match field {
                "id" => self.id = Some(value.to_owned()),
                "event" => self.event = Some(value.to_owned()),
                "data" => self.data.push(value.to_owned()),
                _ => {}
            }
        }
        out
    }

    fn finish(&mut self) -> Option<Event> {
        if self.data.is_empty() && self.event.is_none() {
            return None;
        }
        let text = self.data.join("\n");
        self.data.clear();
        Some(Event {
            id: self.id.take(),
            event: self.event.take().unwrap_or_else(|| "message".to_owned()),
            data: serde_json::from_str(&text).unwrap_or(Value::String(text)),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// Events split across chunks, keep-alives and CRLF line ends.
    #[test]
    fn parses_chunks() {
        let mut p = super::Parser::default();
        assert!(p.push(b": keep-alive\n\nid: 1\nevent: st").is_empty());
        let e = p.push(
            b"ep\r\ndata: {\"step\":1}\r\n\r\nevent: done\ndata: {\"status\":\"answered\"}\n\n",
        );
        assert_eq!(e.len(), 2);
        assert_eq!(
            (e[0].id.as_deref(), e[0].event.as_str()),
            (Some("1"), "step")
        );
        assert_eq!(e[0].data, json!({ "step": 1 }));
        assert_eq!(e[1].data["status"], "answered");
    }
}
