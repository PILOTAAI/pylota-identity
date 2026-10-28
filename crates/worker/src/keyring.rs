//! The deployment's thread, link and cursor keys ([Security §6.2], Rust workspace § 6.1): 32-byte HMAC
//! keys sealed under the master key in D1 `signing_keys`, created on first use, opened with whichever
//! master-key slot matches the envelope's kid, and cached per isolate for 5 minutes. Kids are single
//! lower-case Crockford characters in alphabet order, wrapping after `z`. (`web_bot_auth` keys are
//! Ed25519 seeds and arrive with signed HTTP requests, M25.)
//!
//! [Security §6.2]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/security.md

use std::cell::RefCell;

use pylota_identity_core::{crypto, keys::ALPHABET};
use pylota_identity_platform::d1::{SqlValue, Stmt};
use pylota_identity_platform::{Clock as _, ControlDb as _, Platform, Rng as _};
use serde::Deserialize;

use crate::errors::ApiError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Purpose {
    Thread,
    Link,
    Cursor,
}

impl Purpose {
    pub const fn as_str(self) -> &'static str {
        match self {
            Purpose::Thread => "thread",
            Purpose::Link => "link",
            Purpose::Cursor => "cursor",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "thread" => Purpose::Thread,
            "link" => Purpose::Link,
            "cursor" => Purpose::Cursor,
            _ => return None,
        })
    }

    /// How long a rotated-out key keeps verifying.
    pub const fn verify_window_ms(self) -> i64 {
        match self {
            Purpose::Thread => 90 * 86_400_000,
            Purpose::Link => 7 * 86_400_000,
            Purpose::Cursor => 86_400_000,
        }
    }
}

/// One opened key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub kid: String,
    pub secret: [u8; 32],
    /// `None` for the current key.
    pub verify_until: Option<i64>,
}

#[derive(Deserialize)]
struct Row {
    kid: String,
    ciphertext: Vec<u8>,
    verify_until: Option<i64>,
}

const CACHE_MS: i64 = 5 * 60_000;

thread_local! {
    static CACHE: RefCell<Vec<(Purpose, i64, Vec<Key>)>> = const { RefCell::new(Vec::new()) };
}

fn aad(purpose: Purpose, kid: &str) -> String {
    crypto::aad(
        "signing_keys",
        "ciphertext",
        &format!("{}:{kid}", purpose.as_str()),
    )
}

fn open<P: Platform>(p: &P, purpose: Purpose, row: &Row) -> Result<Key, ApiError> {
    let s = p.secrets();
    let slots: Vec<&[u8; 32]> = [s.master_key_a.as_ref(), s.master_key_b.as_ref()]
        .into_iter()
        .flatten()
        .collect();
    let envelope = std::str::from_utf8(&row.ciphertext).map_err(|_| ApiError::internal())?;
    let secret = crypto::open(&slots, &aad(purpose, &row.kid), envelope).map_err(|e| {
        crate::log::error(
            "keyring_open_failed",
            &[
                ("purpose", purpose.as_str().into()),
                ("reason", format!("{e:?}").into()),
            ],
        );
        ApiError::internal()
    })?;
    Ok(Key {
        kid: row.kid.clone(),
        secret: secret.try_into().map_err(|_| ApiError::internal())?,
        verify_until: row.verify_until,
    })
}

fn seal<P: Platform>(p: &P, purpose: Purpose, kid: &str) -> Vec<u8> {
    let secret: [u8; 32] = p.rng().bytes();
    let nonce: [u8; 12] = p.rng().bytes();
    crypto::seal(
        p.secrets().active_master_key(),
        nonce,
        &aad(purpose, kid),
        &secret,
    )
    .into_bytes()
}

async fn load<P: Platform>(p: &P, purpose: Purpose) -> Result<Vec<Key>, ApiError> {
    let now = p.clock().now_ms();
    let rows: Vec<Row> = p
        .db()
        .query(Stmt::new(
            "SELECT kid, ciphertext, verify_until FROM signing_keys WHERE purpose = ?1 AND (verify_until IS NULL OR verify_until > ?2) ORDER BY created_at DESC",
            vec![purpose.as_str().into(), now.into()],
        ))
        .await?;
    rows.iter().map(|r| open(p, purpose, r)).collect()
}

/// The current key and every key still inside its verify window, current first. Creates the first key
/// of the purpose on first use: concurrent first uses insert the same kid, and `INSERT OR IGNORE` keeps
/// one of them, which every caller then reads.
pub async fn keys<P: Platform>(p: &P, purpose: Purpose) -> Result<Vec<Key>, ApiError> {
    let now = p.clock().now_ms();
    if let Some(hit) = CACHE.with(|c| {
        c.borrow()
            .iter()
            .find(|(pp, at, _)| *pp == purpose && now - at < CACHE_MS)
            .map(|(_, _, k)| k.clone())
    }) {
        return Ok(hit);
    }
    let mut found = load(p, purpose).await?;
    if !found.iter().any(|k| k.verify_until.is_none()) {
        let kid = "0";
        p.db()
            .execute(Stmt::new(
                "INSERT OR IGNORE INTO signing_keys (purpose, kid, ciphertext, public_jwk, created_at, verify_until) VALUES (?1, ?2, ?3, NULL, ?4, NULL)",
                vec![purpose.as_str().into(), kid.into(), SqlValue::Blob(seal(p, purpose, kid)), now.into()],
            ))
            .await?;
        found = load(p, purpose).await?;
    }
    found.sort_by_key(|k| k.verify_until.is_some());
    if found.first().is_none_or(|k| k.verify_until.is_some()) {
        return Err(ApiError::internal());
    }
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.retain(|(pp, _, _)| *pp != purpose);
        c.push((purpose, now, found.clone()));
    });
    Ok(found)
}

/// The key that signs now.
pub async fn current<P: Platform>(p: &P, purpose: Purpose) -> Result<Key, ApiError> {
    Ok(keys(p, purpose).await?.remove(0))
}

/// The kid after `kid`, in alphabet order, wrapping after `z`.
pub fn next_kid(kid: &str) -> String {
    let i = kid
        .bytes()
        .next()
        .and_then(|b| ALPHABET.iter().position(|a| *a == b))
        .unwrap_or(0);
    char::from(ALPHABET[(i + 1) % ALPHABET.len()]).to_string()
}

/// The outcome of a rotation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rotated {
    pub kid: String,
    pub created_at: i64,
    pub previous_kid: String,
    pub previous_verify_until: i64,
    pub revoked: bool,
}

/// `POST /v1/platform/keys/{purpose}/rotate`: one D1 batch.
pub async fn rotate<P: Platform>(
    p: &P,
    purpose: Purpose,
    revoke_previous: bool,
) -> Result<Rotated, ApiError> {
    let prev = current(p, purpose).await?;
    let now = p.clock().now_ms();
    let kid = next_kid(&prev.kid);
    let verify_until = if revoke_previous {
        now
    } else {
        now + purpose.verify_window_ms()
    };
    let mut batch = vec![
        Stmt::new(
            "DELETE FROM signing_keys WHERE purpose = ?1 AND kid = ?2",
            vec![purpose.as_str().into(), kid.clone().into()],
        ),
        Stmt::new(
            "INSERT INTO signing_keys (purpose, kid, ciphertext, public_jwk, created_at, verify_until) VALUES (?1, ?2, ?3, NULL, ?4, NULL)",
            vec![
                purpose.as_str().into(),
                kid.clone().into(),
                SqlValue::Blob(seal(p, purpose, &kid)),
                now.into(),
            ],
        ),
    ];
    batch.insert(
        0,
        if revoke_previous {
            Stmt::new(
                "DELETE FROM signing_keys WHERE purpose = ?1 AND kid = ?2",
                vec![purpose.as_str().into(), prev.kid.clone().into()],
            )
        } else {
            Stmt::new(
                "UPDATE signing_keys SET verify_until = ?3 WHERE purpose = ?1 AND kid = ?2",
                vec![
                    purpose.as_str().into(),
                    prev.kid.clone().into(),
                    verify_until.into(),
                ],
            )
        },
    );
    p.db().batch(batch).await?;
    CACHE.with(|c| c.borrow_mut().retain(|(pp, _, _)| *pp != purpose));
    Ok(Rotated {
        kid,
        created_at: now,
        previous_kid: prev.kid,
        previous_verify_until: verify_until,
        revoked: revoke_previous,
    })
}

#[cfg(test)]
mod tests {
    use pylota_identity_platform::fakes::{FakePlatform, block_on};

    use super::*;

    fn migrated() -> FakePlatform {
        let p = FakePlatform::new();
        p.db.exec_script(include_str!("../../../migrations/d1/0001_init.sql"))
            .unwrap();
        p
    }

    /// One key per purpose, created on first use and opened with the master key; a rotation keeps the
    /// previous key verifying, and a revoking one drops it.
    #[test]
    fn first_use_and_rotation() {
        let p = migrated();
        let a = block_on(current(&p, Purpose::Thread)).unwrap();
        assert_eq!(a.kid, "0");
        // A second isolate (an empty cache) reads the same key.
        CACHE.with(|c| c.borrow_mut().clear());
        assert_eq!(block_on(current(&p, Purpose::Thread)).unwrap(), a);
        let link = block_on(current(&p, Purpose::Link)).unwrap();
        assert_ne!(link.secret, a.secret);
        let n: Vec<serde_json::Value> =
            block_on(p.db.query(Stmt::new("SELECT COUNT(*) AS n FROM signing_keys", vec![])))
                .unwrap();
        assert_eq!(n[0]["n"], 2);

        let r = block_on(rotate(&p, Purpose::Thread, false)).unwrap();
        assert_eq!((r.kid.as_str(), r.previous_kid.as_str()), ("1", "0"));
        let ks = block_on(keys(&p, Purpose::Thread)).unwrap();
        assert_eq!(
            ks.iter().map(|k| k.kid.as_str()).collect::<Vec<_>>(),
            ["1", "0"]
        );
        let r = block_on(rotate(&p, Purpose::Thread, true)).unwrap();
        assert!(r.revoked);
        let ks = block_on(keys(&p, Purpose::Thread)).unwrap();
        assert_eq!(
            ks.iter().map(|k| k.kid.as_str()).collect::<Vec<_>>(),
            ["2", "0"]
        );
        assert_eq!(next_kid("z"), "0");
    }
}
