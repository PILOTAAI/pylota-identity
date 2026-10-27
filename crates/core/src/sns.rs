//! Amazon SNS message verification ([Outbound › Events], [Inbound › SES arrival]). Pure: the caller fetches
//! the signing certificate from the checked `SigningCertURL` and passes its PEM, and passes the time.
//!
//! Only `SignatureVersion` 2 (RSA with SHA-256) is accepted; version 1 (SHA-1) is refused.
//!
//! [Outbound › Events]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md
//! [Inbound › SES arrival]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use rsa::pkcs1::DecodeRsaPublicKey as _;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use x509_cert::der::Decode as _;

/// The fields of an SNS HTTP or SQS envelope that verification reads.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct Envelope {
    #[serde(rename = "Type")]
    pub kind: String,
    pub message_id: String,
    pub topic_arn: String,
    pub message: String,
    pub timestamp: String,
    pub signature_version: String,
    pub signature: String,
    #[serde(rename = "SigningCertURL")]
    pub signing_cert_url: String,
    pub subject: Option<String>,
    #[serde(rename = "SubscribeURL")]
    pub subscribe_url: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Malformed(String),
    /// `SignatureVersion` is not `2`.
    Version(String),
    /// `SigningCertURL` is not `https://sns.{region}.amazonaws.com/….pem`.
    CertificateUrl,
    WrongTopic,
    /// `Timestamp` is unreadable or outside the allowed window.
    Stale,
    Certificate(String),
    BadSignature,
}

/// What a valid message must match.
pub struct Expect<'a> {
    pub region: &'a str,
    pub topic_arn: &'a str,
    /// Allowed distance between `Timestamp` and now: one hour on push, 14 days on the SQS backstop.
    pub max_age_seconds: i64,
}

pub fn parse_envelope(json: &str) -> Result<Envelope, Error> {
    serde_json::from_str(json).map_err(|e| Error::Malformed(e.to_string()))
}

/// The checks that need no certificate, in the order the handler runs them: version, certificate URL,
/// topic, time. Returns the URL to fetch the certificate from.
pub fn precheck<'e>(env: &'e Envelope, expect: &Expect<'_>, now: i64) -> Result<&'e str, Error> {
    if env.signature_version != "2" {
        return Err(Error::Version(env.signature_version.clone()));
    }
    let host = format!("https://sns.{}.amazonaws.com/", expect.region);
    let url = env.signing_cert_url.as_str();
    if !url.starts_with(&host)
        || !url.ends_with(".pem")
        || url[host.len()..].contains(['?', '#', '@'])
    {
        return Err(Error::CertificateUrl);
    }
    if env.topic_arn != expect.topic_arn {
        return Err(Error::WrongTopic);
    }
    let at = parse_rfc3339(&env.timestamp).ok_or(Error::Stale)?;
    if (now - at).abs() > expect.max_age_seconds {
        return Err(Error::Stale);
    }
    Ok(url)
}

/// The string SNS signs: each field as `Name\nValue\n`, in byte order of the names (AWS, "Verifying the
/// signatures of Amazon SNS messages", read 2026-10-27).
pub fn string_to_sign(env: &Envelope) -> Result<String, Error> {
    let mut fields: Vec<(&str, &str)> = vec![
        ("Message", &env.message),
        ("MessageId", &env.message_id),
        ("Timestamp", &env.timestamp),
        ("TopicArn", &env.topic_arn),
        ("Type", &env.kind),
    ];
    match env.kind.as_str() {
        "Notification" => {
            if let Some(subject) = &env.subject {
                fields.push(("Subject", subject));
            }
        }
        "SubscriptionConfirmation" | "UnsubscribeConfirmation" => {
            let missing = || Error::Malformed("SubscribeURL and Token are required".to_owned());
            fields.push((
                "SubscribeURL",
                env.subscribe_url.as_deref().ok_or_else(missing)?,
            ));
            fields.push(("Token", env.token.as_deref().ok_or_else(missing)?));
        }
        other => return Err(Error::Malformed(format!("unknown Type {other}"))),
    }
    fields.sort_by(|a, b| a.0.cmp(b.0));
    Ok(fields.iter().map(|(n, v)| format!("{n}\n{v}\n")).collect())
}

/// Verifies `env` against the PEM certificate fetched from its `SigningCertURL`, after [`precheck`].
pub fn verify(env: &Envelope, expect: &Expect<'_>, now: i64, cert_pem: &str) -> Result<(), Error> {
    precheck(env, expect, now)?;
    let key = public_key(cert_pem)?;
    let signature = STANDARD
        .decode(env.signature.trim())
        .map_err(|e| Error::Malformed(format!("Signature: {e}")))?;
    let digest = Sha256::digest(string_to_sign(env)?.as_bytes());
    // PKCS #1 v1.5 with the SHA-256 DigestInfo prefix, built by hand so that no second `sha2` (rsa 0.9
    // speaks digest 0.10) enters the graph (Rust workspace §3).
    let scheme = rsa::Pkcs1v15Sign {
        hash_len: Some(32),
        prefix: SHA256_DIGEST_INFO.to_vec().into_boxed_slice(),
    };
    key.verify(scheme, &digest, &signature)
        .map_err(|_| Error::BadSignature)
}

/// DER prefix of a SHA-256 `DigestInfo` (RFC 8017 §9.2, note 1).
const SHA256_DIGEST_INFO: [u8; 19] = [
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

fn public_key(pem: &str) -> Result<rsa::RsaPublicKey, Error> {
    let body: String = pem
        .lines()
        .skip_while(|l| !l.starts_with("-----BEGIN CERTIFICATE-----"))
        .skip(1)
        .take_while(|l| !l.starts_with("-----END CERTIFICATE-----"))
        .collect();
    let der = STANDARD
        .decode(body.trim())
        .map_err(|e| Error::Certificate(format!("PEM: {e}")))?;
    let cert = x509_cert::Certificate::from_der(&der)
        .map_err(|e| Error::Certificate(format!("X.509: {e}")))?;
    let spki = cert.tbs_certificate.subject_public_key_info;
    let bits = spki
        .subject_public_key
        .as_bytes()
        .ok_or_else(|| Error::Certificate("unaligned key".to_owned()))?;
    rsa::RsaPublicKey::from_pkcs1_der(bits).map_err(|e| Error::Certificate(format!("RSA key: {e}")))
}

/// `YYYY-MM-DDTHH:MM:SS(.fff)Z` to Unix seconds; `None` for anything else.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    if !s.ends_with('Z') {
        return None;
    }
    let n = |r: core::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60 + ss)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOPIC: &str = "arn:aws:sns:eu-west-2:123456789012:pi-spike-11-inbound";

    fn envelope() -> Envelope {
        Envelope {
            kind: "Notification".into(),
            message_id: "m-1".into(),
            topic_arn: TOPIC.into(),
            message: "{\"notificationType\":\"Received\"}".into(),
            timestamp: "2026-10-27T22:00:00.000Z".into(),
            signature_version: "2".into(),
            signature: "AAAA".into(),
            signing_cert_url:
                "https://sns.eu-west-2.amazonaws.com/SimpleNotificationService-abc.pem".into(),
            subject: None,
            subscribe_url: None,
            token: None,
        }
    }

    fn expect() -> Expect<'static> {
        Expect {
            region: "eu-west-2",
            topic_arn: TOPIC,
            max_age_seconds: 3600,
        }
    }

    const NOW: i64 = 1_793_138_400; // 2026-10-27T22:00:00Z

    #[test]
    fn string_to_sign_is_sorted_with_a_trailing_newline() {
        let mut env = envelope();
        env.subject = Some("Amazon SES Email Receipt Notification".into());
        assert_eq!(
            string_to_sign(&env).unwrap(),
            "Message\n{\"notificationType\":\"Received\"}\nMessageId\nm-1\nSubject\nAmazon SES Email \
             Receipt Notification\nTimestamp\n2026-10-27T22:00:00.000Z\nTopicArn\n"
                .to_owned()
                + TOPIC
                + "\nType\nNotification\n"
        );
    }

    #[test]
    fn precheck_refuses_version_host_topic_and_time() {
        assert!(precheck(&envelope(), &expect(), NOW).is_ok());
        let mut v1 = envelope();
        v1.signature_version = "1".into();
        assert_eq!(
            precheck(&v1, &expect(), NOW),
            Err(Error::Version("1".into()))
        );
        for url in [
            "http://sns.eu-west-2.amazonaws.com/x.pem",
            "https://sns.us-east-1.amazonaws.com/x.pem",
            "https://sns.eu-west-2.amazonaws.com.evil.example/x.pem",
            "https://sns.eu-west-2.amazonaws.com/x.pem?y=.pem",
        ] {
            let mut e = envelope();
            e.signing_cert_url = url.into();
            assert_eq!(
                precheck(&e, &expect(), NOW),
                Err(Error::CertificateUrl),
                "{url}"
            );
        }
        let mut other = envelope();
        other.topic_arn = "arn:aws:sns:eu-west-2:123456789012:other".into();
        assert_eq!(precheck(&other, &expect(), NOW), Err(Error::WrongTopic));
        assert_eq!(
            precheck(&envelope(), &expect(), NOW + 3601),
            Err(Error::Stale)
        );
    }

    #[test]
    fn rfc3339() {
        assert_eq!(parse_rfc3339("2026-10-27T22:00:00.000Z"), Some(NOW));
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2026-10-27 22:00:00Z"), None);
        assert_eq!(parse_rfc3339("2026-13-27T22:00:00Z"), None);
    }
}
