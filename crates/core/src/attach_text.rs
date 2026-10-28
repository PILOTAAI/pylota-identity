//! Attachment text ([Inbound › Attachment text extraction], FR-IN-8): the families decoded in Rust, the
//! page rule of the converter's Markdown, and the stored form with one marker line per page.
//!
//! [Inbound › Attachment text extraction]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use crate::mime::{self, Caps};
use crate::{sanitize, text};

/// Inputs above this size are not converted.
pub const MAX_INPUT_BYTES: usize = 20 * 1024 * 1024;
/// More pages than this, or more text, and the attachment is `unavailable`.
pub const MAX_PAGES: usize = 200;
pub const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;

/// How an attachment's text is obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Decoded in Rust: one page.
    Text,
    Html,
    Message,
    /// `AI.toMarkdown`.
    Converter,
    /// Not supported (DOC, PPT, PPTX, anything else): `unavailable`.
    None,
}

/// The method for a sniffed or declared type.
pub fn method(sniffed: &str, declared: &str) -> Method {
    let t = if sniffed.is_empty() || sniffed == "application/octet-stream" {
        declared
    } else {
        sniffed
    };
    let t = t.to_ascii_lowercase();
    match t.as_str() {
        "text/plain" | "text/csv" | "text/markdown" | "application/json" | "text/xml"
        | "application/xml" => Method::Text,
        "text/html" | "application/xhtml+xml" => Method::Html,
        "message/rfc822" => Method::Message,
        "application/pdf"
        | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "application/vnd.ms-excel.sheet.macroenabled.12"
        | "application/vnd.ms-excel.sheet.binary.macroenabled.12"
        | "application/vnd.ms-excel"
        | "application/vnd.oasis.opendocument.spreadsheet"
        | "application/vnd.oasis.opendocument.text" => Method::Converter,
        t if t.starts_with("image/") => Method::Converter,
        _ => Method::None,
    }
}

/// A text part: UTF-8 with replacement (a declared charset other than UTF-8 is decoded by the MIME
/// parser before the bytes are stored, so the stored bytes are what the part carried).
pub fn decode_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// An HTML part: sanitised, then its text.
pub fn html_text(bytes: &[u8]) -> String {
    let doc = sanitize::parse(&String::from_utf8_lossy(bytes));
    text::derive_text(&doc.body())
}

/// A nested message: a header block, its extracted text, and its attachments' filenames.
pub fn message_text(bytes: &[u8]) -> String {
    let parsed = mime::parse(bytes, &Caps::default());
    let content = crate::inbound::content(&parsed);
    let mut out = String::new();
    if let Some(f) = &parsed.from {
        out.push_str(&format!("From: {}\n", f.address));
    }
    if let Some(d) = parsed.date {
        out.push_str(&format!("Date: {}\n", crate::days::rfc3339(d)));
    }
    if let Some(s) = &parsed.subject {
        out.push_str(&format!("Subject: {}\n", mime::clean_display(s, 998)));
    }
    out.push('\n');
    out.push_str(&content.extracted_text);
    let names: Vec<String> = parsed
        .attachments
        .iter()
        .filter_map(|a| a.filename.as_deref().map(mime::sanitise_filename))
        .collect();
    if !names.is_empty() {
        out.push_str("\n\nAttachments: ");
        out.push_str(&names.join(", "));
    }
    out
}

/// The pages of the converter's Markdown. Primary rule: a line that is exactly `### Page N`, N
/// following the previous page, starts page N (a leading `# {filename}` and `## Contents` are dropped).
/// Fallback: a form feed is a page boundary; with neither, the whole text is page 1.
pub fn converter_pages(data: &str) -> Vec<String> {
    let mut pages: Vec<String> = Vec::new();
    let mut preamble = String::new();
    for line in data.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        let next = pages.len() + 1;
        if bare
            .strip_prefix("### Page ")
            .and_then(|n| n.parse::<usize>().ok())
            == Some(next)
        {
            pages.push(String::new());
            continue;
        }
        match pages.last_mut() {
            Some(p) => p.push_str(line),
            None => preamble.push_str(line),
        }
    }
    if pages.is_empty() {
        return data
            .split('\u{000C}')
            .map(|p| p.trim().to_owned())
            .collect();
    }
    pages.iter().map(|p| p.trim().to_owned()).collect()
}

/// The stored Markdown: hidden characters removed, a marker line before each page. `None` when over
/// the page or text limit (`unavailable`).
pub fn render(pages: &[String]) -> Option<(String, u32)> {
    if pages.len() > MAX_PAGES {
        return None;
    }
    let mut out = String::new();
    for (i, p) in pages.iter().enumerate() {
        let (clean, _) = sanitize::strip_hidden_chars(p);
        out.push_str(&format!("<!-- pm:page {} -->\n", i + 1));
        out.push_str(&clean);
        out.push('\n');
    }
    (out.len() <= MAX_TEXT_BYTES).then_some((out, pages.len() as u32))
}

/// The text of each page of a stored rendering (markers removed).
pub fn split_rendered(md: &str) -> Vec<String> {
    let mut pages: Vec<String> = Vec::new();
    for line in md.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        if bare.starts_with("<!-- pm:page ") && bare.ends_with(" -->") {
            pages.push(String::new());
        } else if let Some(p) = pages.last_mut() {
            p.push_str(line);
        }
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converter_headings_are_pages() {
        let md = "# invoice.pdf\n## Contents\n### Page 1\nINVOICE 88213\n### Page 3\nnot a page\n### Page 2\nTerms\n";
        assert_eq!(
            converter_pages(md),
            vec![
                "INVOICE 88213\n### Page 3\nnot a page".to_owned(),
                "Terms".to_owned()
            ]
        );
        assert_eq!(converter_pages("a\u{000C}b"), vec!["a", "b"]);
        assert_eq!(converter_pages("only"), vec!["only"]);
    }

    #[test]
    fn render_marks_pages_and_limits() {
        let (md, n) = render(&["one".into(), "two\u{200B}".into()]).unwrap();
        assert_eq!(md, "<!-- pm:page 1 -->\none\n<!-- pm:page 2 -->\ntwo\n");
        assert_eq!(n, 2);
        assert_eq!(split_rendered(&md), vec!["one\n", "two\n"]);
        assert!(render(&vec![String::new(); MAX_PAGES + 1]).is_none());
        assert!(render(&["x".repeat(MAX_TEXT_BYTES)]).is_none());
    }

    #[test]
    fn methods() {
        assert_eq!(
            method("application/pdf", "application/octet-stream"),
            Method::Converter
        );
        assert_eq!(method("", "text/csv"), Method::Text);
        assert_eq!(method("text/html", ""), Method::Html);
        assert_eq!(method("application/vnd.ms-powerpoint", ""), Method::None);
        assert_eq!(method("image/png", ""), Method::Converter);
        assert!(html_text(b"<p>Hello <b>there</b></p>").contains("Hello there"));
    }
}
