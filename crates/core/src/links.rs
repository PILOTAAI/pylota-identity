//! Signed links ([Security § 7.3]): `base64url(payload || HMAC-SHA256(link key, payload)[0..16])` with
//! `payload = "l1:{kid}:att:{tenant}:{identity}:{message}:{attachment}:{expires_unix_s}"`. Only the
//! `att` form, and the `export` form of Privacy § 9.4:
//! `payload = "l1:{kid}:export:{tenant}:{export}:{nonce}:{expires_unix_s}"`; uploads add theirs with
//! their milestone.
//!
//! [Security § 7.3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/security.md

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{KeyInit, Mac};
use sha2::Sha256;

/// The target of an `att` link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttLink {
    pub kid: char,
    pub tenant_id: String,
    pub identity_id: String,
    pub message_id: String,
    pub attachment_id: String,
    pub expires_s: i64,
}

fn tag(key: &[u8; 32], payload: &[u8]) -> [u8; 16] {
    let mut m = <hmac::Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("any key length");
    m.update(payload);
    let full = m.finalize().into_bytes();
    let mut out = [0u8; 16];
    out.copy_from_slice(&full[..16]);
    out
}

/// The token of `link` signed with `key`.
pub fn mint(key: &[u8; 32], link: &AttLink) -> String {
    let payload = format!(
        "l1:{}:att:{}:{}:{}:{}:{}",
        link.kid,
        link.tenant_id,
        link.identity_id,
        link.message_id,
        link.attachment_id,
        link.expires_s
    );
    let mut bytes = payload.into_bytes();
    let t = tag(key, &bytes);
    bytes.extend_from_slice(&t);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The kid of a token, read before its key is looked up.
pub fn kid(token: &str) -> Option<char> {
    let bytes = URL_SAFE_NO_PAD.decode(token).ok()?;
    let s = std::str::from_utf8(bytes.get(..bytes.len().checked_sub(16)?)?).ok()?;
    s.strip_prefix("l1:")?.chars().next()
}

/// Verifies a token with the key of its kid (constant-time MAC) and the expiry against `now_s`.
pub fn verify(key: &[u8; 32], token: &str, now_s: i64) -> Option<AttLink> {
    let bytes = URL_SAFE_NO_PAD.decode(token).ok()?;
    let split = bytes.len().checked_sub(16)?;
    let (payload, mac) = bytes.split_at(split);
    let mut m = <hmac::Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("any key length");
    m.update(payload);
    m.verify_truncated_left(mac).ok()?;
    let s = std::str::from_utf8(payload).ok()?;
    let parts: Vec<&str> = s.split(':').collect();
    let [l1, kid, att, tenant, identity, message, attachment, expires] = parts.as_slice() else {
        return None;
    };
    if *l1 != "l1" || *att != "att" || kid.chars().count() != 1 {
        return None;
    }
    let expires_s: i64 = expires.parse().ok()?;
    if expires_s <= now_s {
        return None;
    }
    Some(AttLink {
        kid: kid.chars().next()?,
        tenant_id: (*tenant).to_owned(),
        identity_id: (*identity).to_owned(),
        message_id: (*message).to_owned(),
        attachment_id: (*attachment).to_owned(),
        expires_s,
    })
}

/// The target of an `export` link (Privacy § 9.4); its nonce makes it work once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportLink {
    pub kid: char,
    pub tenant_id: String,
    pub export_id: String,
    pub nonce: String,
    pub expires_s: i64,
}

pub fn mint_export(key: &[u8; 32], link: &ExportLink) -> String {
    let payload = format!(
        "l1:{}:export:{}:{}:{}:{}",
        link.kid, link.tenant_id, link.export_id, link.nonce, link.expires_s
    );
    let mut bytes = payload.into_bytes();
    let t = tag(key, &bytes);
    bytes.extend_from_slice(&t);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Verifies an `export` token as [`verify`] does an `att` one.
pub fn verify_export(key: &[u8; 32], token: &str, now_s: i64) -> Option<ExportLink> {
    let bytes = URL_SAFE_NO_PAD.decode(token).ok()?;
    let split = bytes.len().checked_sub(16)?;
    let (payload, mac) = bytes.split_at(split);
    let mut m = <hmac::Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("any key length");
    m.update(payload);
    m.verify_truncated_left(mac).ok()?;
    let s = std::str::from_utf8(payload).ok()?;
    let parts: Vec<&str> = s.split(':').collect();
    let [l1, kid, kind, tenant, export, nonce, expires] = parts.as_slice() else {
        return None;
    };
    if *l1 != "l1" || *kind != "export" || kid.chars().count() != 1 {
        return None;
    }
    let expires_s: i64 = expires.parse().ok()?;
    if expires_s <= now_s {
        return None;
    }
    Some(ExportLink {
        kid: kid.chars().next()?,
        tenant_id: (*tenant).to_owned(),
        export_id: (*export).to_owned(),
        nonce: (*nonce).to_owned(),
        expires_s,
    })
}

/// Standard padded base64.
pub fn encode_base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Standard base64 (an attachment's `content_base64`); whitespace is ignored.
pub fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD.decode(clean).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_tamper() {
        let key = [7u8; 32];
        let link = AttLink {
            kid: '1',
            tenant_id: "ten_a".into(),
            identity_id: "idn_b".into(),
            message_id: "msg_c".into(),
            attachment_id: "att_d".into(),
            expires_s: 2_000,
        };
        let t = mint(&key, &link);
        assert_eq!(kid(&t), Some('1'));
        assert_eq!(verify(&key, &t, 1_000), Some(link));
        assert_eq!(verify(&key, &t, 2_000), None, "expired");
        assert_eq!(verify(&[8u8; 32], &t, 1_000), None, "another key");
        let mut b = URL_SAFE_NO_PAD.decode(&t).unwrap();
        b[5] ^= 1;
        assert_eq!(verify(&key, &URL_SAFE_NO_PAD.encode(b), 1_000), None);
    }
}
