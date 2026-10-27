//! AWS request signing, Signature Version 4 ([Outbound › Amazon SES]). Pure: the caller supplies the
//! request, the credentials and the time. SES, S3 and SQS calls all sign with [`sigv4`].
//!
//! [Outbound › Amazon SES]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/outbound.md

use hmac::{KeyInit as _, Mac as _};
use sha2::{Digest as _, Sha256};

type HmacSha256 = hmac::Hmac<Sha256>;

/// AWS credentials. Temporary credentials (STS, OIDC) carry a session token, which is then signed as
/// `x-amz-security-token`.
pub struct Credentials<'a> {
    pub key_id: &'a str,
    pub secret: &'a str,
    pub session_token: Option<&'a str>,
}

/// The request to sign. `path` and `query` are already in canonical form (URI-encoded; the query's
/// parameters sorted); `headers` are the headers to sign besides `host`, `x-amz-date` and the session
/// token, for example `content-type` or `x-amz-content-sha256`.
pub struct Request<'a> {
    pub method: &'a str,
    pub host: &'a str,
    pub path: &'a str,
    pub query: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
    /// Lower-case hex SHA-256 of the body.
    pub payload_sha256: &'a str,
}

/// The headers to add to the request.
#[derive(Debug, PartialEq, Eq)]
pub struct Signed {
    pub authorization: String,
    pub x_amz_date: String,
    pub security_token: Option<String>,
}

/// Lower-case hex SHA-256, for [`Request::payload_sha256`] and `x-amz-content-sha256`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Signs `req` for `service` in `region` at `unix_seconds`.
pub fn sigv4(
    req: &Request<'_>,
    creds: &Credentials<'_>,
    region: &str,
    service: &str,
    unix_seconds: i64,
) -> Signed {
    let amz_date = amz_date(unix_seconds);
    let day = &amz_date[..8];

    let mut headers: Vec<(String, String)> = vec![
        ("host".to_owned(), req.host.to_owned()),
        ("x-amz-date".to_owned(), amz_date.clone()),
    ];
    if let Some(token) = creds.session_token {
        headers.push(("x-amz-security-token".to_owned(), token.to_owned()));
    }
    for (name, value) in req.headers {
        headers.push((name.to_ascii_lowercase(), trim_value(value)));
    }
    headers.sort();
    let canonical_headers: String = headers.iter().map(|(n, v)| format!("{n}:{v}\n")).collect();
    let signed_headers = headers
        .iter()
        .map(|(n, _)| n.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        req.method, req.path, req.query, canonical_headers, signed_headers, req.payload_sha256
    );
    let scope = format!("{day}/{region}/{service}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    let k_date = hmac(format!("AWS4{}", creds.secret).as_bytes(), day.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    let k_signing = hmac(&k_service, b"aws4_request");
    let signature = hex(&hmac(&k_signing, string_to_sign.as_bytes()));

    Signed {
        authorization: format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            creds.key_id
        ),
        x_amz_date: amz_date,
        security_token: creds.session_token.map(str::to_owned),
    }
}

/// `YYYYMMDD'T'HHMMSS'Z'` for a Unix time in seconds (UTC).
pub fn amz_date(unix_seconds: i64) -> String {
    let days = unix_seconds.div_euclid(86_400);
    let secs = unix_seconds.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Header values are trimmed and inner runs of spaces collapse to one (SigV4 canonical form).
fn trim_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: Credentials<'static> = Credentials {
        key_id: "AKIDEXAMPLE",
        secret: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
        session_token: None,
    };
    /// 2015-08-30T12:36:00Z, the time of the AWS test suite.
    const SUITE_TIME: i64 = 1_440_938_160;

    fn get_vanilla<'a>(empty: &'a str) -> Request<'a> {
        Request {
            method: "GET",
            host: "example.amazonaws.com",
            path: "/",
            query: "",
            headers: &[],
            payload_sha256: empty,
        }
    }

    /// The AWS SigV4 test suite as published in awslabs/aws-c-auth
    /// (`tests/aws-signing-test-suite/v4`, read 2026-10-27): `get-vanilla`,
    /// `get-vanilla-with-session-token` and `post-x-www-form-urlencoded`.
    #[test]
    fn sigv4_vectors() {
        let empty = sha256_hex(b"");
        assert_eq!(amz_date(SUITE_TIME), "20150830T123600Z");

        let signed = sigv4(
            &get_vanilla(&empty),
            &KEY,
            "us-east-1",
            "service",
            SUITE_TIME,
        );
        assert_eq!(
            signed.authorization,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );

        let with_token = Credentials {
            session_token: Some("6e86291e8372ff2a2260956d9b8aae1d763fbf315fa00fa31553b73ebf194267"),
            ..KEY
        };
        let signed = sigv4(
            &get_vanilla(&empty),
            &with_token,
            "us-east-1",
            "service",
            SUITE_TIME,
        );
        assert!(signed.authorization.ends_with(
            "SignedHeaders=host;x-amz-date;x-amz-security-token, \
             Signature=07ec1639c89043aa0e3e2de82b96708f198cceab042d4a97044c66dd9f74e7f8"
        ));

        let body_hash = sha256_hex(b"Param1=value1");
        assert_eq!(
            body_hash,
            "9095672bbd1f56dfc5b65f3e153adc8731a4a654192329106275f4c7b24d0b6e"
        );
        let post = Request {
            method: "POST",
            host: "example.amazonaws.com",
            path: "/",
            query: "",
            headers: &[
                ("Content-Type", "application/x-www-form-urlencoded"),
                ("Content-Length", "13"),
                ("x-amz-content-sha256", &body_hash),
            ],
            payload_sha256: &body_hash,
        };
        let signed = sigv4(&post, &KEY, "us-east-1", "service", SUITE_TIME);
        assert!(signed.authorization.ends_with(
            "SignedHeaders=content-length;content-type;host;x-amz-content-sha256;x-amz-date, \
             Signature=d3875051da38690788ef43de4db0d8f280229d82040bfac253562e56c3f20e0b"
        ));
    }

    #[test]
    fn amz_date_handles_leap_years_and_the_epoch() {
        assert_eq!(amz_date(0), "19700101T000000Z");
        assert_eq!(amz_date(951_782_400), "20000229T000000Z");
        assert_eq!(amz_date(1_793_131_200), "20261027T200000Z");
    }
}
