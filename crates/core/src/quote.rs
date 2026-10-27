//! Quote and signature stripping ([Inbound › Quote and signature stripping]): `extracted_text`, the new
//! content of a message (FR-IN-7).
//!
//! [Inbound › Quote and signature stripping]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use std::sync::OnceLock;

use regex::Regex;

use crate::sanitize::rcdom::{Handle, NodeData};
use crate::sanitize::{attr, tag};
use crate::text::derive_text_until;

/// The first HTML quote marker in document order: `div.gmail_quote`, `blockquote[type=cite]`,
/// `div#divRplyFwdMsg`, `div#appendonsend`, `hr#stopSpelling`, `div.yahoo_quoted`,
/// `div#mail-editor-reference-message-container`, `blockquote.protonmail_quote`.
pub fn html_marker(root: &Handle) -> Option<Handle> {
    let mut stack = vec![root.clone()];
    while let Some(n) = stack.pop() {
        if let NodeData::Element { .. } = &n.data {
            let t = tag(&n).unwrap_or("").to_ascii_lowercase();
            let id = attr(&n, "id").unwrap_or_default();
            let has_class = |c: &str| {
                attr(&n, "class")
                    .is_some_and(|cl| cl.split_whitespace().any(|x| x.eq_ignore_ascii_case(c)))
            };
            let hit = match t.as_str() {
                "div" => {
                    has_class("gmail_quote")
                        || has_class("yahoo_quoted")
                        || [
                            "divRplyFwdMsg",
                            "appendonsend",
                            "mail-editor-reference-message-container",
                        ]
                        .iter()
                        .any(|i| id.eq_ignore_ascii_case(i))
                }
                "blockquote" => {
                    attr(&n, "type").is_some_and(|v| v.eq_ignore_ascii_case("cite"))
                        || has_class("protonmail_quote")
                }
                "hr" => id.eq_ignore_ascii_case("stopSpelling"),
                _ => false,
            };
            if hit {
                return Some(n);
            }
        }
        stack.extend(n.children.borrow().iter().rev().cloned());
    }
    None
}

struct Patterns {
    header: Vec<Regex>,
    forward: Vec<Regex>,
    outlook_from: Regex,
    outlook_sent: Regex,
    outlook_to: Regex,
    signature: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        let re = |s: &str| Regex::new(&format!("(?i)^(?:{s})$")).expect("fixed pattern");
        let forward = [
            r"-{2,} ?Forwarded message ?-{2,}",
            r"Begin forwarded message:",
        ];
        let header = [
            r"On .{1,200} wrote:",
            r"-{2,} ?Original Message ?-{2,}",
            r"Am .{1,200} schrieb .{1,200}:",
            r"-{2,} ?Ursprüngliche Nachricht ?-{2,}",
            r"Le .{1,200} a écrit ?:",
            r"-{2,} ?Message d'origine ?-{2,}",
            r"El .{1,200} escribió:",
            r"-{2,} ?Mensaje original ?-{2,}",
            r"Il .{1,200} ha scritto:",
            r"-{2,} ?Messaggio originale ?-{2,}",
            r"Op .{1,200} schreef .{1,200}:",
            r"-{2,} ?Oorspronkelijk bericht ?-{2,}",
            r"Em .{1,200} escreveu:",
            r"Den .{1,200} skrev .{1,200}:",
            r".{1,200} skrev:",
            r".{1,200} napisał(?:a|\(a\))?:",
            r".{1,200} писал(?:а|\(а\))?:",
            r".{1,200}(さんは書きました|wrote)[:：]",
            r".{1,200}写道[:：]",
        ];
        Patterns {
            header: header.iter().map(|p| re(p)).collect(),
            forward: forward.iter().map(|p| re(p)).collect(),
            outlook_from: Regex::new(r"(?i)^(From|Von|De|Da|Van|Från|Od) ?:").expect("fixed"),
            outlook_sent: Regex::new(
                r"(?i)^(Sent|Date|Gesendet|Datum|Envoyé|Enviado|Inviato|Verzonden|Skickat|Wysłano|Enviada) ?:",
            )
            .expect("fixed"),
            outlook_to: Regex::new(
                r"(?i)^(To|Subject|An|Betreff|À|Objet|Para|Asunto|A|Oggetto|Aan|Onderwerp|Till|Ämne|Do|Temat|Assunto) ?:",
            )
            .expect("fixed"),
            signature: Regex::new(
                r"(?i)^(Sent from my (iPhone|iPad|Android|mobile device|phone)|Get Outlook for (iOS|Android)|Sent from (Mail|Outlook|Yahoo Mail) for|Von meinem (iPhone|iPad|Smartphone) gesendet|Envoyé de mon (iPhone|iPad)|Enviado desde mi (iPhone|iPad)|Inviato da (iPhone|iPad)|Verzonden (vanaf|met) mijn (iPhone|iPad))",
            )
            .expect("fixed"),
        }
    })
}

/// Where the quoted history starts.
#[derive(Debug, PartialEq, Eq)]
struct Cut {
    line: usize,
    forward: bool,
}

fn find_cut(lines: &[&str]) -> Option<Cut> {
    let p = patterns();
    let matches = |s: &str, set: &[Regex]| set.iter().any(|r| r.is_match(s.trim()));
    for i in 1..lines.len() {
        let joined = lines
            .get(i + 1)
            .map(|n| format!("{} {}", lines[i].trim(), n.trim()));
        let candidates = std::iter::once(lines[i].to_owned()).chain(joined);
        for c in candidates {
            if matches(&c, &p.forward) {
                return Some(Cut {
                    line: i,
                    forward: true,
                });
            }
            if matches(&c, &p.header) {
                return Some(Cut {
                    line: i,
                    forward: false,
                });
            }
        }
        if p.outlook_from.is_match(lines[i].trim()) {
            let window = &lines[i + 1..lines.len().min(i + 5)];
            if window.iter().any(|l| p.outlook_sent.is_match(l.trim()))
                && window.iter().any(|l| p.outlook_to.is_match(l.trim()))
            {
                let underscores = |l: &str| {
                    let t = l.trim();
                    t.len() >= 10 && t.bytes().all(|b| b == b'_')
                };
                let line = if i > 1 && underscores(lines[i - 1]) {
                    i - 1
                } else {
                    i
                };
                return Some(Cut {
                    line,
                    forward: false,
                });
            }
        }
    }
    None
}

/// Step 4: cut at a `-- ` delimiter in the last 15 non-blank lines, or a mobile signature in the last 5.
fn strip_signature(lines: &[&str]) -> usize {
    let non_blank: Vec<usize> = (0..lines.len())
        .filter(|&i| !lines[i].trim().is_empty())
        .collect();
    let tail = |n: usize| non_blank.iter().rev().take(n).copied().collect::<Vec<_>>();
    let last15 = tail(15);
    if let Some(&i) = last15
        .iter()
        .rev()
        .find(|&&i| lines[i] == "-- " || lines[i] == "--")
    {
        return i;
    }
    let p = patterns();
    if let Some(&i) = tail(5)
        .iter()
        .rev()
        .find(|&&i| p.signature.is_match(lines[i].trim()))
    {
        return i;
    }
    lines.len()
}

fn join(lines: &[&str]) -> String {
    lines.join("\n").trim().to_owned()
}

/// `extracted_text`: the new content of a message. `html` is the DOM after the sanitising DOM pass, when
/// the message has an HTML body; its quote markers win over the text algorithm.
pub fn extract_new_content(text: &str, html: Option<&Handle>) -> String {
    let source;
    let text = match html.and_then(|root| html_marker(root).map(|m| (root, m))) {
        Some((root, marker)) => {
            source = derive_text_until(root, Some(&marker));
            if !source.trim().is_empty() {
                let lines: Vec<&str> = source.lines().collect();
                let end = strip_signature(&lines);
                let out = join(&lines[..end]);
                if !out.is_empty() {
                    return out;
                }
            }
            text
        }
        None => text,
    };
    let normalised = text.replace("\r\n", "\n");
    let lines: Vec<&str> = normalised.lines().collect();
    let cut = find_cut(&lines);
    let head = &lines[..cut.as_ref().map_or(lines.len(), |c| c.line)];
    let unquoted: Vec<&str> = head
        .iter()
        .copied()
        .filter(|l| !l.trim_start().starts_with('>'))
        .collect();
    let end = strip_signature(&unquoted);
    let out = join(&unquoted[..end]);
    if !out.is_empty() {
        return out;
    }
    match cut {
        Some(Cut {
            line,
            forward: true,
        }) => join(&lines[line..]),
        _ => {
            let end = strip_signature(&lines);
            join(&lines[..end])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanitize;

    #[test]
    fn reply_headers_by_language() {
        for header in [
            "On Mon, 3 Mar 2026 at 10:00, Ana <ana@example.com> wrote:",
            "-----Original Message-----",
            "Am 03.03.2026 um 10:00 schrieb Ana Müller <ana@example.de>:",
            "Le lun. 3 mars 2026 à 10:00, Ana <ana@example.fr> a écrit :",
            "El lun, 3 mar 2026 a las 10:00, Ana (<ana@example.es>) escribió:",
            "Il giorno lun 3 mar 2026 alle ore 10:00 Ana <ana@example.it> ha scritto:",
            "Op ma 3 mrt 2026 om 10:00 schreef Ana <ana@example.nl>:",
            "Em seg., 3 de mar. de 2026 às 10:00, Ana <ana@example.pt> escreveu:",
            "Den mån 3 mars 2026 kl 10:00 skrev Ana <ana@example.se>:",
            "Ana Kowalska <ana@example.pl> napisał(a):",
            "Анна <ana@example.ru> писал(а):",
            "2026年3月3日(月) 10:00 Ana <ana@example.jp>さんは書きました:",
            "Ana <ana@example.cn> 于2026年3月3日周一 10:00写道：",
        ] {
            let text = format!("Thanks, booked.\n\n{header}\n> Can you book it?\n> Ana");
            assert_eq!(
                extract_new_content(&text, None),
                "Thanks, booked.",
                "{header}"
            );
        }
    }

    #[test]
    fn wrapped_header_outlook_block_and_inline_replies() {
        let wrapped = "Sounds good.\n\nOn Mon, 3 Mar 2026 at 10:00, Ana Long Name\n<ana@example.com> wrote:\n> old";
        assert_eq!(extract_new_content(wrapped, None), "Sounds good.");
        let outlook = "Approved.\n\n________________________________\nFrom: Ana <ana@example.com>\nSent: Monday, 3 March 2026 10:00\nTo: Desk <desk@example.com>\nSubject: Booking\n\nOld text";
        assert_eq!(extract_new_content(outlook, None), "Approved.");
        let inline = "Hi,\n> question one?\nAnswer one.\n> question two?\nAnswer two.";
        assert_eq!(
            extract_new_content(inline, None),
            "Hi,\nAnswer one.\nAnswer two."
        );
        // A header on the first line does not empty the reply.
        let first = "On second thought, wrote:\nI will come.";
        assert_eq!(extract_new_content(first, None), first);
    }

    #[test]
    fn signatures_and_fallbacks() {
        let sig = "See you then.\n\n-- \nAna Müller\nBrightwell Cars";
        assert_eq!(extract_new_content(sig, None), "See you then.");
        let mobile = "On my way.\n\nSent from my iPhone";
        assert_eq!(extract_new_content(mobile, None), "On my way.");
        let regards = "Thanks.\n\nKind regards,\nAna";
        assert_eq!(extract_new_content(regards, None), regards);
        // An inline forward with no comment keeps the forwarded text.
        let fwd = "\n---------- Forwarded message ---------\nFrom: Bo <bo@example.com>\nInvoice attached.";
        assert_eq!(
            extract_new_content(fwd, None),
            "---------- Forwarded message ---------\nFrom: Bo <bo@example.com>\nInvoice attached."
        );
        // Everything quoted: the text with only the signature step.
        let all = "x\n> quoted only\n-- \nsig";
        assert_eq!(extract_new_content(all, None), "x");
        let quoted = "> only quoted";
        assert_eq!(extract_new_content(quoted, None), "> only quoted");
    }

    #[test]
    fn html_markers_win() {
        let html = r#"<div dir="ltr">Yes, Friday works.<br></div><br><div class="gmail_quote"><div>On Mon Ana wrote:</div>
            <blockquote>Does Friday work?</blockquote></div>"#;
        let doc = sanitize::parse(html);
        let text = "Yes, Friday works.\n\nOn Mon Ana wrote:\n> Does Friday work?";
        assert_eq!(
            extract_new_content(text, Some(&doc.body())),
            "Yes, Friday works."
        );
        for marker in [
            r#"<blockquote type="cite">old</blockquote>"#,
            r#"<div id="divRplyFwdMsg">old</div>"#,
            r#"<div id="appendonsend"></div><div>old</div>"#,
            r#"<hr id="stopSpelling">old"#,
            r#"<div class="yahoo_quoted">old</div>"#,
            r#"<div id="mail-editor-reference-message-container">old</div>"#,
            r#"<blockquote class="protonmail_quote">old</blockquote>"#,
        ] {
            let doc = sanitize::parse(&format!("<p>New.</p>{marker}"));
            assert_eq!(
                extract_new_content("New.\nold", Some(&doc.body())),
                "New.",
                "{marker}"
            );
        }
    }
}
