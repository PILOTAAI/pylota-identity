//! Chunking for embeddings ([Search § 6.2]): an estimate that overcounts tokens, chunks of up to 500
//! estimated tokens with 64 of overlap, split at paragraphs, then sentences, then whitespace.
//!
//! [Search § 6.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

pub const TARGET: f64 = 480.0;
pub const MAX: f64 = 500.0;
pub const OVERLAP: f64 = 64.0;
pub const MAX_BODY_CHUNKS: usize = 64;
pub const MAX_ATTACHMENT_CHUNKS: usize = 200;
/// The chunker generation of the model tag (`bge-m3@1`).
pub const CHUNKER_VERSION: u32 = 1;

/// The weight of one character in estimated tokens.
pub fn weight(c: char) -> f64 {
    match c {
        c if c.is_whitespace() => 0.15,
        c if c.is_ascii() => 0.30,
        '\u{80}'..='\u{4FF}' => 0.40,
        '\u{3040}'..='\u{30FF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{AC00}'..='\u{D7AF}' => 1.0,
        _ => 0.60,
    }
}

pub fn est_tokens(s: &str) -> usize {
    s.chars().map(weight).sum::<f64>().ceil() as usize
}

/// One chunk: its text and its character range in the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub char_start: usize,
    pub char_end: usize,
}

/// Segment ends: after `\n\n`, a sentence end, a line break, or whitespace, in that preference.
fn cut_points(chars: &[char]) -> Vec<(usize, u8)> {
    let mut out = Vec::new();
    for i in 1..chars.len() {
        let prev = chars[i - 1];
        let rank = if prev == '\n' && i >= 2 && chars[i - 2] == '\n' {
            0
        } else if (matches!(prev, '.' | '?' | '!') && chars[i].is_whitespace())
            || prev == '。'
            || prev == '\n'
        {
            1
        } else if prev.is_whitespace() {
            2
        } else {
            continue;
        };
        out.push((i, rank));
    }
    out
}

/// Splits `text` into chunks of at most [`MAX`] estimated tokens with [`OVERLAP`].
pub fn split(text: &str, cap: usize) -> Vec<Piece> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let cuts = cut_points(&chars);
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < chars.len() && out.len() < cap {
        // The furthest end within MAX, at the best boundary available.
        let mut acc = 0.0;
        let mut hard_end = start;
        while hard_end < chars.len() && acc + weight(chars[hard_end]) <= MAX {
            acc += weight(chars[hard_end]);
            hard_end += 1;
        }
        let end = if hard_end >= chars.len() {
            chars.len()
        } else {
            let lo = start + (hard_end - start) / 2;
            (0u8..3)
                .find_map(|rank| {
                    cuts.iter()
                        .rev()
                        .find(|(i, r)| *r == rank && *i > lo && *i <= hard_end)
                        .map(|(i, _)| *i)
                })
                .unwrap_or(hard_end.max(start + 1))
        };
        out.push(Piece {
            text: chars[start..end].iter().collect(),
            char_start: start,
            char_end: end,
        });
        if end >= chars.len() {
            break;
        }
        // The next chunk starts with the last OVERLAP estimated tokens, at a whitespace boundary.
        let mut back = 0.0;
        let mut next = end;
        while next > start + 1 && back + weight(chars[next - 1]) <= OVERLAP {
            back += weight(chars[next - 1]);
            next -= 1;
        }
        while next < end && !chars[next - 1].is_whitespace() {
            next += 1;
        }
        start = if next >= end { end } else { next };
    }
    out
}

/// The body source: `"Subject: " + subject + "\n\n" + text`; the offsets count `text` only.
pub fn body(subject: &str, text: &str) -> Vec<Piece> {
    let prefix = format!("Subject: {subject}\n\n");
    let plen = prefix.chars().count();
    split(&format!("{prefix}{text}"), MAX_BODY_CHUNKS)
        .into_iter()
        .map(|p| Piece {
            char_start: p.char_start.saturating_sub(plen),
            char_end: p.char_end.saturating_sub(plen),
            text: p.text,
        })
        .collect()
}

/// Attachment text split at `<!-- page N -->` markers (pages under 50 estimated tokens merge into the
/// next); `(page, piece)` with offsets relative to the page.
pub fn pages(md: &str, budget: usize) -> Vec<(u32, Piece)> {
    let mut pages: Vec<(u32, String)> = Vec::new();
    let mut cur = (1u32, String::new());
    for line in md.split_inclusive('\n') {
        let l = line.trim();
        if let Some(n) = l
            .strip_prefix("<!-- page ")
            .and_then(|r| r.strip_suffix("-->"))
            .and_then(|n| n.trim().parse::<u32>().ok())
        {
            if !cur.1.trim().is_empty() {
                pages.push(std::mem::take(&mut cur));
            }
            cur.0 = n;
            continue;
        }
        cur.1.push_str(line);
    }
    if !cur.1.trim().is_empty() {
        pages.push(cur);
    }
    let mut out = Vec::new();
    let mut carry: Option<(u32, String)> = None;
    for (n, text) in pages {
        let (page, text) = match carry.take() {
            Some((p, t)) => (p, format!("{t}{text}")),
            None => (n, text),
        };
        if est_tokens(&text) < 50 {
            carry = Some((page, text));
            continue;
        }
        for piece in split(&text, budget.saturating_sub(out.len())) {
            out.push((page, piece));
        }
    }
    if let Some((p, t)) = carry {
        for piece in split(&t, budget.saturating_sub(out.len())) {
            out.push((p, piece));
        }
    }
    out.truncate(budget);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunking() {
        assert_eq!(est_tokens("abc def"), 2);
        assert_eq!(est_tokens("日本"), 2);
        let para = "The car was returned with a scratch on the door. ".repeat(80);
        let pieces = body("Damage", &para);
        assert!(pieces.len() > 1);
        assert!(
            pieces
                .iter()
                .all(|p| est_tokens(&p.text) <= MAX as usize + 1)
        );
        // Overlap: the second chunk starts before the first ends.
        assert!(pieces[1].char_start < pieces[0].char_end);
        let md = format!(
            "<!-- page 1 -->\nshort\n<!-- page 2 -->\n{}",
            "word ".repeat(400)
        );
        let p = pages(&md, MAX_ATTACHMENT_CHUNKS);
        assert_eq!(
            p[0].0, 1,
            "a short page merges into the next and keeps its number"
        );
        assert!(split(&"a".repeat(5000), 3).len() <= 3, "caps");
        assert!(format!("msg_{}:a{}:{}", "0".repeat(26), 31, 199).len() <= 64);
    }
}
