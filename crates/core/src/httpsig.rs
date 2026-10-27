//! RFC 9421 HTTP message signature bases ([Agent signing keys §5]).
//!
//! Only the base is built here; signing and the clock and nonce belong to the caller, so this module stays
//! free of keys, time and randomness. Web Bot Auth, the key directory and vault consumer signatures all
//! use it.
//!
//! Component values must be ASCII: RFC 9421 and Cloudflare's verifier reject anything else. An IDN host
//! must be converted to its A-label before it reaches [`authority`].
//!
//! [Agent signing keys §5]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/agent-keys.md

use std::fmt::Write as _;

/// A signature parameter, serialised in the order the caller lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Param {
    Created(u64),
    Expires(u64),
    KeyId(String),
    Alg(String),
    Nonce(String),
    Tag(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A component identifier is empty, not lower-case, or not a token.
    BadComponentName(String),
    /// The same component is covered twice.
    DuplicateComponent(String),
    /// A component value holds a non-ASCII byte or a control character.
    NonAsciiValue(String),
    /// A string parameter cannot be a structured-field string.
    BadParam(&'static str),
    /// The host is empty or not ASCII.
    BadHost,
}

/// The `@authority` value (RFC 9421 §2.2.3): the host in lower case, with the port only when it is not
/// the scheme's default.
pub fn authority(host: &str, port: Option<u16>, default_port: u16) -> Result<String, Error> {
    if host.is_empty()
        || !host.is_ascii()
        || host.bytes().any(|b| b.is_ascii_control() || b == b' ')
    {
        return Err(Error::BadHost);
    }
    let host = host.to_ascii_lowercase();
    Ok(match port {
        Some(p) if p != default_port => format!("{host}:{p}"),
        _ => host,
    })
}

/// The inner list and parameters, as they appear after `sig1=` in `Signature-Input` and as the value of
/// `@signature-params`.
pub fn signature_params(components: &[&str], params: &[Param]) -> Result<String, Error> {
    let mut out = String::from("(");
    for (i, name) in components.iter().enumerate() {
        check_name(name)?;
        if components[..i].contains(name) {
            return Err(Error::DuplicateComponent((*name).to_owned()));
        }
        if i > 0 {
            out.push(' ');
        }
        write!(out, "\"{name}\"").expect("writing to a String");
    }
    out.push(')');
    for param in params {
        match param {
            Param::Created(t) => write!(out, ";created={t}"),
            Param::Expires(t) => write!(out, ";expires={t}"),
            Param::KeyId(v) => write!(out, ";keyid={}", sf_string(v, "keyid")?),
            Param::Alg(v) => write!(out, ";alg={}", sf_string(v, "alg")?),
            Param::Nonce(v) => write!(out, ";nonce={}", sf_string(v, "nonce")?),
            Param::Tag(v) => write!(out, ";tag={}", sf_string(v, "tag")?),
        }
        .expect("writing to a String");
    }
    Ok(out)
}

/// The signature base of RFC 9421 §2.5: one line per covered component, in order, then the
/// `@signature-params` line. Header values are trimmed of surrounding spaces and tabs (§2.1).
pub fn signature_base(covered: &[(&str, &str)], params: &[Param]) -> Result<String, Error> {
    let names: Vec<&str> = covered.iter().map(|(n, _)| *n).collect();
    let params_value = signature_params(&names, params)?;
    let mut out = String::new();
    for (name, value) in covered {
        let value = value.trim_matches([' ', '\t']);
        if value.bytes().any(|b| !(0x20..0x7f).contains(&b)) {
            return Err(Error::NonAsciiValue((*name).to_owned()));
        }
        writeln!(out, "\"{name}\": {value}").expect("writing to a String");
    }
    write!(out, "\"@signature-params\": {params_value}").expect("writing to a String");
    Ok(out)
}

/// A structured-field string (RFC 8941 §3.3.3): printable ASCII, with `"` and `\` escaped.
pub fn sf_string(value: &str, param: &'static str) -> Result<String, Error> {
    if value.bytes().any(|b| !(0x20..0x7f).contains(&b)) {
        return Err(Error::BadParam(param));
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    Ok(out)
}

fn check_name(name: &str) -> Result<(), Error> {
    let body = name.strip_prefix('@').unwrap_or(name);
    let ok = !body.is_empty()
        && body
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(Error::BadComponentName(name.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
    use ed25519_dalek::{Signature, Signer as _, SigningKey};

    /// RFC 9421 appendix B.2.6 with test-key-ed25519 of B.1.4 (rfc-editor.org, read 2026-10-27). Ed25519
    /// is deterministic, so the base must sign to exactly the published signature.
    #[test]
    fn signature_base_rfc9421() {
        let covered = [
            ("date", "Tue, 20 Apr 2021 02:07:55 GMT"),
            ("@method", "POST"),
            ("@path", "/foo"),
            ("@authority", "example.com"),
            ("content-type", "application/json"),
            ("content-length", "18"),
        ];
        let params = [
            Param::Created(1_618_884_473),
            Param::KeyId("test-key-ed25519".into()),
        ];
        let base = signature_base(&covered, &params).unwrap();
        assert_eq!(
            base,
            "\"date\": Tue, 20 Apr 2021 02:07:55 GMT\n\
             \"@method\": POST\n\
             \"@path\": /foo\n\
             \"@authority\": example.com\n\
             \"content-type\": application/json\n\
             \"content-length\": 18\n\
             \"@signature-params\": (\"date\" \"@method\" \"@path\" \"@authority\" \"content-type\" \
             \"content-length\");created=1618884473;keyid=\"test-key-ed25519\""
        );

        let seed: [u8; 32] = URL_SAFE_NO_PAD
            .decode("n4Ni-HpISpVObnQMW0wOhCKROaIKqKtW_2ZYb2p9KcU")
            .unwrap()
            .try_into()
            .unwrap();
        let key = SigningKey::from_bytes(&seed);
        let expected: [u8; 64] = STANDARD
            .decode("wqcAqbmYJ2ji2glfAMaRy4gruYYnx2nEFN2HN6jrnDnQCK1u02Gb04v9EDgwUPiu4A0w6vuQv5lIp5WPpBKRCw==")
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(key.sign(base.as_bytes()), Signature::from_bytes(&expected));
        key.verifying_key()
            .verify_strict(base.as_bytes(), &Signature::from_bytes(&expected))
            .unwrap();
    }

    /// The Web Bot Auth shape of [Agent signing keys §5.2]: parameters in the caller's order, strings quoted.
    #[test]
    fn web_bot_auth_params() {
        let value = signature_params(
            &["@authority", "signature-agent", "from"],
            &[
                Param::Created(1_791_547_200),
                Param::Expires(1_791_547_260),
                Param::KeyId("poqkLGiymh_W0uP6PZFw-dvez3QJT5SolqXBCW38r0U".into()),
                Param::Alg("ed25519".into()),
                Param::Nonce("e8N7".into()),
                Param::Tag("web-bot-auth".into()),
            ],
        )
        .unwrap();
        assert_eq!(
            value,
            "(\"@authority\" \"signature-agent\" \"from\");created=1791547200;expires=1791547260;\
             keyid=\"poqkLGiymh_W0uP6PZFw-dvez3QJT5SolqXBCW38r0U\";alg=\"ed25519\";nonce=\"e8N7\";\
             tag=\"web-bot-auth\""
        );
    }

    #[test]
    fn refuses_non_ascii_values_and_hosts() {
        assert_eq!(
            signature_base(&[("from", "agent@exämple.com")], &[]),
            Err(Error::NonAsciiValue("from".into()))
        );
        assert_eq!(
            signature_base(&[("from", "a\r\nb")], &[]),
            Err(Error::NonAsciiValue("from".into()))
        );
        assert_eq!(authority("bücher.example", None, 443), Err(Error::BadHost));
        assert_eq!(
            authority("xn--bcher-kva.example", None, 443).unwrap(),
            "xn--bcher-kva.example"
        );
    }

    #[test]
    fn authority_drops_the_default_port_and_lowercases() {
        assert_eq!(
            authority("Example.COM", Some(443), 443).unwrap(),
            "example.com"
        );
        assert_eq!(
            authority("example.com", Some(8443), 443).unwrap(),
            "example.com:8443"
        );
    }

    #[test]
    fn refuses_bad_and_duplicate_components() {
        assert_eq!(
            signature_params(&["Date"], &[]),
            Err(Error::BadComponentName("Date".into()))
        );
        assert_eq!(
            signature_params(&["from", "from"], &[]),
            Err(Error::DuplicateComponent("from".into()))
        );
    }

    #[test]
    fn escapes_structured_field_strings() {
        assert_eq!(sf_string(r#"a"b\c"#, "keyid").unwrap(), r#""a\"b\\c""#);
        assert_eq!(sf_string("é", "keyid"), Err(Error::BadParam("keyid")));
    }
}
