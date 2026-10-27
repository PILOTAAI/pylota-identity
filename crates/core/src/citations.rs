//! The agentic-answer citation verifier ([Search › Answer schema], [Search › Citation verifier],
//! [Parity additions › How each rule treats `arc_rescued`]): every answer sentence must cite evidence the
//! planner saw, quote it faithfully, restate only references it holds, and rest on authenticated mail.
//! Deterministic, no I/O (FR-SRCH-8, FR-SRCH-13, NFR-QUAL-2). F11, F17, F18 and Z43.
//!
//! [Search › Answer schema]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md
//! [Search › Citation verifier]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/search.md
//! [Parity additions › How each rule treats `arc_rescued`]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/parity-additions.md

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerStatus {
    Answered,
    InsufficientEvidence,
}

/// One sentence as the model wrote it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftSentence {
    pub text: String,
    pub citations: Vec<String>,
}

/// The model's answer (Search §11.7), structured or rebuilt from prose by the fallback path.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftAnswer {
    pub status: AnswerStatus,
    pub sentences: Vec<DraftSentence>,
    pub confidence: f64,
    pub not_found: Vec<String>,
}

/// `answer_format` in the trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerFormat {
    Structured,
    Fallback,
}

/// Extraction (Search §11.7): strip a surrounding Markdown code fence and parse the schema; otherwise
/// treat the content as prose, split it into sentences whose citations come from inline markers, with
/// `status = answered` and `confidence = 0.5`. `None` when the content is empty (a model error).
pub fn parse_answer(content: &str) -> Option<(DraftAnswer, AnswerFormat)> {
    let t = content.trim();
    if t.is_empty() {
        return None;
    }
    let inner = t
        .strip_prefix("```")
        .and_then(|r| r.strip_suffix("```"))
        .map(|r| r.split_once('\n').map_or("", |(_, body)| body))
        .unwrap_or(t);
    if let Ok(a) = serde_json::from_str::<DraftAnswer>(inner) {
        return Some((a, AnswerFormat::Structured));
    }
    let sentences = split_sentences(t)
        .into_iter()
        .map(|text| DraftSentence {
            text,
            citations: Vec::new(),
        })
        .collect();
    Some((
        DraftAnswer {
            status: AnswerStatus::Answered,
            sentences,
            confidence: 0.5,
            not_found: Vec::new(),
        },
        AnswerFormat::Fallback,
    ))
}

const ABBREVIATIONS: &[&str] = &[
    "e.g.", "i.e.", "Mr.", "Mrs.", "Ms.", "Dr.", "No.", "Ltd.", "Inc.", "St.", "vs.",
];

fn marker() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\[(msg_[0-9A-HJKMNP-TV-Z]{26})\]").expect("fixed pattern"))
}

/// The fallback split (Search §11.8 step 1): at `.`, `?` or `!` followed by whitespace and an upper-case
/// letter, digit or opening quote, except after a listed abbreviation or between digits. Inline markers
/// right after the punctuation stay with the sentence they close.
pub fn split_sentences(prose: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while let Some(c) = prose[i..].chars().next() {
        let next = i + c.len_utf8();
        if matches!(c, '.' | '?' | '!') {
            let mut end = next;
            // Markers after the punctuation: "accepted. [msg_…] Next".
            loop {
                let rest = &prose[end..];
                let trimmed = rest.trim_start();
                match marker().find(trimmed) {
                    Some(m) if m.start() == 0 => end += rest.len() - trimmed.len() + m.end(),
                    _ => break,
                }
            }
            let rest = &prose[end..];
            let after_ws = rest.trim_start();
            let opens = after_ws.chars().next().is_some_and(|n| {
                n.is_uppercase() || n.is_ascii_digit() || matches!(n, '"' | '“' | '‘' | '\'')
            });
            let word = prose[start..next]
                .rsplit(char::is_whitespace)
                .next()
                .unwrap_or("");
            let abbreviation = c == '.' && ABBREVIATIONS.contains(&word);
            let digits = c == '.'
                && prose[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|p| p.is_ascii_digit())
                && after_ws.chars().next().is_some_and(|n| n.is_ascii_digit());
            if rest.len() > after_ws.len() && opens && !abbreviation && !digits {
                let s = prose[start..end].trim();
                if !s.is_empty() {
                    out.push(s.to_owned());
                }
                start = end;
            }
            i = end.max(next);
            continue;
        }
        i = next;
    }
    let tail = prose[start..].trim();
    if !tail.is_empty() {
        out.push(tail.to_owned());
    }
    out
}

/// Parity additions §6.5.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceAuth {
    /// Outbound, or inbound with verdict `pass`.
    Authenticated,
    /// Inbound with verdict `arc_rescued` (a trusted ARC sealer).
    Rescued,
    /// Every other verdict.
    Unauthenticated,
}

/// One message in the planner's evidence set.
#[derive(Clone, Debug, PartialEq)]
pub struct EvidenceItem {
    pub message_id: String,
    pub thread_id: String,
    pub identity_id: String,
    /// Every untrusted string the planner saw for this message, unfenced.
    pub texts_seen: Vec<String>,
    /// The message's normalised reference values.
    pub refs: Vec<String>,
    pub auth: SourceAuth,
    /// A steering signal (§11.9), or a `suspect` screening record.
    pub steering_suspected: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EvidenceSet {
    pub items: Vec<EvidenceItem>,
}

impl EvidenceSet {
    fn get(&self, id: &str) -> Option<&EvidenceItem> {
        self.items.iter().find(|i| i.message_id == id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum RemovalReason {
    NoCitation,
    CitationNotInEvidence,
    QuoteNotFound,
    UnsupportedReference,
    UntrustedSource,
}

/// A removed sentence, recorded in the `answer` trace entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Removal {
    pub index: usize,
    pub reason: RemovalReason,
    /// At most 120 characters of the sentence.
    pub excerpt: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CitationTrust {
    Authenticated,
    PartlyAuthenticated,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VerifiedSentence {
    pub text: String,
    pub citations: Vec<String>,
    pub citation_trust: CitationTrust,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Verified {
    /// `answered` when the model answered and a sentence survived.
    pub status: AnswerStatus,
    /// Kept sentences rendered `"<text> [msg_a][msg_b]"`, joined with single spaces.
    pub text: String,
    pub sentences: Vec<VerifiedSentence>,
    /// The model's confidence × kept / total when sentences were removed.
    pub confidence: f64,
    pub removed: Vec<Removal>,
    /// `evidence[].quotes`: per message ID, the quoted fragments of kept sentences found in it.
    pub quotes: BTreeMap<String, Vec<String>>,
}

/// `N` of step 5: NFKC; lower case; curly quotes to straight; `‐ – — −` to `-`; whitespace runs to one
/// space; leading and trailing punctuation and spaces trimmed.
pub fn normalise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.nfkc().flat_map(char::to_lowercase) {
        let c = match c {
            '‘' | '’' | '‚' | '‛' => '\'',
            '“' | '”' | '„' | '‟' => '"',
            '‐' | '–' | '—' | '−' => '-',
            c => c,
        };
        if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    let punct =
        |c: char| c == ' ' || c.is_ascii_punctuation() || matches!(c, '«' | '»' | '¡' | '¿');
    out.trim_matches(punct).to_owned()
}

/// Quoted fragments of step 5: spans between `"…"` or `“…”`, split at `…` or `...`, as written.
fn fragments(text: &str) -> Vec<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let re = R.get_or_init(|| Regex::new(r#""([^"]*)"|“([^”]*)”"#).expect("fixed pattern"));
    re.captures_iter(text)
        .filter_map(|c| c.get(1).or_else(|| c.get(2)))
        .flat_map(|m| m.as_str().split(['…']).flat_map(|p| p.split("...")))
        .filter(|f| normalise(f).chars().count() >= 3)
        .map(|f| f.trim().to_owned())
        .collect()
}

struct RefPatterns {
    dates: Regex,
    amount_before: Regex,
    amount_after: Regex,
    token: Regex,
}

fn ref_patterns() -> &'static RefPatterns {
    static P: OnceLock<RefPatterns> = OnceLock::new();
    P.get_or_init(|| {
        let re = |s: &str| Regex::new(s).expect("fixed pattern");
        let months = "JANUARY|FEBRUARY|MARCH|APRIL|MAY|JUNE|JULY|AUGUST|SEPTEMBER|OCTOBER|NOVEMBER|\
                      DECEMBER|JAN|FEB|MAR|APR|JUN|JUL|AUG|SEP|SEPT|OCT|NOV|DEC";
        let number = r"(\d{1,3}(?:,\d{3})+(?:\.\d+)?|\d+(?:[.,]\d+)?)";
        RefPatterns {
            dates: re(&format!(
                r"\b(?:\d{{4}}-\d{{2}}-\d{{2}}|\d{{1,2}}[/.]\d{{1,2}}[/.]\d{{4}}|\d{{1,2}} (?:{months})\.? \d{{4}})\b"
            )),
            amount_before: re(&format!(r"(£|€|\$|\b(?:GBP|EUR|USD)\b) ?{number}")),
            amount_after: re(&format!(r"\b{number} ?(€|£|\b(?:EUR|GBP|USD)\b)")),
            token: re(r"[A-Z0-9]+(?:[.,:/+-][A-Z0-9]+)*"),
        }
    })
}

/// `{ISO code}:{amount with two decimals}`; a comma is the decimal separator only for euro amounts of
/// the form `\d+,\d{2}` (Inbound › Reference extraction, `amount`).
fn amount(currency: &str, number: &str) -> String {
    let code = match currency {
        "£" | "GBP" => "GBP",
        "€" | "EUR" => "EUR",
        _ => "USD",
    };
    let euro_comma = code == "EUR"
        && number
            .split_once(',')
            .is_some_and(|(a, b)| b.len() == 2 && !a.contains('.') && !b.contains(','));
    let n = if euro_comma {
        number.replace(',', ".")
    } else {
        number.replace(',', "")
    };
    let (int, frac) = n.split_once('.').unwrap_or((&n, ""));
    let frac: String = frac
        .chars()
        .chain("00".chars())
        .take(2.max(frac.len()))
        .collect();
    format!("{code}:{int}.{frac}")
}

/// Reference tokens of step 6, normalised (upper case; amounts as `GBP:412.80`), that contain a digit
/// and are at least 4 characters long; dates excluded. With `keep_amount_numbers`, an amount also gives
/// its bare number, so a sentence stating `412.80` finds a source that says `£412.80`.
fn ref_tokens(text: &str, keep_amount_numbers: bool) -> BTreeSet<String> {
    let p = ref_patterns();
    let upper: String = text.nfkc().collect::<String>().to_uppercase();
    let mut s = p.dates.replace_all(&upper, " ").into_owned();
    let mut out = BTreeSet::new();
    for (re, cur_at, num_at) in [(&p.amount_before, 1, 2), (&p.amount_after, 2, 1)] {
        for c in re.captures_iter(&s) {
            out.insert(amount(&c[cur_at], &c[num_at]));
        }
        if !keep_amount_numbers {
            s = re.replace_all(&s, " ").into_owned();
        }
    }
    for m in p.token.find_iter(&s) {
        let t = m.as_str();
        if t.chars().count() >= 4 && t.bytes().any(|b| b.is_ascii_digit()) {
            out.insert(t.to_owned());
        }
    }
    out
}

/// What one cited message offers steps 5–7.
struct Source<'a> {
    item: &'a EvidenceItem,
    texts: Vec<String>,
    refs: BTreeSet<String>,
}

impl Source<'_> {
    fn has_fragment(&self, f: &str) -> bool {
        let f = normalise(f);
        self.texts.iter().any(|t| t.contains(&f))
    }
}

/// Verifies a draft answer against the evidence set (Search §11.8).
pub fn verify(answer: &DraftAnswer, evidence: &EvidenceSet) -> Verified {
    let mut kept = Vec::new();
    let mut removed = Vec::new();
    let mut quotes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (index, s) in answer.sentences.iter().enumerate() {
        // 2. Citations: the item's and every inline marker, then markers removed.
        let mut cites: Vec<String> = Vec::new();
        let inline = marker().captures_iter(&s.text).map(|c| c[1].to_owned());
        for id in s.citations.iter().cloned().chain(inline) {
            if !cites.contains(&id) {
                cites.push(id);
            }
        }
        let stripped = marker().replace_all(&s.text, "");
        let text = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
        let text = text.replace(" .", ".").replace(" ,", ",");
        match judge(&text, &cites, evidence) {
            Ok((trust, found)) => {
                for (id, f) in found {
                    let q = quotes.entry(id).or_default();
                    if !q.contains(&f) {
                        q.push(f);
                    }
                }
                kept.push(VerifiedSentence {
                    text,
                    citations: cites,
                    citation_trust: trust,
                });
            }
            Err(reason) => removed.push(Removal {
                index,
                reason,
                excerpt: text.chars().take(120).collect(),
            }),
        }
    }
    let total = answer.sentences.len();
    let confidence = if removed.is_empty() || total == 0 {
        answer.confidence
    } else {
        answer.confidence * kept.len() as f64 / total as f64
    };
    let rendered: Vec<String> = kept
        .iter()
        .map(|s| {
            let marks: String = s.citations.iter().map(|c| format!("[{c}]")).collect();
            format!("{} {marks}", s.text)
        })
        .collect();
    Verified {
        status: if answer.status == AnswerStatus::Answered && !kept.is_empty() {
            AnswerStatus::Answered
        } else {
            AnswerStatus::InsufficientEvidence
        },
        text: rendered.join(" "),
        sentences: kept,
        confidence,
        removed,
        quotes,
    }
}

type Found = Vec<(String, String)>;

/// Steps 3–7 for one sentence: its trust and the `(message ID, fragment)` pairs found, or the reason
/// it is removed.
fn judge(
    text: &str,
    cites: &[String],
    evidence: &EvidenceSet,
) -> Result<(CitationTrust, Found), RemovalReason> {
    // 3–4.
    if cites.is_empty() {
        return Err(RemovalReason::NoCitation);
    }
    let mut sources = Vec::new();
    for id in cites {
        let item = evidence
            .get(id)
            .ok_or(RemovalReason::CitationNotInEvidence)?;
        let mut refs = BTreeSet::new();
        for t in &item.texts_seen {
            refs.extend(ref_tokens(t, true));
        }
        refs.extend(item.refs.iter().map(|r| r.to_uppercase()));
        sources.push(Source {
            item,
            texts: item.texts_seen.iter().map(|t| normalise(t)).collect(),
            refs,
        });
    }
    // 5. Quoted phrases.
    let frags = fragments(text);
    if !frags
        .iter()
        .all(|f| sources.iter().any(|s| s.has_fragment(f)))
    {
        return Err(RemovalReason::QuoteNotFound);
    }
    // 6. References.
    let refs = ref_tokens(text, false);
    if !refs
        .iter()
        .all(|r| sources.iter().any(|s| s.refs.contains(r)))
    {
        return Err(RemovalReason::UnsupportedReference);
    }
    // 7. Source trust.
    let auth = |a: SourceAuth| sources.iter().filter(move |s| s.item.auth == a);
    if sources.iter().any(|s| s.item.steering_suspected)
        || auth(SourceAuth::Unauthenticated).count() == sources.len()
    {
        return Err(RemovalReason::UntrustedSource);
    }
    let all_authenticated = auth(SourceAuth::Authenticated).count() == sources.len();
    if !all_authenticated {
        let trusted: Vec<&Source> = if auth(SourceAuth::Authenticated).next().is_some() {
            auth(SourceAuth::Authenticated).collect()
        } else {
            auth(SourceAuth::Rescued).collect()
        };
        let ok = frags
            .iter()
            .all(|f| trusted.iter().any(|s| s.has_fragment(f)))
            && refs
                .iter()
                .all(|r| trusted.iter().any(|s| s.refs.contains(r)));
        if !ok {
            return Err(RemovalReason::UntrustedSource);
        }
    }
    let found = sources
        .iter()
        .flat_map(|s| {
            frags
                .iter()
                .filter(|f| s.has_fragment(f))
                .map(|f| (s.item.message_id.clone(), f.clone()))
        })
        .collect();
    let trust = if all_authenticated {
        CitationTrust::Authenticated
    } else {
        CitationTrust::PartlyAuthenticated
    };
    Ok((trust, found))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const A: &str = "msg_01JA0000000000000000000001";
    const B: &str = "msg_01JA0000000000000000000002";
    const C: &str = "msg_01JA0000000000000000000003";
    const X: &str = "msg_01JA0000000000000000000009";

    fn item(id: &str, auth: SourceAuth, texts: &[&str], refs: &[&str]) -> EvidenceItem {
        EvidenceItem {
            message_id: id.into(),
            thread_id: "thr_01JA0000000000000000000001".into(),
            identity_id: "idn_01JA0000000000000000000001".into(),
            texts_seen: texts.iter().map(|&t| t.into()).collect(),
            refs: refs.iter().map(|&r| r.into()).collect(),
            auth,
            steering_suspected: false,
        }
    }

    fn draft(sentences: &[(&str, &[&str])]) -> DraftAnswer {
        DraftAnswer {
            status: AnswerStatus::Answered,
            sentences: sentences
                .iter()
                .map(|(t, c)| DraftSentence {
                    text: (*t).into(),
                    citations: c.iter().map(|&c| c.into()).collect(),
                })
                .collect(),
            confidence: 0.8,
            not_found: Vec::new(),
        }
    }

    fn one(text: &str, cites: &[&str], ev: &EvidenceSet) -> Result<CitationTrust, RemovalReason> {
        let v = verify(&draft(&[(text, cites)]), ev);
        match (v.sentences.first(), v.removed.first()) {
            (Some(s), None) => Ok(s.citation_trust),
            (None, Some(r)) => Err(r.reason),
            _ => unreachable!(),
        }
    }

    fn claims() -> EvidenceSet {
        EvidenceSet {
            items: vec![
                item(
                    A,
                    SourceAuth::Authenticated,
                    &[
                        "Claim 7781 – update",
                        "We are pleased to confirm claim 7781 has been accepted.",
                    ],
                    &["7781"],
                ),
                item(
                    B,
                    SourceAuth::Authenticated,
                    &["Invoice 88213 total £412.80"],
                    &["88213"],
                ),
            ],
        }
    }

    /// Covers: F11
    #[test]
    fn f11_removal_reasons() {
        let ev = claims();
        assert_eq!(
            one("Claim 7781 was accepted.", &[A], &ev),
            Ok(CitationTrust::Authenticated)
        );
        assert_eq!(
            one("Claim 7781 was accepted.", &[], &ev),
            Err(RemovalReason::NoCitation)
        );
        assert_eq!(
            one("Claim 7781 was accepted.", &[A, X], &ev),
            Err(RemovalReason::CitationNotInEvidence)
        );
        assert_eq!(
            one("They wrote \"claim 7781 has been rejected\".", &[A], &ev),
            Err(RemovalReason::QuoteNotFound)
        );
        assert_eq!(
            one("Claim 7782 was accepted.", &[A], &ev),
            Err(RemovalReason::UnsupportedReference)
        );
        // An amount is a reference; dates are not.
        assert_eq!(
            one("Invoice 88213 is for £412.80.", &[B], &ev),
            Ok(CitationTrust::Authenticated)
        );
        assert_eq!(
            one("Invoice 88213 is 412.80.", &[B], &ev),
            Ok(CitationTrust::Authenticated)
        );
        assert_eq!(
            one("Invoice 88213 is for £412.90.", &[B], &ev),
            Err(RemovalReason::UnsupportedReference)
        );
        assert_eq!(
            one(
                "Claim 7781 was accepted on 2 October 2026 (2026-10-02).",
                &[A],
                &ev
            ),
            Ok(CitationTrust::Authenticated)
        );
        // Short fragments are ignored.
        assert_eq!(
            one("Claim 7781 was \"ok\".", &[A], &ev),
            Ok(CitationTrust::Authenticated)
        );
    }

    /// Covers: F11
    #[test]
    fn f11_inline_markers() {
        let ev = claims();
        let v = verify(
            &draft(&[
                (&format!("Claim 7781 was accepted [{A}]."), &[]),
                (&format!("The invoice is 88213 [{B}][{A}]."), &[B]),
            ]),
            &ev,
        );
        assert!(v.removed.is_empty());
        assert_eq!(v.sentences[0].text, "Claim 7781 was accepted.");
        assert_eq!(v.sentences[0].citations, vec![A.to_owned()]);
        assert_eq!(v.sentences[1].citations, vec![B.to_owned(), A.to_owned()]);
        assert_eq!(
            v.text,
            format!("Claim 7781 was accepted. [{A}] The invoice is 88213. [{B}][{A}]")
        );
        // An inline marker for an ID outside the evidence set removes the sentence.
        let v = verify(&draft(&[(&format!("Claim 7781 [{X}]."), &[A])]), &ev);
        assert_eq!(v.removed[0].reason, RemovalReason::CitationNotInEvidence);
        assert_eq!(v.status, AnswerStatus::InsufficientEvidence);
    }

    /// Covers: F11
    #[test]
    fn f11_quote_normalisation() {
        let ev = EvidenceSet {
            items: vec![item(
                A,
                SourceAuth::Authenticated,
                &["Repairs — “brake pads   and discs” — will take\n3 days…"],
                &[],
            )],
        };
        for q in [
            "The garage said \"BRAKE PADS and discs\".",
            "The garage said “brake pads and discs”.",
            "The garage said \"brake pads ... will take 3 days\".",
            "The garage said \"Repairs – “brake pads and discs”\".",
        ] {
            assert_eq!(one(q, &[A], &ev), Ok(CitationTrust::Authenticated), "{q}");
        }
        let v = verify(&draft(&[("They said “brake pads … 3 days”.", &[A])]), &ev);
        assert_eq!(
            v.quotes[A],
            vec!["brake pads".to_owned(), "3 days".to_owned()]
        );
        assert_eq!(normalise("  “Hello,   World!” "), "hello, world");
        assert_eq!(normalise("a—b–c−d"), "a-b-c-d");
    }

    /// Covers: F11
    #[test]
    fn f11_fallback_split() {
        let prose = format!(
            "Claim 7781 was accepted [{A}]. The excess, i.e. the first £250, is due. Mr. Lee \
             paid 412.80 on Monday! Did they? \"Yes,\" said Dr. Patel. 3 parts arrived. [{B}] Done"
        );
        assert_eq!(
            split_sentences(&prose),
            vec![
                format!("Claim 7781 was accepted [{A}]."),
                "The excess, i.e. the first £250, is due.".to_owned(),
                "Mr. Lee paid 412.80 on Monday!".to_owned(),
                "Did they?".to_owned(),
                "\"Yes,\" said Dr. Patel.".to_owned(),
                format!("3 parts arrived. [{B}]"),
                "Done".to_owned(),
            ]
        );
        assert_eq!(
            split_sentences("Total 412. 80 people came."),
            vec!["Total 412. 80 people came."]
        );

        let (a, f) =
            parse_answer(&format!("Claim 7781 was accepted [{A}]. It is closed.")).unwrap();
        assert_eq!(f, AnswerFormat::Fallback);
        assert_eq!(
            (a.status, a.confidence, a.sentences.len()),
            (AnswerStatus::Answered, 0.5, 2)
        );
        let v = verify(&a, &claims());
        assert_eq!(v.removed.len(), 1);
        assert_eq!(v.removed[0].reason, RemovalReason::NoCitation);
        assert_eq!(v.confidence, 0.25);

        let json = format!(
            "```json\n{{\"status\":\"answered\",\"sentences\":[{{\"text\":\"Claim 7781 was accepted.\",\"citations\":[\"{A}\"]}}],\"confidence\":0.9,\"not_found\":[]}}\n```"
        );
        let (a, f) = parse_answer(&json).unwrap();
        assert_eq!(f, AnswerFormat::Structured);
        assert_eq!(a.sentences[0].citations, vec![A.to_owned()]);
        // Unknown fields fall back to prose.
        let (_, f) = parse_answer("{\"status\":\"answered\",\"extra\":1}").unwrap();
        assert_eq!(f, AnswerFormat::Fallback);
        assert_eq!(parse_answer("  \n"), None);
    }

    /// Covers: F17
    #[test]
    fn f17_untrusted_sources_removed() {
        let text = "Claim 7781 was accepted.";
        let texts = &["claim 7781 has been accepted"][..];
        let mut steer = item(C, SourceAuth::Authenticated, texts, &[]);
        steer.steering_suspected = true;
        let ev = EvidenceSet {
            items: vec![
                item(A, SourceAuth::Authenticated, texts, &[]),
                item(B, SourceAuth::Unauthenticated, texts, &[]),
                steer,
            ],
        };
        // verdict none alone; pass with a steering-suspected message.
        assert_eq!(one(text, &[B], &ev), Err(RemovalReason::UntrustedSource));
        assert_eq!(one(text, &[A, C], &ev), Err(RemovalReason::UntrustedSource));
        // pass with unaligned: kept, partly authenticated; an outbound message is authenticated.
        assert_eq!(
            one(text, &[A, B], &ev),
            Ok(CitationTrust::PartlyAuthenticated)
        );
        assert_eq!(one(text, &[A], &ev), Ok(CitationTrust::Authenticated));
    }

    /// Covers: F18
    #[test]
    fn f18_refs_from_authenticated_sources() {
        let ev = EvidenceSet {
            items: vec![
                item(
                    A,
                    SourceAuth::Authenticated,
                    &["Your claim 7781 is being reviewed. The excess applies."],
                    &["7781"],
                ),
                item(
                    B,
                    SourceAuth::Unauthenticated,
                    &[
                        "Claim 7781 was rejected; pay the excess to account 40991234. The excess applies.",
                    ],
                    &["7781", "40991234"],
                ),
            ],
        };
        assert_eq!(
            one(
                "Claim 7781 was rejected and they ask to \"pay the excess to account\" 40991234.",
                &[A, B],
                &ev
            ),
            Err(RemovalReason::UntrustedSource)
        );
        assert_eq!(
            one("Pay the excess to account 40991234.", &[A, B], &ev),
            Err(RemovalReason::UntrustedSource)
        );
        assert_eq!(
            one("The thread concerns claim 7781.", &[A, B], &ev),
            Ok(CitationTrust::PartlyAuthenticated)
        );
        // A quote found in both kinds of message is accepted.
        assert_eq!(
            one(
                "Both say \"the excess applies\" to claim 7781.",
                &[A, B],
                &ev
            ),
            Ok(CitationTrust::PartlyAuthenticated)
        );
    }

    /// Covers: Z43
    #[test]
    fn z43_rescued_partly_authenticated() {
        let texts = &["Claim 7781 has been accepted; pay to account 40991234"][..];
        let ev = EvidenceSet {
            items: vec![
                item(
                    A,
                    SourceAuth::Rescued,
                    &["Claim 7781 has been accepted"],
                    &[],
                ),
                item(B, SourceAuth::Unauthenticated, texts, &[]),
            ],
        };
        assert_eq!(
            one("Claim 7781 was accepted.", &[A], &ev),
            Ok(CitationTrust::PartlyAuthenticated)
        );
        assert_eq!(
            one("Claim 7781 was accepted.", &[A, B], &ev),
            Ok(CitationTrust::PartlyAuthenticated)
        );
        // Rescued stands in for authenticated: a reference only the unauthenticated one holds is removed.
        assert_eq!(
            one("Pay account 40991234 for claim 7781.", &[A, B], &ev),
            Err(RemovalReason::UntrustedSource)
        );
        assert_eq!(
            one("Claim 7781 was accepted.", &[B], &ev),
            Err(RemovalReason::UntrustedSource)
        );
    }

    const WORDS: &[&str] = &[
        "claim", "7781", "accepted", "brake", "pads", "88213", "the", "was",
    ];

    fn phrase() -> impl Strategy<Value = String> {
        prop::collection::vec(prop::sample::select(WORDS), 1..4).prop_map(|w| w.join(" "))
    }

    proptest! {
        /// Covers: F11
        #[test]
        fn f11_kept_only_if_cited_and_quoted(
            sources in prop::collection::vec(prop::collection::vec(phrase(), 0..3), 1..4),
            sentences in prop::collection::vec(
                (phrase(), prop::collection::vec(phrase(), 0..2), prop::collection::vec(0usize..5, 0..3)),
                1..5),
        ) {
            let ids = [A, B, C, X, "msg_bad"];
            let ev = EvidenceSet { items: sources.iter().enumerate().map(|(i, t)| {
                let texts: Vec<&str> = t.iter().map(String::as_str).collect();
                item(ids[i], SourceAuth::Authenticated, &texts, &[])
            }).collect() };
            let d = DraftAnswer {
                status: AnswerStatus::Answered,
                sentences: sentences.iter().map(|(t, q, c)| DraftSentence {
                    text: format!("{t} {}", q.iter().map(|q| format!("\"{q}\"")).collect::<Vec<_>>().join(" ")),
                    citations: c.iter().map(|&i| ids[i].to_owned()).collect(),
                }).collect(),
                confidence: 1.0,
                not_found: Vec::new(),
            };
            let v = verify(&d, &ev);
            prop_assert_eq!(v.sentences.len() + v.removed.len(), d.sentences.len());
            for s in &v.sentences {
                prop_assert!(!s.citations.is_empty());
                let cited: Vec<&EvidenceItem> = s.citations.iter().map(|c| ev.get(c)).collect::<Option<_>>().unwrap();
                for f in fragments(&s.text) {
                    let f = normalise(&f);
                    prop_assert!(cited.iter().any(|i| i.texts_seen.iter().any(|t| normalise(t).contains(&f))));
                }
            }
        }
    }
}
