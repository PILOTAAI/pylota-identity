//! DNS over HTTPS against the two configured resolvers (`PM_DOH_RESOLVERS`): `GET
//! {resolver}?name={name}&type={TYPE}` with `accept: application/dns-json`, 3 seconds, no retries here.
//! Cloudflare's `cloudflare-dns.com/dns-query` and Google's `dns.google/resolve` both answer the JSON
//! format with `Status`, `AD` and `Answer[{name, type, TTL, data}]`; they differ in the details
//! [`parse_json`] evens out (read from both on 2026-10-28; the answers are in `canned/`):
//!
//! - Cloudflare writes TXT data as the record's quoted character-strings (`"\"v=DKIM1; …\" \"…\""`);
//!   Google writes them unquoted and concatenated. Both come out concatenated and unquoted.
//! - Google ends names with a dot; Cloudflare does not, except in the data of NS, CNAME and MX
//!   answers. Names and targets come out lower case without the final dot.
//! - Records of other types in `Answer` (`RRSIG`, `DNAME`, …) are skipped; a CNAME in front of the
//!   asked type is kept, as `Answer` lists it.

use serde::Deserialize;

use crate::http::{HttpClient, HttpError, HttpRequest, Redirect};

/// The resolvers, in `PM_DOH_RESOLVERS` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resolver {
    First,
    Second,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RType {
    A,
    Aaaa,
    Cname,
    Mx,
    Ns,
    Txt,
}

impl RType {
    pub const fn as_str(self) -> &'static str {
        match self {
            RType::A => "A",
            RType::Aaaa => "AAAA",
            RType::Cname => "CNAME",
            RType::Mx => "MX",
            RType::Ns => "NS",
            RType::Txt => "TXT",
        }
    }

    /// The RR type number of the JSON format.
    pub const fn code(self) -> u16 {
        match self {
            RType::A => 1,
            RType::Ns => 2,
            RType::Cname => 5,
            RType::Mx => 15,
            RType::Txt => 16,
            RType::Aaaa => 28,
        }
    }

    pub fn from_code(code: u16) -> Option<Self> {
        Some(match code {
            1 => RType::A,
            2 => RType::Ns,
            5 => RType::Cname,
            15 => RType::Mx,
            16 => RType::Txt,
            28 => RType::Aaaa,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DnsRecord {
    pub name: String,
    pub rtype: RType,
    pub ttl: u32,
    pub data: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnsStatus {
    NoError,
    NxDomain,
    ServFail,
    Refused,
    Other(u16),
}

impl DnsStatus {
    pub fn from_code(code: u16) -> Self {
        match code {
            0 => DnsStatus::NoError,
            2 => DnsStatus::ServFail,
            3 => DnsStatus::NxDomain,
            5 => DnsStatus::Refused,
            n => DnsStatus::Other(n),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DnsAnswer {
    pub status: DnsStatus,
    /// The resolver validated the answer with DNSSEC.
    pub ad: bool,
    pub records: Vec<DnsRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DnsError {
    Timeout,
    /// The resolver answered with this HTTP status; `0` when no response came (DNS, TLS or connect
    /// failure on the way to the resolver).
    Http(u16),
    Malformed,
}

pub trait Dns {
    async fn query(&self, r: Resolver, name: &str, rtype: RType) -> Result<DnsAnswer, DnsError>;
}

pub const TIMEOUT_MS: u32 = 3_000;
/// More than any answer the designs read (a TXT set of a few KB); a longer body is `Malformed`.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
struct Json {
    #[serde(rename = "Status")]
    status: u16,
    #[serde(rename = "AD", default)]
    ad: bool,
    #[serde(rename = "Answer", default)]
    answer: Vec<JsonRecord>,
}

#[derive(Deserialize)]
struct JsonRecord {
    name: String,
    #[serde(rename = "type")]
    rtype: u16,
    #[serde(rename = "TTL", default)]
    ttl: u32,
    data: String,
}

/// A DoH JSON answer from either resolver.
pub fn parse_json(body: &[u8]) -> Result<DnsAnswer, DnsError> {
    let json: Json = serde_json::from_slice(body).map_err(|_| DnsError::Malformed)?;
    let records = json
        .answer
        .into_iter()
        .filter_map(|r| {
            let rtype = RType::from_code(r.rtype)?;
            let data = match rtype {
                RType::Txt => unquote_txt(&r.data),
                RType::Cname | RType::Ns => name(&r.data),
                RType::Mx => match r.data.split_once(' ') {
                    Some((pref, host)) => format!("{pref} {}", name(host)),
                    None => r.data,
                },
                RType::A | RType::Aaaa => r.data,
            };
            Some(DnsRecord {
                name: name(&r.name),
                rtype,
                ttl: r.ttl,
                data,
            })
        })
        .collect();
    Ok(DnsAnswer {
        status: DnsStatus::from_code(json.status),
        ad: json.ad,
        records,
    })
}

fn name(n: &str) -> String {
    n.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// The concatenated character-strings of TXT data written in zone-file form (`"a" "b"`, with `\"`,
/// `\\` and `\DDD` escapes). Data not in that form, as Google writes it, is returned as is.
pub fn unquote_txt(data: &str) -> String {
    fn quoted(data: &str) -> Option<String> {
        let bytes = data.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        let mut strings = 0;
        while i < bytes.len() {
            match bytes[i] {
                b' ' | b'\t' => i += 1,
                b'"' => {
                    i += 1;
                    loop {
                        match *bytes.get(i)? {
                            b'"' => break,
                            b'\\' => {
                                let next = *bytes.get(i + 1)?;
                                if next.is_ascii_digit() {
                                    let digits = data.get(i + 1..i + 4)?;
                                    out.push(digits.parse::<u8>().ok()?);
                                    i += 4;
                                } else {
                                    out.push(next);
                                    i += 2;
                                }
                            }
                            b => {
                                out.push(b);
                                i += 1;
                            }
                        }
                    }
                    i += 1;
                    strings += 1;
                }
                _ => return None,
            }
        }
        (strings > 0).then(|| String::from_utf8_lossy(&out).into_owned())
    }
    if data.starts_with('"') {
        quoted(data).unwrap_or_else(|| data.to_owned())
    } else {
        data.to_owned()
    }
}

/// [`Dns`] over any [`HttpClient`]: the Cloudflare implementation, and natively with a fake client.
pub struct DohClient<H> {
    pub http: H,
    /// `PM_DOH_RESOLVERS`, in order.
    pub resolvers: [String; 2],
}

impl<H: HttpClient> DohClient<H> {
    pub fn url(&self, r: Resolver, name: &str, rtype: RType) -> String {
        let base = match r {
            Resolver::First => &self.resolvers[0],
            Resolver::Second => &self.resolvers[1],
        };
        let sep = if base.contains('?') { '&' } else { '?' };
        format!("{base}{sep}name={}&type={}", encode(name), rtype.as_str())
    }
}

/// Query-string encoding of a DNS name: letters, digits, `-`, `.` and `_` pass, anything else is
/// percent-encoded.
fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

impl<H: HttpClient> Dns for DohClient<H> {
    async fn query(&self, r: Resolver, name: &str, rtype: RType) -> Result<DnsAnswer, DnsError> {
        let req = HttpRequest {
            method: "GET",
            url: self.url(r, name, rtype),
            headers: vec![("accept".to_owned(), "application/dns-json".to_owned())],
            body: None,
            timeout_ms: TIMEOUT_MS,
            redirect: Redirect::Manual,
            max_body_bytes: MAX_BODY_BYTES,
        };
        let resp = self.http.send(req).await.map_err(|e| match e {
            HttpError::Timeout => DnsError::Timeout,
            _ => DnsError::Http(0),
        })?;
        if resp.status != 200 {
            return Err(DnsError::Http(resp.status));
        }
        if resp.body_truncated {
            return Err(DnsError::Malformed);
        }
        parse_json(&resp.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakes::{FakeHttp, ScriptedResponse, block_on};

    fn canned(resolver: &str, case: &str) -> &'static str {
        macro_rules! cases {
            ($($r:literal $c:literal),* $(,)?) => {
                match (resolver, case) {
                    $(($r, $c) => include_str!(concat!("canned/", $r, "-", $c, ".json")),)*
                    _ => panic!("no canned answer {resolver}-{case}"),
                }
            };
        }
        cases!(
            "cloudflare" "dkim", "google" "dkim", "cloudflare" "mx", "google" "mx",
            "cloudflare" "ns", "google" "ns", "cloudflare" "cname", "google" "cname",
            "cloudflare" "chain", "google" "chain", "cloudflare" "nxdomain", "google" "nxdomain",
            "cloudflare" "servfail", "google" "servfail", "cloudflare" "ad", "google" "ad",
        )
    }

    /// TXT, MX, NS and CNAME, NXDOMAIN and SERVFAIL from both resolvers (build plan M4): each pair of
    /// real answers, read from Cloudflare and Google on 2026-10-28, parses to the same records but
    /// for the TTLs, which each resolver counts down on its own.
    #[test]
    fn parse_canned_answers() {
        let both = |case: &str| {
            let a = parse_json(canned("cloudflare", case).as_bytes()).unwrap();
            let b = parse_json(canned("google", case).as_bytes()).unwrap();
            let strip = |x: &DnsAnswer| {
                let mut x = x.clone();
                x.records.iter_mut().for_each(|r| r.ttl = 0);
                x.records.sort_by(|p, q| p.data.cmp(&q.data));
                x
            };
            assert_eq!(strip(&a), strip(&b), "{case}");
            a
        };

        // TXT: Cloudflare's two quoted character-strings and Google's concatenation agree.
        let dkim = both("dkim");
        assert_eq!(dkim.status, DnsStatus::NoError);
        let [txt] = &dkim.records[..] else {
            panic!("{dkim:?}")
        };
        assert_eq!(
            (txt.name.as_str(), txt.rtype),
            ("google._domainkey.github.com", RType::Txt)
        );
        assert!(txt.data.starts_with("v=DKIM1; k=rsa; p=MIIBIjANBg"));
        assert!(txt.data.ends_with("IDAQAB") && !txt.data.contains('"'));
        assert!(
            txt.data.contains("RNAPKu/OPoA7dlR"),
            "the two strings joined without a space"
        );

        let mx = both("mx");
        assert_eq!(mx.records[0].rtype, RType::Mx);
        assert_eq!(
            mx.records[0].data,
            "0 github-com.mail.protection.outlook.com"
        );

        let ns = both("ns");
        let hosts: Vec<&str> = ns.records.iter().map(|r| r.data.as_str()).collect();
        assert_eq!(
            hosts,
            ["cortney.ns.cloudflare.com", "everton.ns.cloudflare.com"]
        );
        assert!(
            ns.records
                .iter()
                .all(|r| r.name == "pylotamail.com" && r.rtype == RType::Ns)
        );

        let cname = both("cname");
        assert_eq!(
            (
                cname.records[0].name.as_str(),
                cname.records[0].data.as_str()
            ),
            ("www.github.com", "github.com")
        );

        // A TXT query for a CNAME: the CNAME comes first, then the target's TXT records (the canned
        // files keep the first two of each resolver's, which order the set differently).
        for resolver in ["cloudflare", "google"] {
            let chain = parse_json(canned(resolver, "chain").as_bytes()).unwrap();
            assert_eq!(
                (chain.records[0].rtype, chain.records[0].data.as_str()),
                (RType::Cname, "github.com")
            );
            assert!(
                chain.records[1..]
                    .iter()
                    .all(|r| r.rtype == RType::Txt && r.name == "github.com")
            );
            assert!(chain.records.iter().all(|r| !r.data.starts_with('"')));
        }

        let nx = both("nxdomain");
        assert_eq!((nx.status, nx.records.len()), (DnsStatus::NxDomain, 0));
        let fail = both("servfail");
        assert_eq!((fail.status, fail.records.len()), (DnsStatus::ServFail, 0));
        let ad = both("ad");
        assert!(ad.ad && ad.records.iter().all(|r| r.rtype == RType::A));

        assert_eq!(parse_json(b"<html>"), Err(DnsError::Malformed));
        assert_eq!(parse_json(b"{}"), Err(DnsError::Malformed));
    }

    #[test]
    fn txt_unquoting() {
        assert_eq!(unquote_txt(r#""a" "b""#), "ab");
        assert_eq!(unquote_txt(r#""say \"hi\"\\ \059""#), "say \"hi\"\\ ;");
        assert_eq!(unquote_txt("v=spf1 -all"), "v=spf1 -all");
        assert_eq!(unquote_txt(r#""unterminated"#), r#""unterminated"#);
    }

    /// The client sends the JSON request to the resolver it is asked for and maps failures.
    #[test]
    fn client_requests_and_errors() {
        let http = FakeHttp::new();
        let doh = DohClient {
            http,
            resolvers: [
                "https://cloudflare-dns.com/dns-query".to_owned(),
                "https://dns.google/resolve".to_owned(),
            ],
        };
        let url = doh.url(Resolver::Second, "google._domainkey.github.com", RType::Txt);
        assert_eq!(
            url,
            "https://dns.google/resolve?name=google._domainkey.github.com&type=TXT"
        );
        doh.http.script(
            &url,
            ScriptedResponse::ok(canned("google", "dkim").as_bytes().to_vec()),
        );
        let a = block_on(doh.query(Resolver::Second, "google._domainkey.github.com", RType::Txt))
            .unwrap();
        assert_eq!(a.records.len(), 1);
        let sent = doh.http.requests();
        assert_eq!(sent[0].timeout_ms, TIMEOUT_MS);
        assert!(
            sent[0]
                .headers
                .contains(&("accept".into(), "application/dns-json".into()))
        );

        let url = doh.url(Resolver::First, "x y.example", RType::A);
        assert!(url.ends_with("name=x%20y.example&type=A"));
        doh.http.script(&url, ScriptedResponse::status(502));
        assert_eq!(
            block_on(doh.query(Resolver::First, "x y.example", RType::A)),
            Err(DnsError::Http(502))
        );
        doh.http.script(
            &url,
            ScriptedResponse::ok(Vec::new()).after_ms(TIMEOUT_MS + 1),
        );
        assert_eq!(
            block_on(doh.query(Resolver::First, "x y.example", RType::A)),
            Err(DnsError::Timeout)
        );
        // No script for the name: the fake answers as an unreachable host.
        assert_eq!(
            block_on(doh.query(Resolver::First, "none.example", RType::A)),
            Err(DnsError::Http(0))
        );
    }
}
