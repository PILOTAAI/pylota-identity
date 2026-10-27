//! The `message/delivery-status` body of an RFC 3464 DSN ([Inbound › Automated mail and loops]): one
//! per-message block, then one block per recipient, blocks separated by blank lines, each a list of
//! header-style fields with continuation lines.
//!
//! [Inbound › Automated mail and loops]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use serde::Serialize;

/// At most this many recipients are kept.
pub const MAX_RECIPIENTS: usize = 100;
/// Each `diagnostic_code` is cut to this many characters.
pub const MAX_DIAGNOSTIC: usize = 512;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DsnRecipient {
    /// The address of `Final-Recipient` (its `rfc822;` type removed), normalised; `None` when it is not
    /// an address.
    pub final_recipient: Option<String>,
    /// `failed`, `delayed`, `delivered`, `relayed` or `expanded`, lower case.
    pub action: Option<String>,
    /// The status code, e.g. `5.1.1`.
    pub status: Option<String>,
    pub diagnostic_code: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Dsn {
    pub reporting_mta: Option<String>,
    pub original_envelope_id: Option<String>,
    /// From the returned message's headers, filled by the caller.
    pub original_message_id: Option<String>,
    pub original_from: Option<String>,
    pub recipients: Vec<DsnRecipient>,
    /// More recipients than [`MAX_RECIPIENTS`] were listed.
    pub truncated: bool,
}

/// Fields of one block, names lower-cased, continuation lines joined with a space.
fn fields(block: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in block.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    out
}

/// The value after `type;` (`rfc822; a@b` → `a@b`), trimmed.
fn typed(value: &str) -> String {
    value
        .split_once(';')
        .map_or(value, |(_, v)| v)
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_owned()
}

/// Parses the text of a `message/delivery-status` part.
pub fn parse(body: &str) -> Dsn {
    let normalised = body.replace("\r\n", "\n");
    let mut blocks = normalised.split("\n\n").filter(|b| !b.trim().is_empty());
    let mut dsn = Dsn::default();
    if let Some(first) = blocks.next() {
        for (name, value) in fields(first) {
            match name.as_str() {
                "reporting-mta" => dsn.reporting_mta = Some(typed(&value)),
                "original-envelope-id" => dsn.original_envelope_id = Some(value),
                _ => {}
            }
        }
    }
    for block in blocks {
        let mut r = DsnRecipient::default();
        for (name, value) in fields(block) {
            match name.as_str() {
                "final-recipient" => {
                    r.final_recipient = crate::mime::normalise_address(&typed(&value))
                }
                "original-recipient" if r.final_recipient.is_none() => {
                    r.final_recipient = crate::mime::normalise_address(&typed(&value));
                }
                "action" => r.action = Some(value.to_ascii_lowercase()),
                "status" => {
                    r.status = value.split_whitespace().next().map(str::to_owned);
                }
                "diagnostic-code" => {
                    r.diagnostic_code = Some(typed(&value).chars().take(MAX_DIAGNOSTIC).collect());
                }
                _ => {}
            }
        }
        if r == DsnRecipient::default() {
            continue;
        }
        if dsn.recipients.len() == MAX_RECIPIENTS {
            dsn.truncated = true;
            break;
        }
        dsn.recipients.push(r);
    }
    dsn
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3464_example() {
        let body = "Reporting-MTA: dns; mx.example.net\r\nOriginal-Envelope-Id: env-42\r\n\r\n\
                    Final-Recipient: rfc822; Ana@Brightwell.example\r\nAction: failed\r\nStatus: 5.1.1\r\n\
                    Diagnostic-Code: smtp; 550 5.1.1 <ana@brightwell.example>:\r\n  Recipient address rejected\r\n\r\n\
                    Original-Recipient: rfc822;bo@example.com\r\nAction: Delayed\r\nStatus: 4.4.1 (timeout)\r\n";
        let d = parse(body);
        assert_eq!(d.reporting_mta.as_deref(), Some("mx.example.net"));
        assert_eq!(d.original_envelope_id.as_deref(), Some("env-42"));
        assert_eq!(d.recipients.len(), 2);
        assert_eq!(
            d.recipients[0].final_recipient.as_deref(),
            Some("ana@brightwell.example")
        );
        assert_eq!(d.recipients[0].status.as_deref(), Some("5.1.1"));
        assert_eq!(
            d.recipients[0].diagnostic_code.as_deref(),
            Some("550 5.1.1 <ana@brightwell.example>: Recipient address rejected")
        );
        assert_eq!(d.recipients[1].action.as_deref(), Some("delayed"));
        assert_eq!(d.recipients[1].status.as_deref(), Some("4.4.1"));
    }

    #[test]
    fn caps_and_garbage() {
        let mut body = String::from("Reporting-MTA: dns; mx\n\n");
        for i in 0..150 {
            body.push_str(&format!(
                "Final-Recipient: rfc822; r{i}@x.example\nAction: failed\nDiagnostic-Code: {}\n\n",
                "x".repeat(900)
            ));
        }
        let d = parse(&body);
        assert_eq!(d.recipients.len(), 100);
        assert!(d.truncated);
        assert_eq!(d.recipients[0].diagnostic_code.as_ref().unwrap().len(), 512);
        assert_eq!(parse(""), Dsn::default());
        assert_eq!(parse("\n\n\n:::\n\n \n").recipients.len(), 0);
        let junk =
            parse("Reporting-MTA: x\n\nFinal-Recipient: rfc822; not an address\nAction: failed\n");
        assert_eq!(junk.recipients[0].final_recipient, None);
    }
}
