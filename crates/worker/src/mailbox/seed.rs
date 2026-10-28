//! `itest-hooks` only: `POST /__test/bulk-seed` (Testing § 6.2, § 6.9). Synthetic messages from a
//! seeded generator written straight into the mailbox (thread, message, FTS rows, references) and
//! marked embedded, for the search benchmarks; the test process seeds the fake Vectorize with the same
//! texts. No `email()`, no events, no webhooks.

use pylota_identity_api_types::ids::{IdPrefix, format as format_id};
use pylota_identity_core::search::fts_doc::{self, MessageForIndex};
use pylota_identity_platform::PResult;
use pylota_identity_platform::durable::Sql;
use serde_json::{Value, json};

const WORDS: &[&str] = &[
    "booking",
    "invoice",
    "van",
    "car",
    "hire",
    "return",
    "deposit",
    "insurance",
    "damage",
    "fuel",
    "mileage",
    "pickup",
    "dropoff",
    "airport",
    "station",
    "keys",
    "contract",
    "renewal",
    "payment",
    "refund",
    "receipt",
    "quote",
    "fleet",
    "service",
    "tyres",
    "brakes",
    "inspection",
    "garage",
    "weekend",
    "monday",
    "tuesday",
    "friday",
    "morning",
    "evening",
    "late",
    "early",
    "extension",
    "driver",
    "licence",
    "passport",
    "address",
    "phone",
    "customer",
    "account",
    "balance",
    "overdue",
    "reminder",
    "confirmation",
    "cancellation",
    "upgrade",
    "estate",
    "electric",
    "charging",
    "cable",
    "child",
    "seat",
    "roof",
    "rack",
    "toll",
    "parking",
    "fine",
    "penalty",
    "appeal",
    "photos",
    "report",
    "scratch",
    "dent",
    "windscreen",
    "mirror",
    "battery",
    "recovery",
    "breakdown",
    "accident",
    "claim",
    "excess",
    "waiver",
    "policy",
    "terms",
    "agreement",
    "signature",
    "schedule",
    "delivery",
    "collection",
    "office",
    "manager",
    "team",
    "thanks",
    "please",
    "urgent",
    "today",
    "tomorrow",
    "next",
    "week",
];

/// SplitMix64.
fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Filler words drawn besides the domain vocabulary.
const FILLER: u64 = 3_000;

/// `n` words: one in five from the domain vocabulary, the rest from 3,000 filler words, so a domain
/// term appears in about one message in ten, as a real mailbox's subject-matter words do.
fn words(state: &mut u64, n: usize) -> String {
    (0..n)
        .map(|_| {
            *state = mix(*state);
            if (*state).is_multiple_of(5) {
                WORDS[((*state >> 8) % WORDS.len() as u64) as usize].to_owned()
            } else {
                let k = (*state >> 8) % FILLER;
                format!("w{k:04x}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Writes messages `start..start + count`; answers `[{id, thread_id, text, sent_at}]` (`sent_at` in
/// Unix seconds) for the fake Vectorize.
pub fn bulk_seed<S: Sql>(
    s: &S,
    start: u32,
    count: u32,
    seed: u64,
    now: i64,
) -> PResult<Vec<Value>> {
    #[derive(serde::Deserialize)]
    struct Seq {
        seq: i64,
    }
    #[derive(serde::Deserialize)]
    struct Row {
        rowid: i64,
    }
    let mut out = Vec::with_capacity(count as usize);
    for i in start..start + count {
        let mut st = mix(seed ^ u64::from(i));
        let subject = words(&mut st, 5);
        let mut body = words(&mut st, 45);
        let invoice = 10_000 + (st % 90_000);
        if i.is_multiple_of(7) {
            body.push_str(&format!(" invoice {invoice}"));
        }
        let from = format!("sender{}@vendor{}.example", st % 300, st % 40);
        let at = now - i64::from(count + start - i) * 60_000;
        let id = format_id(
            IdPrefix::Message,
            at as u64,
            u128::from(mix(st)) << 16 | u128::from(i),
        );
        let thread = format_id(
            IdPrefix::Thread,
            at as u64,
            u128::from(mix(st ^ 1)) << 16 | u128::from(i),
        );
        let participants = json!([{ "address": from, "name": "" }]).to_string();
        let seq: Option<Seq> = s.first(
            "INSERT INTO threads (id, subject, first_at, last_at, message_count, unread_count, participants_json, last_inbound_at) VALUES (?1, ?2, ?3, ?3, 1, 1, ?4, ?3) RETURNING seq",
            &[thread.clone().into(), subject.clone().into(), at.into(), participants.into()],
        )?;
        let seq = seq.map_or(0, |x| x.seq);
        let row: Option<Row> = s.first(
            "INSERT INTO messages (id, thread_seq, direction, status, raw_sha256, raw_r2_key, raw_size, from_address, from_name, sender_domain, to_json, subject, text, extracted_text, snippet, sent_at, received_at, kind, verdict, known_sender, read, triage_status, parser_version, metadata_json, received_evented_at) \
             VALUES (?1, ?2, 'inbound', 'received', '', '', 0, ?3, '', ?4, '[]', ?5, ?6, ?6, ?7, ?8, ?8, 'normal', 'none', 0, 0, 'pending', 1, '{}', ?8) RETURNING rowid",
            &[
                id.clone().into(),
                seq.into(),
                from.clone().into(),
                from.rsplit('@').next().unwrap_or("").to_owned().into(),
                subject.clone().into(),
                body.clone().into(),
                body.chars().take(200).collect::<String>().into(),
                at.into(),
            ],
        )?;
        let rowid = row.map_or(0, |r| r.rowid);
        let refs: Vec<String> = if i.is_multiple_of(7) {
            vec![invoice.to_string()]
        } else {
            Vec::new()
        };
        let doc = fts_doc::fts_doc(&MessageForIndex {
            subject: &subject,
            from_name: "",
            from_address: &from,
            recipients: &[],
            delivered_to: "",
            extracted_text: &body,
            text: &body,
            attachments: &[],
            refs: &refs,
        });
        s.exec(
            "INSERT INTO fts (rowid, subject, participants, body_new, body_full, attachments, refs) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            &[
                rowid.into(),
                doc.subject.clone().into(),
                doc.participants.clone().into(),
                doc.body_new.into(),
                doc.body_full.into(),
                doc.attachments.into(),
                doc.refs.clone().into(),
            ],
        )?;
        s.exec(
            "INSERT INTO fts_tri (rowid, subject, participants, refs) VALUES (?1, ?2, ?3, ?4)",
            &[
                rowid.into(),
                doc.subject.into(),
                doc.participants.into(),
                doc.refs.into(),
            ],
        )?;
        for r in &refs {
            s.exec(
                "INSERT OR IGNORE INTO refs (message_rowid, kind, value, source) VALUES (?1, 'invoice', ?2, 'body')",
                &[rowid.into(), r.clone().into()],
            )?;
        }
        let len = body.chars().count() as i64;
        s.exec(
            "INSERT INTO chunks (vector_id, message_rowid, attachment_id, ordinal, page, char_start, char_end, model, status, updated_at) VALUES (?1, ?2, NULL, 0, NULL, 0, ?3, 'bge-m3@1', 'embedded', ?4)",
            &[format!("{id}:0").into(), rowid.into(), len.into(), now.into()],
        )?;
        out.push(json!({
            "id": id, "thread_id": thread, "sent_at": at / 1000,
            "text": format!("Subject: {subject}\n\n{body}"),
        }));
    }
    Ok(out)
}
