//! Snippets ([Search § 5.5]): built in Rust because contentless FTS5 tables cannot. The text is folded
//! (NFKD, combining marks dropped, lower case) with a map back to the original, tokenised into runs of
//! letters and digits, and the window that covers the most distinct terms wins.
//!
//! [Search § 5.5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md

use unicode_normalization::UnicodeNormalization as _;
use unicode_normalization::char::is_combining_mark;

use crate::query::TermPattern;

/// One token of the folded text: its folded form and its range in the original, in characters.
struct Token {
    folded: String,
    start: usize,
    end: usize,
}

fn fold_str(s: &str) -> String {
    s.nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

fn tokens(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphanumeric() {
            let start = i;
            while i < chars.len() && chars[i].is_alphanumeric() {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            out.push(Token {
                folded: fold_str(&word),
                start,
                end: i,
            });
        } else {
            i += 1;
        }
    }
    out
}

fn leaf_tokens(t: &TermPattern) -> Vec<String> {
    tokens(&t.text).into_iter().map(|t| t.folded).collect()
}

/// The spans `(term index, start, end)` of the terms in `text`, in characters.
pub fn spans(text: &str, terms: &[TermPattern]) -> Vec<(usize, usize, usize)> {
    let toks = tokens(text);
    let mut out = Vec::new();
    for (ti, t) in terms.iter().enumerate() {
        let leaf = leaf_tokens(t);
        if leaf.is_empty() {
            continue;
        }
        for i in 0..toks.len() {
            if i + leaf.len() > toks.len() {
                break;
            }
            let ok = leaf.iter().enumerate().all(|(k, w)| {
                let tok = &toks[i + k].folded;
                if t.prefix && k + 1 == leaf.len() {
                    tok.starts_with(w.as_str())
                } else {
                    tok == w
                }
            });
            if ok {
                out.push((ti, toks[i].start, toks[i + leaf.len() - 1].end));
            }
        }
    }
    out
}

/// Whether any term occurs in `text`.
pub fn contains_any(text: &str, terms: &[TermPattern]) -> bool {
    !spans(text, terms).is_empty()
}

/// A plain-text snippet of at most `chars` characters around the best window of term spans; the
/// start of `text` when no term occurs.
pub fn snippet(text: &str, terms: &[TermPattern], chars: usize) -> String {
    let all: Vec<char> = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let sp = spans(text, terms);
    let len = all.len();
    let mut best_start = 0usize;
    if !sp.is_empty() && len > chars {
        let mut best = (0usize, 0usize);
        for &(_, s, _) in &sp {
            let start = s.saturating_sub(chars / 4).min(len.saturating_sub(chars));
            let end = start + chars;
            let inside: Vec<&(usize, usize, usize)> = sp
                .iter()
                .filter(|(_, a, b)| *a >= start && *b <= end)
                .collect();
            let mut distinct: Vec<usize> = inside.iter().map(|x| x.0).collect();
            distinct.sort_unstable();
            distinct.dedup();
            let score = (10 * distinct.len() + inside.len(), usize::MAX - start);
            if score > best {
                best = score;
                best_start = start;
            }
        }
    }
    // Word boundaries.
    while best_start > 0
        && best_start < len
        && all[best_start - 1].is_alphanumeric()
        && all[best_start].is_alphanumeric()
    {
        best_start -= 1;
    }
    let lead = best_start > 0;
    let budget = chars.saturating_sub(usize::from(lead));
    let mut end = (best_start + budget).min(len);
    let trail = end < len;
    let budget_end = if trail { end.saturating_sub(1) } else { end };
    end = budget_end;
    if trail {
        while end > best_start + 1 && all[end - 1].is_alphanumeric() && all[end].is_alphanumeric() {
            end -= 1;
        }
    }
    let body: String = all[best_start..end].iter().collect();
    let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s = String::new();
    if lead {
        s.push('…');
    }
    s.push_str(&body);
    if trail {
        s.push('…');
    }
    s.chars().take(chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::TextField;

    fn t(text: &str, prefix: bool) -> TermPattern {
        TermPattern {
            text: text.into(),
            prefix,
            phrase: text.contains(' '),
            field: TextField::Any,
        }
    }

    #[test]
    fn snippet_window() {
        let text = format!(
            "{} The invoice for Café Brightwell is attached. {}",
            "x ".repeat(200),
            "y ".repeat(200)
        );
        let s = snippet(&text, &[t("cafe", false), t("invo", true)], 60);
        assert!(s.contains("invoice") && s.contains("Café"), "{s}");
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert!(s.chars().count() <= 60);
        assert_eq!(
            spans("Ab12 cde here", &[t("ab12 cde", false)]).len(),
            1,
            "phrase"
        );
        assert_eq!(snippet("short text", &[t("zzz", false)], 100), "short text");
    }
}
