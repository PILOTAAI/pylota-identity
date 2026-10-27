//! Automated mail and loops ([Inbound › Automated mail and loops], FR-IN-6, B8, D6): the message kind,
//! the evidence of every signal, and the parsed DSN; plus the send-side rules that use them.
//!
//! [Inbound › Automated mail and loops]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use serde::Serialize;

use crate::mime::dsn::{self, Dsn};
use crate::mime::{Attachment, ParsedMessage};
use crate::policy::MAX_HOP;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Dsn,
    Mdn,
    List,
    Automated,
    Calendar,
    Normal,
}

impl Kind {
    /// `trust.automated`: agents must not auto-reply to these.
    pub fn automated(self) -> bool {
        matches!(self, Kind::Dsn | Kind::Mdn | Kind::List | Kind::Automated)
    }

    /// Consumer step 10a: a calendar object turns `normal` into `calendar`, nothing else.
    pub fn with_calendar(self) -> Kind {
        if self == Kind::Normal {
            Kind::Calendar
        } else {
            self
        }
    }
}

/// The disposition notification of an RFC 8098 MDN.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Mdn {
    pub original_message_id: Option<String>,
    pub disposition: Option<String>,
}

/// `automated_json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Classification {
    pub kind: Kind,
    pub automated: bool,
    /// Every signal that matched, in table order.
    pub evidence: Vec<&'static str>,
    /// `X-Pylota-Identity-Hop`, 0 when absent.
    pub hop: u32,
    pub dsn: Option<Dsn>,
    pub mdn: Option<Mdn>,
}

const AUTO_SUBJECTS: &[&str] = &[
    "out of office",
    "automatic reply:",
    "auto:",
    "autosvar:",
    "abwesenheitsnotiz",
    "réponse automatique",
];

const BOUNCE_SUBJECT: &[&str] = &[
    "undeliver",
    "delivery status notification",
    "mail delivery failed",
    "returned mail",
    "failure notice",
];

fn local(address: &str) -> &str {
    address.rsplit_once('@').map_or(address, |(l, _)| l)
}

fn find<'a>(p: &'a ParsedMessage, types: &[&str]) -> Option<&'a Attachment> {
    p.attachments
        .iter()
        .find(|a| types.contains(&a.content_type.as_str()))
}

/// Classifies a parsed message. `envelope_from` is the SMTP `MAIL FROM`; empty is the null sender.
pub fn classify(p: &ParsedMessage, envelope_from: &str) -> Classification {
    let h = &p.headers;
    let mut ev: Vec<&'static str> = Vec::new();
    let mut kind = None;
    let set = |k: Kind, kind: &mut Option<Kind>| {
        if kind.is_none() {
            *kind = Some(k);
        }
    };
    let subject = p.subject.as_deref().unwrap_or("").to_lowercase();
    let from_local = p.from.as_ref().map(|f| local(&f.address)).unwrap_or("");

    // 1. DSN
    let report = h.content_type == "multipart/report";
    let returned = find(
        p,
        &[
            "message/rfc822",
            "text/rfc822-headers",
            "message/global",
            "message/global-headers",
        ],
    );
    if report && h.report_type.as_deref() == Some("delivery-status") {
        ev.push("delivery_status_report");
        set(Kind::Dsn, &mut kind);
    }
    if matches!(from_local, "mailer-daemon" | "postmaster")
        && BOUNCE_SUBJECT.iter().any(|s| subject.contains(s))
        && returned.is_some()
    {
        ev.push("bounce_heuristic");
        set(Kind::Dsn, &mut kind);
    }
    // 2. MDN
    if report && h.report_type.as_deref() == Some("disposition-notification") {
        ev.push("disposition_report");
        set(Kind::Mdn, &mut kind);
    }
    // 3. List
    let precedence = h
        .precedence
        .as_deref()
        .map(|v| v.trim().to_ascii_lowercase());
    for (present, name) in [
        (h.list_id.is_some(), "list_id"),
        (h.list_unsubscribe.is_some(), "list_unsubscribe"),
        (h.list_post.is_some(), "list_post"),
        (precedence.as_deref() == Some("list"), "precedence_list"),
        (precedence.as_deref() == Some("bulk"), "precedence_bulk"),
    ] {
        if present {
            ev.push(name);
            set(Kind::List, &mut kind);
        }
    }
    // 4. Automated
    let suppress = h
        .x_auto_response_suppress
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let hop = h.hop.unwrap_or(0);
    for (present, name) in [
        (
            h.auto_submitted.as_deref().is_some_and(|v| {
                !v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("no")
            }),
            "auto_submitted",
        ),
        (h.x_autoreply, "x_autoreply"),
        (h.x_autorespond, "x_autorespond"),
        (
            suppress
                .split([',', ' '])
                .any(|t| t.trim() == "all" || t.trim() == "oof"),
            "x_auto_response_suppress",
        ),
        (
            matches!(precedence.as_deref(), Some("junk" | "auto_reply")),
            "precedence_auto",
        ),
        (
            AUTO_SUBJECTS.iter().any(|s| subject.starts_with(s)),
            "auto_reply_subject",
        ),
        (
            matches!(
                from_local,
                "noreply" | "no-reply" | "donotreply" | "do-not-reply"
            ),
            "noreply_sender",
        ),
        (
            envelope_from.trim().trim_matches(['<', '>']).is_empty(),
            "null_sender",
        ),
        (hop >= MAX_HOP, "hop_limit"),
    ] {
        if present {
            ev.push(name);
            set(Kind::Automated, &mut kind);
        }
    }
    if hop >= 1 {
        ev.push("pylota_agent");
    }
    if h.disposition_notification_to.is_some() {
        ev.push("mdn_requested");
    }

    let kind = kind.unwrap_or(Kind::Normal);
    let dsn = (kind == Kind::Dsn).then(|| read_dsn(p, returned));
    let mdn = (kind == Kind::Mdn).then(|| read_mdn(p));
    Classification {
        kind,
        automated: kind.automated(),
        evidence: ev,
        hop,
        dsn,
        mdn,
    }
}

fn read_dsn(p: &ParsedMessage, returned: Option<&Attachment>) -> Dsn {
    let mut d = find(
        p,
        &["message/delivery-status", "message/global-delivery-status"],
    )
    .map(|a| dsn::parse(&String::from_utf8_lossy(&a.data)))
    .unwrap_or_default();
    if let Some(r) = returned {
        match &r.nested {
            Some(n) => {
                d.original_message_id = n.message_id.clone();
                d.original_from = n.from.as_ref().map(|f| f.address.clone());
            }
            None => {
                let headers = crate::mime::parse_headers_only(&r.data);
                d.original_message_id = (!headers.has(crate::mime::Flag::MessageIdSynthetic))
                    .then_some(headers.message_id);
                d.original_from = headers.from.map(|f| f.address);
            }
        }
    }
    d
}

fn read_mdn(p: &ParsedMessage) -> Mdn {
    let Some(part) = find(
        p,
        &[
            "message/disposition-notification",
            "message/global-disposition-notification",
        ],
    ) else {
        return Mdn::default();
    };
    let text = String::from_utf8_lossy(&part.data);
    let mut m = Mdn::default();
    for line in text.lines() {
        if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "original-message-id" => {
                    m.original_message_id = crate::thread::normalise_msg_id(value)
                }
                "disposition" => m.disposition = Some(value.trim().to_ascii_lowercase()),
                _ => {}
            }
        }
    }
    m
}

/// Step 10 of the send pipeline (D6): an auto-reply only answers `normal` or `calendar` mail and stays
/// under the per-thread cap of automatic exchanges.
pub fn auto_reply_allowed(target: Kind, automatic_exchanges: u32, cap: u32) -> bool {
    matches!(target, Kind::Normal | Kind::Calendar) && automatic_exchanges < cap
}

/// The `X-Pylota-Identity-Hop` of a send: 1 for a new send, else 1 + the hop of the message answered.
/// `Err(hop)` when it would reach [`MAX_HOP`] (`409 loop_detected`, `details.hop`).
pub fn outbound_hop(previous: Option<u32>) -> Result<u32, u32> {
    let hop = previous.map_or(1, |h| h.saturating_add(1));
    if hop >= MAX_HOP { Err(hop) } else { Ok(hop) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mime::{Caps, parse};

    fn c(raw: &str, envelope_from: &str) -> Classification {
        classify(&parse(raw.as_bytes(), &Caps::default()), envelope_from)
    }

    const PLAIN: &str = "From: Ana <ana@example.com>\r\nSubject: Booking\r\n\r\nHello";

    #[test]
    fn b8_mdn_request_ignored() {
        let with_request = format!("Disposition-Notification-To: ana@example.com\r\n{PLAIN}");
        let r = c(&with_request, "ana@example.com");
        assert_eq!(r.kind, Kind::Normal);
        assert!(!r.automated);
        assert_eq!(r.evidence, vec!["mdn_requested"]);

        let mdn = "From: ana@example.com\r\nSubject: Read: Booking\r\nContent-Type: multipart/report; report-type=disposition-notification; boundary=b\r\n\r\n\
                   --b\r\nContent-Type: text/plain\r\n\r\nRead.\r\n--b\r\nContent-Type: message/disposition-notification\r\n\r\n\
                   Reporting-UA: x\r\nOriginal-Message-ID: <M1@Agents.Example>\r\nDisposition: manual-action/MDN-sent-manually; displayed\r\n--b--\r\n";
        let r = c(mdn, "ana@example.com");
        assert_eq!(r.kind, Kind::Mdn);
        assert!(r.automated);
        let m = r.mdn.unwrap();
        assert_eq!(m.original_message_id.as_deref(), Some("M1@agents.example"));
        assert_eq!(
            m.disposition.as_deref(),
            Some("manual-action/mdn-sent-manually; displayed")
        );
    }

    #[test]
    fn d6_dsn() {
        let raw = "From: Mail Delivery System <MAILER-DAEMON@mx.example.net>\r\nSubject: Undelivered Mail Returned to Sender\r\n\
                   Content-Type: multipart/report; report-type=delivery-status; boundary=b\r\n\r\n\
                   --b\r\nContent-Type: text/plain\r\n\r\nSorry.\r\n\
                   --b\r\nContent-Type: message/delivery-status\r\n\r\nReporting-MTA: dns; mx.example.net\r\n\r\n\
                   Final-Recipient: rfc822; gone@example.net\r\nAction: failed\r\nStatus: 5.1.1\r\n\r\n\
                   --b\r\nContent-Type: text/rfc822-headers\r\n\r\nFrom: desk@agents.example\r\nMessage-ID: <abc@Agents.Example>\r\nSubject: Hi\r\n\r\n\
                   --b--\r\n";
        let r = c(raw, "");
        assert_eq!(r.kind, Kind::Dsn);
        assert_eq!(
            r.evidence[..2],
            ["delivery_status_report", "bounce_heuristic"]
        );
        assert!(r.evidence.contains(&"null_sender"));
        let d = r.dsn.unwrap();
        assert_eq!(d.reporting_mta.as_deref(), Some("mx.example.net"));
        assert_eq!(
            d.recipients[0].final_recipient.as_deref(),
            Some("gone@example.net")
        );
        assert_eq!(d.original_message_id.as_deref(), Some("abc@agents.example"));
        assert_eq!(d.original_from.as_deref(), Some("desk@agents.example"));
    }

    #[test]
    fn d6_lists_and_auto_replies() {
        let list = format!("List-Id: <ops.example.com>\r\nPrecedence: bulk\r\n{PLAIN}");
        let r = c(&list, "bounces@example.com");
        assert_eq!(r.kind, Kind::List);
        assert_eq!(r.evidence, vec!["list_id", "precedence_bulk"]);

        for header in [
            "Auto-Submitted: auto-replied",
            "Auto-Submitted: auto-generated",
            "Auto-Submitted: auto-notified",
            "X-Autoreply: yes",
            "X-Autorespond: yes",
            "X-Auto-Response-Suppress: OOF, AutoReply",
            "X-Auto-Response-Suppress: All",
            "Precedence: junk",
            "Precedence: auto_reply",
            "X-Pylota-Identity-Hop: 10",
        ] {
            let r = c(&format!("{header}\r\n{PLAIN}"), "ana@example.com");
            assert_eq!(r.kind, Kind::Automated, "{header}");
        }
        assert_eq!(
            c(&format!("Auto-Submitted: no\r\n{PLAIN}"), "a@x.example").kind,
            Kind::Normal
        );
        for subject in [
            "Out of Office: away",
            "Automatic reply: Booking",
            "Auto: x",
            "Autosvar: x",
            "Abwesenheitsnotiz: x",
            "Réponse automatique : x",
        ] {
            let raw = format!("From: ana@example.com\r\nSubject: {subject}\r\n\r\nx");
            assert_eq!(
                c(&raw, "ana@example.com").kind,
                Kind::Automated,
                "{subject}"
            );
        }
        let raw = "From: No Reply <no-reply@example.com>\r\nSubject: Receipt\r\n\r\nx";
        assert_eq!(
            c(raw, "bounce@example.com").evidence,
            vec!["noreply_sender"]
        );
        // Our own agent mail below the limit is evidence only.
        let r = c(
            &format!("X-Pylota-Identity-Hop: 3\r\n{PLAIN}"),
            "a@x.example",
        );
        assert_eq!((r.kind, r.hop), (Kind::Normal, 3));
        assert_eq!(r.evidence, vec!["pylota_agent"]);
        assert_eq!(Kind::Normal.with_calendar(), Kind::Calendar);
        assert_eq!(Kind::List.with_calendar(), Kind::List);
    }

    #[test]
    fn d6_send_rules() {
        assert!(auto_reply_allowed(Kind::Normal, 0, 2));
        assert!(auto_reply_allowed(Kind::Calendar, 1, 2));
        assert!(!auto_reply_allowed(Kind::Normal, 2, 2));
        for k in [Kind::Dsn, Kind::Mdn, Kind::List, Kind::Automated] {
            assert!(!auto_reply_allowed(k, 0, 2), "{k:?}");
        }
        assert_eq!(outbound_hop(None), Ok(1));
        assert_eq!(outbound_hop(Some(0)), Ok(1));
        assert_eq!(outbound_hop(Some(8)), Ok(9));
        assert_eq!(outbound_hop(Some(9)), Err(10));
        assert_eq!(outbound_hop(Some(u32::MAX)), Err(u32::MAX));
    }
}
