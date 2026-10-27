//! Text derived from HTML ([Inbound › Text derivation], B4): HTML-only mail always gets a `text`.
//!
//! [Inbound › Text derivation]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use std::rc::Rc;

use crate::sanitize::rcdom::{Handle, NodeData};
use crate::sanitize::{attr, tag};

/// Walks `root` (a DOM after the sanitising DOM pass) and renders its text.
pub fn derive_text(root: &Handle) -> String {
    derive_text_until(root, None)
}

/// As [`derive_text`], stopping at `stop` (a quote marker): it and everything after it in document
/// order are left out.
pub fn derive_text_until(root: &Handle, stop: Option<&Handle>) -> String {
    let mut w = Writer::default();
    w.walk(root, stop, 0);
    w.finish()
}

#[derive(Default)]
struct Writer {
    lines: Vec<String>,
    cur: String,
    pending_space: bool,
    quote_depth: usize,
    pre: usize,
    stopped: bool,
    /// Per open list: `None` for `ul`, the next number for `ol`.
    lists: Vec<Option<i64>>,
    /// Cells written in the current table row.
    cells: Vec<usize>,
}

const SKIP: &[&str] = &[
    "script", "style", "head", "title", "noscript", "template", "iframe", "object", "embed",
    "applet", "svg", "math", "select", "textarea", "button",
];

fn is_block(t: &str) -> bool {
    matches!(
        t,
        "p" | "div"
            | "br"
            | "li"
            | "tr"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "pre"
            | "table"
            | "hr"
            | "ul"
            | "ol"
            | "dl"
            | "dt"
            | "dd"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "center"
            | "figure"
            | "address"
    )
}

impl Writer {
    fn text(&mut self, s: &str) {
        if self.pre > 0 {
            for (i, part) in s.split('\n').enumerate() {
                if i > 0 {
                    self.newline();
                }
                self.cur.push_str(part);
            }
            return;
        }
        for c in s.chars() {
            if c.is_whitespace() {
                if !self.cur.is_empty() {
                    self.pending_space = true;
                }
            } else {
                if self.pending_space {
                    self.cur.push(' ');
                    self.pending_space = false;
                }
                self.cur.push(c);
            }
        }
    }

    /// Text that starts a token (a list marker, a cell separator): no pending space before it.
    fn raw(&mut self, s: &str) {
        self.pending_space = false;
        self.cur.push_str(s);
    }

    fn newline(&mut self) {
        let prefix = "> ".repeat(self.quote_depth);
        let line = std::mem::take(&mut self.cur);
        self.lines.push(if line.is_empty() && !prefix.is_empty() {
            prefix.trim_end().to_owned()
        } else {
            format!("{prefix}{}", line.trim_end())
        });
        self.pending_space = false;
    }

    /// Ends the current line, if any.
    fn end_line(&mut self) {
        if !self.cur.is_empty() {
            self.newline();
        }
    }

    /// Ends the line and adds one blank line, unless the text so far (at this quote depth) already
    /// ends in one.
    fn blank(&mut self) {
        self.end_line();
        let prefix = "> ".repeat(self.quote_depth);
        let prefix = prefix.trim_end();
        if self
            .lines
            .last()
            .is_some_and(|l| !l.trim_start_matches('>').trim().is_empty() && l.starts_with(prefix))
        {
            self.newline();
        }
    }

    fn walk(&mut self, node: &Handle, stop: Option<&Handle>, depth: usize) {
        if self.stopped || depth > crate::sanitize::MAX_DOM_DEPTH {
            return;
        }
        if stop.is_some_and(|s| Rc::ptr_eq(s, node)) {
            self.stopped = true;
            return;
        }
        match &node.data {
            NodeData::Text { contents } => {
                let t = contents.borrow().to_string();
                self.text(&t);
            }
            NodeData::Element { .. } => {
                let t = tag(node).unwrap_or("").to_ascii_lowercase();
                if SKIP.contains(&t.as_str()) {
                    return;
                }
                self.open(node, &t);
                if t == "a" {
                    self.link(node, stop, depth);
                } else {
                    self.children(node, stop, depth);
                }
                self.close(&t);
            }
            _ => self.children(node, stop, depth),
        }
    }

    fn children(&mut self, node: &Handle, stop: Option<&Handle>, depth: usize) {
        let children: Vec<Handle> = node.children.borrow().clone();
        for c in &children {
            self.walk(c, stop, depth + 1);
        }
    }

    fn open(&mut self, node: &Handle, t: &str) {
        match t {
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => self.blank(),
            "blockquote" => {
                self.end_line();
                self.quote_depth += 1;
            }
            "pre" => {
                self.end_line();
                self.pre += 1;
            }
            "ul" => {
                self.end_line();
                self.lists.push(None);
            }
            "ol" => {
                self.end_line();
                let start = attr(node, "start")
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(1);
                self.lists.push(Some(start));
            }
            "li" => {
                self.end_line();
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => "- ".to_owned(),
                };
                self.raw(&marker);
            }
            "tr" => {
                self.end_line();
                self.cells.push(0);
            }
            "td" | "th" => {
                if let Some(n) = self.cells.last_mut() {
                    *n += 1;
                    if *n > 1 {
                        self.raw(" | ");
                    }
                }
            }
            "img" => {
                if let Some(alt) = attr(node, "alt").map(|a| a.trim().to_owned())
                    && !alt.is_empty()
                {
                    if self.pending_space || self.cur.ends_with(|c: char| !c.is_whitespace()) {
                        self.pending_space = !self.cur.is_empty();
                    }
                    self.text(&format!("[image: {alt}]"));
                }
            }
            "br" => self.newline(),
            "hr" => {
                self.end_line();
            }
            t if is_block(t) => self.end_line(),
            _ => {}
        }
    }

    fn close(&mut self, t: &str) {
        match t {
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => self.blank(),
            "blockquote" => {
                self.end_line();
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            "pre" => {
                self.end_line();
                self.pre = self.pre.saturating_sub(1);
            }
            "ul" | "ol" => {
                self.end_line();
                self.lists.pop();
            }
            "tr" => {
                self.end_line();
                self.cells.pop();
            }
            "br" | "img" | "td" | "th" => {}
            t if is_block(t) => self.end_line(),
            _ => {}
        }
    }

    /// `text (url)` when the URL differs from the text and its scheme is http, https or mailto.
    fn link(&mut self, node: &Handle, stop: Option<&Handle>, depth: usize) {
        let start_line = self.lines.len();
        let start = self.cur.len();
        self.children(node, stop, depth);
        let Some(href) = attr(node, "href").map(|h| h.trim().to_owned()) else {
            return;
        };
        let lower = href.to_ascii_lowercase();
        if !(lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("mailto:"))
        {
            return;
        }
        let text = if self.lines.len() == start_line {
            self.cur.get(start..).unwrap_or("").trim().to_owned()
        } else {
            String::new()
        };
        let norm = |s: &str| {
            s.trim()
                .trim_start_matches("mailto:")
                .trim_end_matches('/')
                .to_ascii_lowercase()
        };
        if norm(&text) != norm(&href)
            && norm(&text) != norm(lower.split_once("://").map_or("", |(_, r)| r))
        {
            if text.is_empty() {
                self.text(&href);
            } else {
                self.text(&format!(" ({href})"));
            }
        }
    }

    fn finish(mut self) -> String {
        self.end_line();
        let mut out: Vec<&str> = Vec::new();
        let mut blanks = 0;
        for l in &self.lines {
            if l.trim().is_empty() {
                blanks += 1;
                if blanks > 2 {
                    continue;
                }
            } else {
                blanks = 0;
            }
            out.push(l);
        }
        out.join("\n").trim().to_owned()
    }
}

/// `snippet`: the first 240 characters of `extracted_text`, whitespace collapsed.
pub fn snippet(extracted_text: &str) -> String {
    extracted_text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanitize;

    fn render(html: &str) -> String {
        let doc = sanitize::parse(html);
        derive_text(&doc.body())
    }

    /// Covers: B4
    #[test]
    fn b4_html_only() {
        let html = r#"<h1>Booking  confirmed</h1><p>Hello <b>Ana</b>,<br>your car is
            ready.</p><ul><li>Pick-up: 9:00</li><li>Return: 18:00</li></ul>
            <ol start="3"><li>three</li><li>four</li></ol>
            <blockquote><p>quoted line</p><p>second</p></blockquote>
            <p><a href="https://example.com/b/42">View booking</a> or
            <a href="https://example.com">https://example.com</a> or
            <a href="mailto:desk@example.com">desk@example.com</a></p>
            <table><tr><th>Item</th><th>Price</th></tr><tr><td>Car</td><td>&pound;41.20</td></tr></table>
            <pre>  keep   this
  spacing</pre><img alt="logo" src="cid:x"><img src="cid:y" alt="">
            <script>var x = "no";</script><p></p><p></p><p></p><p>end</p>"#;
        assert_eq!(
            render(html),
            "Booking confirmed\n\nHello Ana,\nyour car is ready.\n\n- Pick-up: 9:00\n- Return: 18:00\n\
             3. three\n4. four\n> quoted line\n>\n> second\n>\n\
             View booking (https://example.com/b/42) or https://example.com or desk@example.com\n\n\
             Item | Price\nCar | £41.20\n  keep   this\n  spacing\n[image: logo]\n\nend"
        );
        assert_eq!(snippet("  a\n\nb  c "), "a b c");
        assert_eq!(snippet(&"x".repeat(500)).len(), 240);
    }
}
