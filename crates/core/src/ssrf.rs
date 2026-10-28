//! The SSRF guard's decisions ([Security § 9]): the URL rules checked at create, update and before every
//! attempt, and the blocked address ranges checked on every resolved address. Pure: the worker's
//! `net::GuardedHttp` resolves and sends.
//!
//! [Security § 9]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/security.md

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// At most this many bytes of URL.
pub const MAX_URL_BYTES: usize = 2048;

/// Why a URL or an address is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotHttps,
    TooLong,
    UserInfo,
    Fragment,
    IpLiteral,
    BadHost,
    RefusedName,
    Port,
    NoAddress,
    BlockedAddress,
}

impl Refusal {
    pub const fn message(self) -> &'static str {
        match self {
            Self::NotHttps => "The URL must use https.",
            Self::TooLong => "The URL is longer than 2,048 bytes.",
            Self::UserInfo => "The URL may not carry a user name or password.",
            Self::Fragment => "The URL may not carry a fragment.",
            Self::IpLiteral => "The host must be a DNS name, not an IP address.",
            Self::BadHost => "The host must be a DNS name of at least two labels.",
            Self::RefusedName => "This host name is not allowed.",
            Self::Port => "The port must be 443 or 1024 to 65535.",
            Self::NoAddress => "The host name has no address.",
            Self::BlockedAddress => "The host name resolves to a private or reserved address.",
        }
    }
}

/// A URL that passed rules 1–4: its lower-case host and port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
}

/// Names refused whatever the deployment (rule 3): `localhost` and these suffixes.
const REFUSED_SUFFIXES: &[&str] = &[
    "localhost",
    "local",
    "internal",
    "invalid",
    "test",
    "example",
    "onion",
    "home.arpa",
    "arpa",
];

fn under(host: &str, name: &str) -> bool {
    host == name || host.ends_with(&format!(".{name}"))
}

/// A host written as an IPv4 address in any notation the WHATWG URL parser accepts: dotted decimal,
/// integer, hexadecimal or octal parts.
fn ipv4_like(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    let last = parts.last().copied().unwrap_or("");
    let numeric = |p: &str| {
        !p.is_empty()
            && (p.chars().all(|c| c.is_ascii_digit())
                || (p.len() > 2
                    && (p.starts_with("0x") || p.starts_with("0X"))
                    && p[2..].chars().all(|c| c.is_ascii_hexdigit()))
                || p == "0x"
                || p == "0X")
    };
    // The URL standard treats a host whose last label is numeric as IPv4.
    numeric(last) || parts.iter().all(|p| numeric(p))
}

/// Rules 1–4 of § 9.1. `refused_hosts`: the API and console hosts and the platform domain (a name
/// under the platform domain is refused too).
pub fn check_url(url: &str, refused_hosts: &[&str]) -> Result<Target, Refusal> {
    if url.len() > MAX_URL_BYTES {
        return Err(Refusal::TooLong);
    }
    let Some(rest) = url.strip_prefix("https://").or_else(|| {
        url.get(..8)
            .filter(|p| p.eq_ignore_ascii_case("https://"))
            .map(|_| &url[8..])
    }) else {
        return Err(Refusal::NotHttps);
    };
    if url.contains('#') {
        return Err(Refusal::Fragment);
    }
    let authority = rest.split(['/', '?']).next().unwrap_or("");
    if authority.contains('@') {
        return Err(Refusal::UserInfo);
    }
    if authority.starts_with('[') {
        return Err(Refusal::IpLiteral);
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => {
            let port: u16 = p.parse().map_err(|_| Refusal::Port)?;
            (h, Some(port))
        }
        None => (authority, None),
    };
    let port = match port {
        None | Some(443) => 443,
        Some(p) if p >= 1024 => p,
        Some(_) => return Err(Refusal::Port),
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.parse::<IpAddr>().is_ok() || ipv4_like(&host) {
        return Err(Refusal::IpLiteral);
    }
    if REFUSED_SUFFIXES.iter().any(|s| under(&host, s))
        || refused_hosts
            .iter()
            .map(|h| h.split(':').next().unwrap_or("").to_ascii_lowercase())
            .any(|h| !h.is_empty() && under(&host, &h))
    {
        return Err(Refusal::RefusedName);
    }
    let labels: Vec<&str> = host.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    if labels.len() < 2 || host.len() > 253 || !labels.iter().all(label_ok) {
        return Err(Refusal::BadHost);
    }
    Ok(Target { host, port })
}

fn v4_in(ip: Ipv4Addr, net: [u8; 4], bits: u32) -> bool {
    let mask = if bits == 0 {
        0
    } else {
        u32::MAX << (32 - bits)
    };
    (u32::from(ip) & mask) == (u32::from(Ipv4Addr::from(net)) & mask)
}

fn v6_in(ip: Ipv6Addr, net: [u16; 8], bits: u32) -> bool {
    let mask = if bits == 0 {
        0
    } else {
        u128::MAX << (128 - bits)
    };
    (u128::from(ip) & mask) == (u128::from(Ipv6Addr::from(net)) & mask)
}

const V4_BLOCKED: &[([u8; 4], u32)] = &[
    ([0, 0, 0, 0], 8),
    ([10, 0, 0, 0], 8),
    ([100, 64, 0, 0], 10),
    ([127, 0, 0, 0], 8),
    ([169, 254, 0, 0], 16),
    ([172, 16, 0, 0], 12),
    ([192, 0, 0, 0], 24),
    ([192, 0, 2, 0], 24),
    ([192, 88, 99, 0], 24),
    ([192, 168, 0, 0], 16),
    ([198, 18, 0, 0], 15),
    ([198, 51, 100, 0], 24),
    ([203, 0, 113, 0], 24),
    ([224, 0, 0, 0], 4),
    ([240, 0, 0, 0], 4),
];

const V6_BLOCKED: &[([u16; 8], u32)] = &[
    ([0; 8], 128),
    ([0, 0, 0, 0, 0, 0, 0, 1], 128),
    ([0x100, 0, 0, 0, 0, 0, 0, 0], 64),
    ([0x2001, 0, 0, 0, 0, 0, 0, 0], 23),
    ([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0], 32),
    ([0x3fff, 0, 0, 0, 0, 0, 0, 0], 20),
    ([0x5f00, 0, 0, 0, 0, 0, 0, 0], 16),
    ([0xfc00, 0, 0, 0, 0, 0, 0, 0], 7),
    ([0xfe80, 0, 0, 0, 0, 0, 0, 0], 10),
    ([0xfec0, 0, 0, 0, 0, 0, 0, 0], 10),
    ([0xff00, 0, 0, 0, 0, 0, 0, 0], 8),
];

/// Whether an address is in a blocked range of § 9.1 rule 5.
pub fn blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => V4_BLOCKED.iter().any(|&(n, b)| v4_in(v4, n, b)),
        IpAddr::V6(v6) => {
            // IPv4-mapped (::ffff:0:0/96), NAT64 (64:ff9b::/96, 64:ff9b:1::/48) and 6to4 (2002::/16)
            // carry an IPv4 address; none is a destination a webhook needs, so the whole ranges are
            // refused, which covers every embedded IPv4 the table lists.
            if v6_in(v6, [0, 0, 0, 0, 0, 0xffff, 0, 0], 96)
                || v6_in(v6, [0x64, 0xff9b, 0, 0, 0, 0, 0, 0], 96)
                || v6_in(v6, [0x64, 0xff9b, 1, 0, 0, 0, 0, 0], 48)
                || v6_in(v6, [0x2002, 0, 0, 0, 0, 0, 0, 0], 16)
            {
                return true;
            }
            V6_BLOCKED.iter().any(|&(n, b)| v6_in(v6, n, b))
        }
    }
}

/// Rule 5 over the resolved addresses: none, or any blocked, is refused.
pub fn check_addresses(addresses: &[IpAddr]) -> Result<(), Refusal> {
    if addresses.is_empty() {
        return Err(Refusal::NoAddress);
    }
    if addresses.iter().any(|a| blocked(*a)) {
        return Err(Refusal::BlockedAddress);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    /// Covers FR-WH-5: loopback, RFC 1918, link-local, CGNAT, `::1`, `fc00::/7`, the metadata address,
    /// IPv4-mapped, NAT64 and 6to4 forms.
    #[test]
    fn refuses_private_ranges() {
        for a in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "198.18.0.1",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:8.8.8.8",
            "64:ff9b::a00:1",
            "64:ff9b:1::1",
            "2002:7f00:1::",
            "2001:db8::1",
            "100::1",
        ] {
            assert!(blocked(ip(a)), "{a} must be blocked");
        }
        for a in [
            "8.8.8.8",
            "1.1.1.1",
            "172.32.0.1",
            "100.128.0.1",
            "2606:4700::1111",
            "2a00:1450::1",
        ] {
            assert!(!blocked(ip(a)), "{a} is public");
        }
        assert_eq!(check_addresses(&[]), Err(Refusal::NoAddress));
        assert_eq!(
            check_addresses(&[ip("8.8.8.8"), ip("10.0.0.1")]),
            Err(Refusal::BlockedAddress)
        );
        assert_eq!(check_addresses(&[ip("8.8.8.8")]), Ok(()));
    }

    #[test]
    fn url_rules() {
        let refused = ["api.pylotamail.com", "pylotamail.com"];
        let ok = check_url("https://hooks.example.com/mail?x=1", &refused).unwrap();
        assert_eq!((ok.host.as_str(), ok.port), ("hooks.example.com", 443));
        assert_eq!(
            check_url("https://hooks.example.com:8443/x", &refused)
                .unwrap()
                .port,
            8443
        );
        for (u, r) in [
            ("http://hooks.example.com/", Refusal::NotHttps),
            ("https://u:p@hooks.example.com/", Refusal::UserInfo),
            ("https://hooks.example.com/#f", Refusal::Fragment),
            ("https://127.0.0.1/", Refusal::IpLiteral),
            ("https://[::1]/", Refusal::IpLiteral),
            ("https://2130706433/", Refusal::IpLiteral),
            ("https://0x7f000001/", Refusal::IpLiteral),
            ("https://0177.0.0.1/", Refusal::IpLiteral),
            ("https://localhost/", Refusal::RefusedName),
            ("https://intranet/", Refusal::BadHost),
            ("https://svc.internal/", Refusal::RefusedName),
            ("https://hooks.example/", Refusal::RefusedName),
            ("https://x.pylotamail.com/", Refusal::RefusedName),
            ("https://api.pylotamail.com/", Refusal::RefusedName),
            ("https://hooks.example.com:80/", Refusal::Port),
            ("https://hooks.example.com:22/", Refusal::Port),
        ] {
            assert_eq!(check_url(u, &refused), Err(r), "{u}");
        }
        assert_eq!(
            check_url(
                &format!("https://a.example.com/{}", "x".repeat(2048)),
                &refused
            ),
            Err(Refusal::TooLong)
        );
    }
}
