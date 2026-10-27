//! Threading rules ([Threading § 3–7]): the inbound resolution order, Message-ID normalisation,
//! `References` building and trimming, subject normalisation and participants. Headers and tokens join
//! threads; subjects never do.
//!
//! [Threading § 3–7]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/threading.md

use crate::thread_token::TokenCheck;

pub struct InboundThreadInputs<'a> {
    /// From `thread_token::verify`, after rate limiting.
    pub token: TokenCheck,
    /// Normalised msg-id.
    pub in_reply_to: Option<&'a str>,
    /// Normalised msg-ids, header order.
    pub references: &'a [String],
    /// Normalised `From` address.
    pub sender: &'a str,
    /// Verdict `pass` or `arc_rescued`, or an aligned DKIM or SPF pass.
    pub sender_authenticated: bool,
    /// Hex SHA-256 of the primary UID, only for a verified calendar object.
    pub calendar_uid_hash: Option<&'a str>,
    /// Its normalised `ORGANIZER`, set with `calendar_uid_hash`.
    pub calendar_organizer: Option<&'a str>,
    /// For reply, refresh and counter: the replier's party hash; `None` for organizer methods.
    pub calendar_replier: Option<&'a str>,
}

/// Implemented over the mailbox SQLite connection.
pub trait ThreadLookup {
    fn thread_exists(&self, seq: i64) -> bool;
    fn thread_of_message_id(&self, msg_id: &str) -> Option<i64>;
    fn is_participant(&self, seq: i64, address: &str) -> bool;
    fn thread_of_calendar_uid(
        &self,
        uid_hash: &str,
        organizer: &str,
        replier: Option<&str>,
    ) -> Option<i64>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinVia {
    Token,
    InReplyTo,
    References,
    CalendarUid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadDecision {
    Join { seq: i64, via: JoinVia },
    New,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ThreadResolution {
    pub decision: ThreadDecision,
    /// Trust flag `thread_join_unverified`.
    pub join_unverified: bool,
}

/// At most this many `References` lookups, newest first.
pub const MAX_REFERENCE_LOOKUPS: usize = 50;

/// FR-THR-1: token, then `In-Reply-To`, then `References` (newest first), then a verified calendar
/// UID, else a new thread. The subject is never consulted.
pub fn resolve_inbound(inp: &InboundThreadInputs<'_>, db: &impl ThreadLookup) -> ThreadResolution {
    let header_seq = || -> Option<(i64, JoinVia)> {
        if let Some(seq) = inp.in_reply_to.and_then(|id| db.thread_of_message_id(id)) {
            return Some((seq, JoinVia::InReplyTo));
        }
        inp.references
            .iter()
            .rev()
            .take(MAX_REFERENCE_LOOKUPS)
            .find_map(|id| db.thread_of_message_id(id))
            .map(|seq| (seq, JoinVia::References))
    };
    let participant = |seq: i64| inp.sender_authenticated && db.is_participant(seq, inp.sender);

    let mut join_unverified = matches!(inp.token, TokenCheck::Invalid);
    let decision = match inp.token {
        TokenCheck::Valid { seq, .. } if i64::try_from(seq).is_ok_and(|s| db.thread_exists(s)) => {
            let seq = seq as i64;
            if !participant(seq) && header_seq().map(|(s, _)| s) != Some(seq) {
                join_unverified = true;
            }
            ThreadDecision::Join {
                seq,
                via: JoinVia::Token,
            }
        }
        _ => {
            let by_uid = || {
                let uid = inp.calendar_uid_hash?;
                let organizer = inp.calendar_organizer?;
                db.thread_of_calendar_uid(uid, organizer, inp.calendar_replier)
                    .map(|seq| (seq, JoinVia::CalendarUid))
            };
            match header_seq().or_else(by_uid) {
                Some((seq, via)) => {
                    if !participant(seq) {
                        join_unverified = true;
                    }
                    ThreadDecision::Join { seq, via }
                }
                None => ThreadDecision::New,
            }
        }
    };
    ThreadResolution {
        decision,
        join_unverified,
    }
}

/// Normalises one msg-id (Threading § 3.2): the content of the first `<…>` (or the trimmed value), CFWS
/// removed, the part after the last `@` lower-cased. `None` when empty, over 998 bytes or without `@`.
pub fn normalise_msg_id(value: &str) -> Option<String> {
    let inner = match value.find('<') {
        Some(start) => {
            let rest = &value[start + 1..];
            &rest[..rest.find('>')?]
        }
        None => value.trim(),
    };
    let stripped = strip_cfws(inner);
    if stripped.is_empty() || stripped.len() > 998 {
        return None;
    }
    let at = stripped.rfind('@')?;
    let (local, domain) = stripped.split_at(at);
    if local.is_empty() || domain.len() < 2 {
        return None;
    }
    Some(format!("{local}{}", domain.to_ascii_lowercase()))
}

/// Removes whitespace, folded line breaks and RFC 5322 comments (nested parentheses, `\` escapes).
fn strip_cfws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' if depth > 0 => {
                chars.next();
            }
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out
}

/// At most this many entries of `References` are stored.
pub const MAX_STORED_REFERENCES: usize = 200;

/// Splits `References` (or `In-Reply-To`) into normalised msg-ids (`1*msg-id`), skipping malformed
/// tokens between valid ones; at most 200 kept.
pub fn split_references(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else { break };
        // A '<' before the '>' starts a new id: the earlier one was malformed.
        let candidate = &after[..end];
        let candidate = match candidate.rfind('<') {
            Some(i) => &candidate[i + 1..],
            None => candidate,
        };
        if let Some(id) = normalise_msg_id(&format!("<{candidate}>")) {
            out.push(id);
            if out.len() == MAX_STORED_REFERENCES {
                break;
            }
        }
        rest = &after[end + 1..];
    }
    out
}

pub const MAX_REFERENCES: usize = 20;

/// RFC 5322 § 3.6.4: the parent's `References` (or its `In-Reply-To` when it has none), then the parent's
/// Message-ID; repeats keep their last occurrence; over 20, the first plus the 19 most recent (C2).
pub fn build_references(
    parent_refs: &[String],
    parent_in_reply_to: Option<&str>,
    parent_id: &str,
) -> Vec<String> {
    let mut v: Vec<String> = if parent_refs.is_empty() {
        parent_in_reply_to
            .map(|s| vec![s.to_owned()])
            .unwrap_or_default()
    } else {
        parent_refs.to_vec()
    };
    v.push(parent_id.to_owned());
    dedupe_keep_last(&mut v);
    if v.len() > MAX_REFERENCES {
        let first = v[0].clone();
        let tail = v.split_off(v.len() - (MAX_REFERENCES - 1));
        v = std::iter::once(first).chain(tail).collect();
    }
    v
}

fn dedupe_keep_last(v: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    let mut kept: Vec<String> = v
        .drain(..)
        .rev()
        .filter(|id| seen.insert(id.clone()))
        .collect();
    kept.reverse();
    *v = kept;
}

/// The `References` header value: `<id>` values separated by one space (the MIME builder folds it).
pub fn references_header(ids: &[String]) -> String {
    ids.iter()
        .map(|id| format!("<{id}>"))
        .collect::<Vec<_>>()
        .join(" ")
}

const REPLY_PREFIXES: &[&str] = &[
    "re",
    "fwd",
    "fw",
    "aw",
    "wg",
    "sv",
    "vs",
    "vb",
    "antw",
    "doorst",
    "tr",
    "rif",
    "i",
    "r",
    "enc",
    "res",
    "rv",
    "odp",
    "pd",
    "ynt",
    "ilt",
    "atb",
    "πληρ",
    "σχετ",
    "ответ",
    "回复",
    "回覆",
    "答复",
    "转发",
    "轉寄",
];

/// Removes one localised reply or forward prefix at the start of `s` (already trimmed), with an
/// optional counter (`[2]`, `(3)`) and an ASCII or full-width colon. Single-letter prefixes need a
/// colon and a space.
fn strip_prefix_once(s: &str) -> Option<&str> {
    for p in REPLY_PREFIXES {
        let Some(rest) = strip_caseless(s, p) else {
            continue;
        };
        let mut rest = rest.trim_start();
        if let Some(r) = counter(rest) {
            rest = r.trim_start();
        }
        let Some(after) = rest.strip_prefix(':').or_else(|| rest.strip_prefix('：')) else {
            continue;
        };
        if p.chars().count() == 1 && !after.starts_with(' ') {
            continue;
        }
        return Some(after.trim_start());
    }
    None
}

/// `s` without `prefix` (lower case), compared character by character in lower case; Turkish `İ`
/// counts as `i`.
fn strip_caseless<'s>(s: &'s str, prefix: &str) -> Option<&'s str> {
    let mut chars = s.char_indices();
    for want in prefix.chars() {
        let (_, c) = chars.next()?;
        let lower = if c == 'İ' {
            'i'
        } else {
            c.to_lowercase().next()?
        };
        if lower != want {
            return None;
        }
    }
    Some(chars.next().map_or("", |(i, _)| &s[i..]))
}

fn counter(s: &str) -> Option<&str> {
    let (open, close) = match s.chars().next()? {
        '[' => ('[', ']'),
        '(' => ('(', ')'),
        _ => return None,
    };
    let inner = &s[open.len_utf8()..];
    let end = inner.find(close)?;
    let digits = &inner[..end];
    ((1..=4).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| &inner[end + 1..])
}

/// The subject for display and for building reply and forward subjects (Threading § 7). Never affects
/// thread membership. Empty stays empty; display shows `(no subject)`.
pub fn normalise_subject(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    // A leading list tag is kept; prefixes after it are removed.
    let (tag, mut rest) = match cleaned.strip_prefix('[').and_then(|r| {
        let end = r.find(']')?;
        let inner = &r[..end];
        (1..=40)
            .contains(&inner.chars().count())
            .then(|| (&cleaned[..end + 2], r[end + 1..].trim_start()))
    }) {
        Some((tag, rest)) => (Some(tag), rest),
        None => (None, cleaned.as_str()),
    };
    for _ in 0..10 {
        match strip_prefix_once(rest) {
            Some(r) => rest = r,
            None => break,
        }
    }
    match tag {
        Some(tag) if rest.is_empty() => tag.to_owned(),
        Some(tag) => format!("{tag} {rest}"),
        None => rest.to_owned(),
    }
}

/// What a thread list shows for an empty subject.
pub const NO_SUBJECT: &str = "(no subject)";

/// `Re: ` + the normalised subject, cut at 998 characters on a character boundary.
pub fn reply_subject(original: &str) -> String {
    cap_subject(format!("Re: {}", normalise_subject(original)))
}

/// `Fwd: ` + the normalised subject.
pub fn forward_subject(original: &str) -> String {
    cap_subject(format!("Fwd: {}", normalise_subject(original)))
}

fn cap_subject(s: String) -> String {
    if s.chars().count() <= 998 {
        s
    } else {
        s.chars().take(998).collect()
    }
}

/// One participant of a thread.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Participant {
    pub address: String,
    pub name: String,
}

pub const MAX_PARTICIPANTS: usize = 50;

/// Adds a message's addresses to the participants list (Threading § 6). `addresses` are already
/// normalised and in the order of the rules (inbound: From, To, Cc; outbound: To, Cc); BCC recipients
/// are never passed. `own` are the identity's addresses (any status) and the journal address.
pub fn add_participants(list: &mut Vec<Participant>, addresses: &[(&str, &str)], own: &[&str]) {
    for &(address, name) in addresses {
        if own.contains(&address) {
            continue;
        }
        let name: String = name.chars().filter(|c| !c.is_control()).take(78).collect();
        if let Some(existing) = list.iter_mut().find(|p| p.address == address) {
            if !name.is_empty() {
                existing.name = name;
            }
        } else if list.len() < MAX_PARTICIPANTS {
            list.push(Participant {
                address: address.to_owned(),
                name,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Db {
        threads: Vec<i64>,
        ids: HashMap<String, i64>,
        participants: Vec<(i64, String)>,
        uids: HashMap<(String, String), i64>,
    }

    impl ThreadLookup for Db {
        fn thread_exists(&self, seq: i64) -> bool {
            self.threads.contains(&seq)
        }
        fn thread_of_message_id(&self, msg_id: &str) -> Option<i64> {
            self.ids.get(msg_id).copied()
        }
        fn is_participant(&self, seq: i64, address: &str) -> bool {
            self.participants
                .iter()
                .any(|(s, a)| *s == seq && a == address)
        }
        fn thread_of_calendar_uid(&self, uid: &str, org: &str, _: Option<&str>) -> Option<i64> {
            self.uids.get(&(uid.to_owned(), org.to_owned())).copied()
        }
    }

    fn inputs<'a>(
        token: TokenCheck,
        irt: Option<&'a str>,
        refs: &'a [String],
    ) -> InboundThreadInputs<'a> {
        InboundThreadInputs {
            token,
            in_reply_to: irt,
            references: refs,
            sender: "ana@example.com",
            sender_authenticated: true,
            calendar_uid_hash: None,
            calendar_organizer: None,
            calendar_replier: None,
        }
    }

    fn valid(seq: u64) -> TokenCheck {
        TokenCheck::Valid {
            seq,
            kid: b'0',
            current: true,
        }
    }

    fn db() -> Db {
        let mut db = Db {
            threads: vec![3, 7],
            ..Db::default()
        };
        db.ids.insert("a@x.example".into(), 3);
        db.ids.insert("b@x.example".into(), 7);
        db.participants.push((3, "ana@example.com".into()));
        db
    }

    #[test]
    fn c1_token_only() {
        let r = resolve_inbound(&inputs(valid(3), None, &[]), &db());
        assert_eq!(
            r,
            ThreadResolution {
                decision: ThreadDecision::Join {
                    seq: 3,
                    via: JoinVia::Token
                },
                join_unverified: false
            }
        );
        // A token for an erased thread is ignored.
        let r = resolve_inbound(&inputs(valid(5), None, &[]), &db());
        assert_eq!(r.decision, ThreadDecision::New);
        // The token wins over headers naming another thread; a non-participant is flagged.
        let mut i = inputs(valid(7), Some("a@x.example"), &[]);
        let r = resolve_inbound(&i, &db());
        assert_eq!(
            r.decision,
            ThreadDecision::Join {
                seq: 7,
                via: JoinVia::Token
            }
        );
        assert!(r.join_unverified);
        // ...unless the headers name the same thread.
        i.in_reply_to = Some("b@x.example");
        assert!(!resolve_inbound(&i, &db()).join_unverified);
    }

    #[test]
    fn c1_no_headers_new_thread() {
        let r = resolve_inbound(&inputs(TokenCheck::Absent, None, &[]), &db());
        assert_eq!(r.decision, ThreadDecision::New);
        assert!(!r.join_unverified);
        let r = resolve_inbound(&inputs(TokenCheck::Invalid, None, &[]), &db());
        assert_eq!(r.decision, ThreadDecision::New);
        assert!(r.join_unverified);
        let r = resolve_inbound(&inputs(TokenCheck::Unchecked, None, &[]), &db());
        assert!(!r.join_unverified);
    }

    #[test]
    fn c1_subject_never_joins() {
        // There is no subject input at all: identical subjects cannot merge threads.
        let r = resolve_inbound(
            &inputs(TokenCheck::Absent, Some("unknown@x.example"), &[]),
            &db(),
        );
        assert_eq!(r.decision, ThreadDecision::New);
    }

    #[test]
    fn c9_header_join_non_participant() {
        let r = resolve_inbound(&inputs(TokenCheck::Absent, Some("a@x.example"), &[]), &db());
        assert_eq!(
            r.decision,
            ThreadDecision::Join {
                seq: 3,
                via: JoinVia::InReplyTo
            }
        );
        assert!(!r.join_unverified);
        let mut i = inputs(TokenCheck::Absent, Some("b@x.example"), &[]);
        assert!(resolve_inbound(&i, &db()).join_unverified);
        i.in_reply_to = Some("a@x.example");
        i.sender = "eve@example.net";
        assert!(resolve_inbound(&i, &db()).join_unverified);
    }

    #[test]
    fn c10_forged_participant() {
        let mut i = inputs(TokenCheck::Absent, Some("a@x.example"), &[]);
        i.sender_authenticated = false;
        assert!(resolve_inbound(&i, &db()).join_unverified);
    }

    #[test]
    fn references_walk_order() {
        let refs: Vec<String> = vec![
            "a@x.example".into(),
            "none@x.example".into(),
            "b@x.example".into(),
        ];
        let r = resolve_inbound(&inputs(TokenCheck::Absent, None, &refs), &db());
        assert_eq!(
            r.decision,
            ThreadDecision::Join {
                seq: 7,
                via: JoinVia::References
            }
        );
        // Only the 50 most recent are looked up.
        let mut many = vec!["a@x.example".to_owned()];
        many.extend((0..50).map(|i| format!("{i}@nowhere.example")));
        let r = resolve_inbound(&inputs(TokenCheck::Absent, None, &many), &db());
        assert_eq!(r.decision, ThreadDecision::New);
    }

    #[test]
    fn calendar_uid_step() {
        let mut db = db();
        db.uids.insert(("h1".into(), "org@example.com".into()), 7);
        let mut i = inputs(TokenCheck::Absent, None, &[]);
        i.calendar_uid_hash = Some("h1");
        i.calendar_organizer = Some("org@example.com");
        let r = resolve_inbound(&i, &db);
        assert_eq!(
            r.decision,
            ThreadDecision::Join {
                seq: 7,
                via: JoinVia::CalendarUid
            }
        );
        assert!(r.join_unverified);
        // Headers decide first.
        i.in_reply_to = Some("a@x.example");
        assert_eq!(
            resolve_inbound(&i, &db).decision,
            ThreadDecision::Join {
                seq: 3,
                via: JoinVia::InReplyTo
            }
        );
        // No organizer, no UID join.
        i.in_reply_to = None;
        i.calendar_organizer = Some("eve@example.net");
        assert_eq!(resolve_inbound(&i, &db).decision, ThreadDecision::New);
    }

    #[test]
    fn msg_id_normalisation() {
        assert_eq!(
            normalise_msg_id(" <Abc.Def@Mail.Example.COM> ").as_deref(),
            Some("Abc.Def@mail.example.com")
        );
        assert_eq!(
            normalise_msg_id("<a (comment) @ x.example>").as_deref(),
            Some("a@x.example")
        );
        assert_eq!(
            normalise_msg_id("a@B@C.example").as_deref(),
            Some("a@B@c.example")
        );
        assert_eq!(normalise_msg_id("<>"), None);
        assert_eq!(normalise_msg_id("<no-at>"), None);
        assert_eq!(
            normalise_msg_id(&format!("<{}@x.example>", "a".repeat(990))),
            None
        );
        assert_eq!(
            split_references("<a@x.example> junk <b@X.example>\r\n <c@x.example"),
            vec!["a@x.example", "b@x.example"]
        );
        assert_eq!(split_references("<bad <b@x.example>"), vec!["b@x.example"]);
        let long: String = (0..250).map(|i| format!("<{i}@x.example> ")).collect();
        assert_eq!(split_references(&long).len(), 200);
    }

    #[test]
    fn c2_trim_references() {
        let refs: Vec<String> = (0..150).map(|i| format!("{i}@x.example")).collect();
        let mut with_dup = refs.clone();
        with_dup.insert(5, "149@x.example".into());
        let out = build_references(&with_dup, None, "p@x.example");
        assert_eq!(out.len(), 20);
        assert_eq!(out[0], "0@x.example");
        assert_eq!(out[1], "132@x.example");
        assert_eq!(out[18], "149@x.example");
        assert_eq!(out[19], "p@x.example");
        let unique: std::collections::HashSet<_> = out.iter().collect();
        assert_eq!(unique.len(), 20);
        assert_eq!(
            references_header(&out[..2]),
            "<0@x.example> <132@x.example>"
        );
    }

    #[test]
    fn c2_rfc5322_parent_rules() {
        assert_eq!(
            build_references(&[], Some("i@x.example"), "p@x.example"),
            vec!["i@x.example", "p@x.example"]
        );
        assert_eq!(
            build_references(&[], None, "p@x.example"),
            vec!["p@x.example"]
        );
        let refs = vec!["p@x.example".to_owned(), "q@x.example".to_owned()];
        assert_eq!(
            build_references(&refs, None, "p@x.example"),
            vec!["q@x.example", "p@x.example"]
        );
    }

    #[test]
    fn c8_prefixes() {
        for (input, want) in [
            ("Re: hello", "hello"),
            ("RE: Re: Fwd: hello", "hello"),
            ("Fw: hello", "hello"),
            ("AW: WG: hello", "hello"),
            ("SV: VS: VB: hello", "hello"),
            ("Antw: Doorst: hello", "hello"),
            ("TR: hello", "hello"),
            ("R: RIF: I: hello", "hello"),
            ("RES: RV: ENC: hello", "hello"),
            ("Odp: PD: hello", "hello"),
            ("YNT: İLT: hello", "hello"),
            ("ΣΧΕΤ: ΠΛΗΡ: hello", "hello"),
            ("Ответ: hello", "hello"),
            ("回复：hello", "hello"),
            ("回覆：答复：转发：轉寄：hello", "hello"),
            ("Re[2]: Re(3): hello", "hello"),
            ("Re : hello", "hello"),
            ("[acme-ops] Re: Fwd: x", "[acme-ops] x"),
            ("  Re:\t\r\n  spaced   out ", "spaced out"),
            ("R2D2", "R2D2"),
            ("R:drive", "R:drive"),
            ("Reply needed", "Reply needed"),
            ("Re:", ""),
            ("", ""),
        ] {
            assert_eq!(normalise_subject(input), want, "{input}");
        }
        let ten = "Re: ".repeat(12) + "x";
        assert_eq!(normalise_subject(&ten), "Re: Re: x");
        assert_eq!(reply_subject("RE: Re: booking"), "Re: booking");
        assert_eq!(forward_subject("Fwd: booking"), "Fwd: booking");
        assert_eq!(reply_subject(&"x".repeat(2000)).chars().count(), 998);
    }

    #[test]
    fn participants_never_bcc() {
        let mut list = Vec::new();
        add_participants(
            &mut list,
            &[
                ("ana@example.com", "Ana"),
                ("me@agents.example", "Me"),
                ("bo@example.com", ""),
            ],
            &["me@agents.example"],
        );
        assert_eq!(list.len(), 2);
        add_participants(&mut list, &[("bo@example.com", "Bo\u{7}")], &[]);
        assert_eq!(list[1].name, "Bo");
        add_participants(&mut list, &[("ana@example.com", "")], &[]);
        assert_eq!(list[0].name, "Ana");
        let many: Vec<String> = (0..60).map(|i| format!("{i}@example.com")).collect();
        let pairs: Vec<(&str, &str)> = many.iter().map(|a| (a.as_str(), "")).collect();
        add_participants(&mut list, &pairs, &[]);
        assert_eq!(list.len(), 50);
    }
}
