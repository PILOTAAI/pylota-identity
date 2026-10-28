//! The one builder of a message's keyword-index row ([Search §5.1], analyzer version 1): ingest, the
//! attachment-text update, reindex and erasure all call it.
//!
//! [Search §5.1]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use crate::refs::plate_display;

/// At most this much text per attachment, and per message, in the `attachments` column.
pub const ATTACHMENT_TEXT_BYTES: usize = 256 * 1024;
pub const MESSAGE_ATTACHMENT_BYTES: usize = 1024 * 1024;

/// What the builder reads from a message.
pub struct MessageForIndex<'a> {
    pub subject: &'a str,
    pub from_name: &'a str,
    pub from_address: &'a str,
    /// Every `to` and `cc` entry: `(name, address)`.
    pub recipients: &'a [(String, String)],
    pub delivered_to: &'a str,
    pub extracted_text: &'a str,
    pub text: &'a str,
    /// For each attachment, in order: filename and its extracted text (empty until ready).
    pub attachments: &'a [(String, String)],
    /// Normalised reference values.
    pub refs: &'a [String],
}

/// The six column values of `fts` (`fts_tri` takes `subject`, `participants` and `refs`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FtsDoc {
    pub subject: String,
    pub participants: String,
    pub body_new: String,
    pub body_full: String,
    pub attachments: String,
    pub refs: String,
}

fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn join(parts: impl IntoIterator<Item = String>) -> String {
    parts
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn fts_doc(m: &MessageForIndex<'_>) -> FtsDoc {
    let mut participants = vec![m.from_name.to_owned(), m.from_address.to_owned()];
    for (n, a) in m.recipients {
        participants.push(n.clone());
        participants.push(a.clone());
    }
    participants.push(m.delivered_to.to_owned());
    let mut attachments = String::new();
    for (name, text) in m.attachments {
        let piece = format!("{name} {}", cut(text, ATTACHMENT_TEXT_BYTES));
        if attachments.len() + piece.len() + 1 > MESSAGE_ATTACHMENT_BYTES {
            attachments.push(' ');
            attachments.push_str(cut(
                &piece,
                MESSAGE_ATTACHMENT_BYTES.saturating_sub(attachments.len()),
            ));
            break;
        }
        if !attachments.is_empty() {
            attachments.push(' ');
        }
        attachments.push_str(&piece);
    }
    let mut refs: Vec<String> = Vec::new();
    for r in m.refs {
        refs.push(r.clone());
        if let Some(d) = plate_display(r) {
            refs.push(d);
        }
    }
    FtsDoc {
        subject: m.subject.to_owned(),
        participants: join(participants),
        body_new: m.extracted_text.to_owned(),
        body_full: m.text.to_owned(),
        attachments: attachments.trim().to_owned(),
        refs: join(refs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_follow_the_table() {
        let recipients = vec![("Desk".to_owned(), "desk.acme@agents.example".to_owned())];
        let atts = vec![("invoice.pdf".to_owned(), "INVOICE 88213".to_owned())];
        let refs = vec!["AB12CDE".to_owned(), "88213".to_owned()];
        let d = fts_doc(&MessageForIndex {
            subject: "Re: Booking",
            from_name: "Sam",
            from_address: "sam@acme.example",
            recipients: &recipients,
            delivered_to: "desk.acme@agents.example",
            extracted_text: "new",
            text: "new\n> old",
            attachments: &atts,
            refs: &refs,
        });
        assert_eq!(d.subject, "Re: Booking");
        assert_eq!(
            d.participants,
            "Sam sam@acme.example Desk desk.acme@agents.example desk.acme@agents.example"
        );
        assert_eq!(d.attachments, "invoice.pdf INVOICE 88213");
        assert!(d.refs.starts_with("AB12CDE AB12 CDE"), "{}", d.refs);
        assert_eq!(
            (d.body_new.as_str(), d.body_full.as_str()),
            ("new", "new\n> old")
        );
    }
}
