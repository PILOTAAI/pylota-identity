//! The console's pure rules ([Console and workspaces]): roles and their permission sets, keyed hashes of
//! tokens and cookie values, sign-in codes, CSRF tokens and session lifetimes. No clock, randomness or
//! I/O: callers pass them in.
//!
//! [Console and workspaces]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{KeyInit, Mac};
use sha2::Sha256;

/// A workspace role (Console › Roles).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Viewer,
    Member,
    Admin,
    Owner,
}

const VIEWER: &[&str] = &[
    "identities:read",
    "domains:read",
    "messages:read",
    "attachments:read",
    "search:read",
    "usage:read",
    "members:read",
    "metrics:read",
];

const MEMBER_EXTRA: &[&str] = &[
    "messages:write",
    "search:agentic",
    "quarantine:review",
    "accounts:request",
    "drafts:read",
];

const ADMIN_EXTRA: &[&str] = &[
    "identities:write",
    "identities:sign",
    "domains:write",
    "messages:send",
    "webhooks:read",
    "webhooks:manage",
    "keys:manage",
    "erasure:manage",
    "suppressions:manage",
    "audit:read",
    "members:manage",
    "policy:write",
    "accounts:approve",
    "drafts:write",
    "drafts:approve",
    "events:read",
];

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
            Role::Viewer => "viewer",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "owner" => Role::Owner,
            "admin" => Role::Admin,
            "member" => Role::Member,
            "viewer" => Role::Viewer,
            _ => return None,
        })
    }

    /// The permission set of the role's session principal (Console › Roles, second table).
    pub fn permissions(self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = VIEWER.to_vec();
        if self >= Role::Member {
            out.extend(MEMBER_EXTRA);
        }
        if self >= Role::Admin {
            out.extend(ADMIN_EXTRA);
        }
        if self == Role::Owner {
            out.push("tenants:erase");
        }
        out
    }
}

/// `HMAC-SHA256(key, value)` as lower-case hex: how tokens, codes and cookie values are stored
/// (Console › Keyed hashes).
pub fn keyed_hash(key: &[u8; 32], value: &str) -> String {
    let mut m = <hmac::Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("any key length");
    m.update(value.as_bytes());
    hex(&m.finalize().into_bytes())
}

/// The code's keyed hash binds it to the address: `HMAC(key, email || code)`.
pub fn code_hash(key: &[u8; 32], email: &str, code: &str) -> String {
    keyed_hash(key, &format!("{email}{code}"))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A token or cookie value: the key's one-character kid, then 32 random bytes in base64url.
pub fn token(kid: &str, random: &[u8; 32]) -> String {
    format!("{kid}{}", URL_SAFE_NO_PAD.encode(random))
}

/// The kid a token starts with, when it has the token's shape.
pub fn token_kid(token: &str) -> Option<&str> {
    (token.len() == 44 && token.is_ascii()).then(|| &token[..1])
}

/// A six-digit code from successive 32-bit draws, uniform by rejection sampling: `None` when every draw
/// fell in the biased tail (the caller draws again).
pub fn code_from(draws: &[u32]) -> Option<String> {
    const LIMIT: u32 = u32::MAX - (u32::MAX % 1_000_000);
    draws
        .iter()
        .find(|d| **d < LIMIT)
        .map(|d| format!("{:06}", d % 1_000_000))
}

/// `base64url(HMAC-SHA256(csrf_secret, "console-form"))`.
pub fn csrf_token(csrf_secret: &str) -> String {
    let mut m = <hmac::Hmac<Sha256> as KeyInit>::new_from_slice(csrf_secret.as_bytes())
        .expect("any key length");
    m.update(b"console-form");
    URL_SAFE_NO_PAD.encode(m.finalize().into_bytes())
}

/// Constant-time equality of two strings.
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

const DAY_MS: i64 = 86_400_000;

/// A session's expiry: 7 days after it was last seen, never past 30 days after its sign-in.
pub fn session_expiry(authenticated_at: i64, last_seen_at: i64) -> i64 {
    (last_seen_at + 7 * DAY_MS).min(authenticated_at + 30 * DAY_MS)
}

/// A sensitive action needs a sign-in within the last 10 minutes.
pub fn recently_signed_in(authenticated_at: i64, now: i64) -> bool {
    now - authenticated_at <= 10 * 60_000
}

/// The browser family of a `User-Agent`, for the sessions list; never the header itself.
pub fn browser_family(user_agent: &str) -> &'static str {
    let ua = user_agent.to_ascii_lowercase();
    if ua.contains("edg/") {
        "Edge"
    } else if ua.contains("firefox/") {
        "Firefox"
    } else if ua.contains("chrome/") || ua.contains("chromium/") {
        "Chrome"
    } else if ua.contains("safari/") {
        "Safari"
    } else {
        "Other"
    }
}

/// A `next` target after sign-in or re-authentication: a path under `/console/`, nothing else.
pub fn safe_next(next: &str) -> Option<&str> {
    let ok = (next == "/console" || next.starts_with("/console/"))
        && !next.contains("//")
        && !next.contains('\\')
        && next
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'<' && b != b'>');
    ok.then_some(next)
}

/// An address shown in a `member.invited` event: the first character of the local part, then `***`.
pub fn email_hint(email: &str) -> String {
    match email.split_once('@') {
        Some((local, domain)) => {
            let first: String = local.chars().take(1).collect();
            format!("{first}***@{domain}")
        }
        None => "***".into(),
    }
}

/// The client network a sign-in request is counted for: an IPv4 address, or an IPv6 address's /64.
pub fn client_network(ip: &str) -> String {
    match ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(v6)) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
        Ok(std::net::IpAddr::V4(v4)) => v4.to_string(),
        Err(_) => ip.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Console › Roles: each set holds the previous one; only the owner holds `tenants:erase`; a member
    /// never manages members, keys or the policy.
    #[test]
    fn role_permissions() {
        let v = Role::Viewer.permissions();
        let m = Role::Member.permissions();
        let a = Role::Admin.permissions();
        let o = Role::Owner.permissions();
        assert!(v.iter().all(|p| m.contains(p)));
        assert!(m.iter().all(|p| a.contains(p)));
        assert!(a.iter().all(|p| o.contains(p)));
        assert!(o.contains(&"tenants:erase") && !a.contains(&"tenants:erase"));
        assert!(a.contains(&"identities:sign") && !m.contains(&"identities:sign"));
        for p in [
            "members:manage",
            "keys:manage",
            "policy:write",
            "messages:send",
        ] {
            assert!(!m.contains(&p), "{p}");
        }
        assert!(!v.contains(&"search:agentic") && m.contains(&"search:agentic"));
        assert_eq!(Role::parse("admin"), Some(Role::Admin));
        assert_eq!(Role::parse("root"), None);
    }

    #[test]
    fn tokens_codes_and_csrf() {
        let t = token("a", &[7; 32]);
        assert_eq!((t.len(), token_kid(&t)), (44, Some("a")));
        assert_eq!(token_kid("short"), None);
        assert_eq!(code_from(&[u32::MAX, 1_234_567]).as_deref(), Some("234567"));
        assert_eq!(code_from(&[u32::MAX]), None);
        assert_eq!(code_from(&[5]).as_deref(), Some("000005"));
        let k = [1u8; 32];
        assert_ne!(
            code_hash(&k, "a@x.example", "123456"),
            code_hash(&k, "b@x.example", "123456")
        );
        assert_eq!(keyed_hash(&k, "v").len(), 64);
        assert_eq!(csrf_token("s"), csrf_token("s"));
        assert_ne!(csrf_token("s"), csrf_token("t"));
        assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "ab"));
    }

    /// Sessions: 7 days rolling, 30 days absolute; re-authentication within 10 minutes.
    #[test]
    fn session_rules() {
        let day = DAY_MS;
        assert_eq!(session_expiry(0, 0), 7 * day);
        assert_eq!(session_expiry(0, 28 * day), 30 * day);
        assert!(recently_signed_in(0, 600_000) && !recently_signed_in(0, 600_001));
        assert_eq!(
            browser_family("Mozilla/5.0 (X11) Chrome/130.0 Safari/537"),
            "Chrome"
        );
        assert_eq!(browser_family("Mozilla/5.0 Firefox/131.0"), "Firefox");
    }

    #[test]
    fn next_hints_and_networks() {
        assert_eq!(safe_next("/console/keys"), Some("/console/keys"));
        for bad in [
            "https://evil.example/",
            "//evil.example",
            "/v1/me",
            "/console/\\x",
            "/console//x",
        ] {
            assert_eq!(safe_next(bad), None, "{bad}");
        }
        assert_eq!(
            email_hint("ola@brightwell.example"),
            "o***@brightwell.example"
        );
        assert_eq!(client_network("2001:db8:1:2:3:4:5:6"), "2001:db8:1:2::/64");
        assert_eq!(client_network("2001:db8:1:2:ffff::1"), "2001:db8:1:2::/64");
        assert_eq!(client_network("192.0.2.7"), "192.0.2.7");
    }
}
