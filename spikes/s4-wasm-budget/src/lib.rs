//! Spike S4's Worker (build plan M1): parse a large message the way inbound will, in the deployed runtime,
//! and report what it took. CPU and wall time come from the Worker's trace events (`wrangler tail`), because
//! the clock inside a Worker does not advance during computation; memory is the wasm linear memory after the
//! request.
//!
//! `POST /parse` with the raw message as the body. The work per request: parse with `mail-parser`, walk
//! every part, decode every attachment, sanitise the HTML body with `ammonia`, hash the whole message with
//! SHA-256 (the dominant cost of DKIM verification on a large message: the body hash), and build a short
//! reply with `mail-builder`.
#![forbid(unsafe_code)]

/// What one parse found; serialised as the response.
pub fn parse_report(raw: &[u8]) -> serde_json::Value {
    use sha2::Digest as _;

    let Some(message) = mail_parser::MessageParser::default().parse(raw) else {
        return serde_json::json!({ "parsed": false, "raw_len": raw.len() });
    };
    let mut attachment_bytes = 0usize;
    for attachment in message.attachments() {
        attachment_bytes += attachment.contents().len();
    }
    let html = message
        .body_html(0)
        .map(|h| h.into_owned())
        .unwrap_or_default();
    let clean = ammonia::clean(&html);
    let hash = sha2::Sha256::digest(raw);
    let reply = mail_builder::MessageBuilder::new()
        .from(("Spike S4", "s4@pylotamail.com"))
        .to("someone@example.com")
        .subject("Re: S4")
        // mail-builder 1.0.0 calls SystemTime::now() for a missing Date, Message-ID or boundary, which
        // panics on wasm32-unknown-unknown (finding for M9: always supply them).
        .date(1_793_000_000u64)
        .message_id("s4-reply@pylotamail.com")
        .text_body("parsed")
        .write_to_vec()
        .map(|v| v.len())
        .unwrap_or(0);
    serde_json::json!({
        "parsed": true,
        "raw_len": raw.len(),
        "parts": message.parts.len(),
        "attachments": message.attachment_count(),
        "attachment_bytes": attachment_bytes,
        "html_len": html.len(),
        "clean_len": clean.len(),
        "sha256_first_byte": hash[0],
        "reply_len": reply,
        "subject": message.subject().unwrap_or_default(),
    })
}

#[cfg(target_arch = "wasm32")]
mod entry {
    use pylota_identity_platform::cf::glue::{
        CfPlatform, HttpRequestIn, HttpResponseOut, WorkerApp,
    };
    use pylota_identity_platform::cf::wasm_memory_bytes;

    pub struct App;

    impl WorkerApp for App {
        fn new(_platform: CfPlatform) -> Self {
            App
        }

        async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
            let before = wasm_memory_bytes();
            let mut report = if req.method == "POST" {
                super::parse_report(&req.body.map(|b| b.to_vec()).unwrap_or_default())
            } else {
                serde_json::json!({ "hello": "s4" })
            };
            report["wasm_memory_before"] = before.into();
            report["wasm_memory_after"] = wasm_memory_bytes().into();
            HttpResponseOut::Http {
                status: 200,
                headers: vec![("content-type".to_owned(), "application/json".to_owned())],
                body: report.to_string().into_bytes(),
            }
        }
    }

    pylota_identity_platform::export_worker! {
        app: crate::entry::App,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_a_small_message() {
        let raw = b"From: a@example.com\r\nTo: b@example.com\r\nSubject: hi\r\n\r\nbody\r\n";
        let report = super::parse_report(raw);
        assert_eq!(report["parsed"], true);
        assert_eq!(report["subject"], "hi");
    }
}
