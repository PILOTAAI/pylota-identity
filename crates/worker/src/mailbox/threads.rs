//! Threads in the mailbox ([Threading § 3–6]): the lookups `core::thread::resolve_inbound` asks, the new
//! thread row, and the recompute of a thread's visible columns.
//!
//! [Threading § 3–6]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/threading.md

use pylota_identity_core::thread::{self, Participant, ThreadLookup};
use pylota_identity_platform::PResult;
use pylota_identity_platform::durable::Sql;
use serde::Deserialize;

#[derive(Deserialize)]
struct Seq {
    seq: i64,
}

#[derive(Deserialize)]
struct One {
    #[allow(dead_code)]
    x: i64,
}

/// [`ThreadLookup`] over the mailbox's SQLite; a failed query reads as "not found".
pub struct Lookup<'a, S: Sql>(pub &'a S);

impl<S: Sql> ThreadLookup for Lookup<'_, S> {
    fn thread_exists(&self, seq: i64) -> bool {
        self.0
            .first::<One>("SELECT 1 AS x FROM threads WHERE seq = ?1", &[seq.into()])
            .ok()
            .flatten()
            .is_some()
    }

    fn thread_of_message_id(&self, msg_id: &str) -> Option<i64> {
        self.0
            .first::<Seq>(
                "SELECT thread_seq AS seq FROM messages WHERE rfc_message_id = ?1 ORDER BY rowid DESC LIMIT 1",
                &[msg_id.into()],
            )
            .ok()
            .flatten()
            .map(|r| r.seq)
    }

    fn is_participant(&self, seq: i64, address: &str) -> bool {
        participants(self.0, seq)
            .ok()
            .is_some_and(|l| l.iter().any(|p| p.address == address))
            || self
                .0
                .first::<One>(
                    "SELECT 1 AS x FROM messages WHERE thread_seq = ?1 AND from_address = ?2 LIMIT 1",
                    &[seq.into(), address.into()],
                )
                .ok()
                .flatten()
                .is_some()
    }

    fn thread_of_calendar_uid(
        &self,
        _uid: &str,
        _organizer: &str,
        _replier: Option<&str>,
    ) -> Option<i64> {
        // Calendar threading arrives with its milestone (Calendar invitations §4.3).
        None
    }
}

#[derive(Deserialize)]
struct P {
    participants_json: String,
}

/// The stored participants of a thread.
pub fn participants<S: Sql>(s: &S, seq: i64) -> PResult<Vec<Participant>> {
    Ok(s.first::<P>(
        "SELECT participants_json FROM threads WHERE seq = ?1",
        &[seq.into()],
    )?
    .and_then(|p| serde_json::from_str(&p.participants_json).ok())
    .unwrap_or_default())
}

/// A new thread (Threading § 3.4): `subject` is empty unless the first message is visible.
pub fn insert<S: Sql>(
    s: &S,
    id: &str,
    subject: &str,
    at: i64,
    reply_from: Option<&str>,
) -> PResult<i64> {
    let row: Option<Seq> = s.first(
        "INSERT INTO threads (id, subject, first_at, last_at, last_inbound_at, message_count, unread_count, participants_json, reply_from_address) VALUES (?1, ?2, ?3, ?3, NULL, 0, 0, '[]', ?4) RETURNING seq",
        &[id.into(), subject.into(), at.into(), reply_from.map(str::to_owned).into()],
    )?;
    row.map(|r| r.seq).ok_or_else(|| {
        pylota_identity_platform::PlatformError::new(
            pylota_identity_platform::PlatformErrorKind::Internal,
            "sql",
            "no seq returned",
        )
    })
}

#[derive(Deserialize)]
struct Subj {
    subject: Option<String>,
}

/// Recomputes the visible columns after a visible message was added (or released), adding its
/// addresses to the participants. `reply_from` replaces `reply_from_address` when set.
pub fn recompute<S: Sql>(
    s: &S,
    seq: i64,
    addresses: &[(&str, &str)],
    own: &[&str],
    reply_from: Option<&str>,
    unarchive: bool,
) -> PResult<()> {
    let mut list = participants(s, seq)?;
    thread::add_participants(&mut list, addresses, own);
    let first: Option<Subj> = s.first(
        "SELECT subject FROM messages WHERE thread_seq = ?1 AND status NOT IN ('quarantined','screening','hidden','throttled') ORDER BY received_at, rowid LIMIT 1",
        &[seq.into()],
    )?;
    let subject = thread::normalise_subject(first.and_then(|f| f.subject).as_deref().unwrap_or(""));
    s.exec(
        "UPDATE threads SET \
           message_count = (SELECT COUNT(*) FROM messages WHERE thread_seq = ?1 AND status NOT IN ('quarantined','screening','hidden','throttled')), \
           unread_count = (SELECT COUNT(*) FROM messages WHERE thread_seq = ?1 AND direction = 'inbound' AND status = 'received' AND read = 0), \
           last_at = COALESCE((SELECT MAX(received_at) FROM messages WHERE thread_seq = ?1 AND status NOT IN ('quarantined','screening','hidden','throttled')), first_at), \
           last_inbound_at = (SELECT MAX(received_at) FROM messages WHERE thread_seq = ?1 AND direction = 'inbound' AND status = 'received'), \
           subject = CASE WHEN subject = '' THEN ?2 ELSE subject END, \
           participants_json = ?3, \
           reply_from_address = COALESCE(?4, reply_from_address), \
           archived = CASE WHEN ?5 = 1 THEN 0 ELSE archived END \
         WHERE seq = ?1",
        &[
            seq.into(),
            subject.into(),
            serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()).into(),
            reply_from.map(str::to_owned).into(),
            i64::from(unarchive).into(),
        ],
    )?;
    Ok(())
}

/// Recomputes only the read counter (labels and read state).
pub fn recount_unread<S: Sql>(s: &S, seq: i64) -> PResult<()> {
    s.exec(
        "UPDATE threads SET unread_count = (SELECT COUNT(*) FROM messages WHERE thread_seq = ?1 AND direction = 'inbound' AND status = 'received' AND read = 0) WHERE seq = ?1",
        &[seq.into()],
    )?;
    Ok(())
}
