//! DKIM-signed test mail: the corpus's test-only key (`s1._domainkey.brightwell.example`, in the
//! corpus zones the fake DoH serves), signed with mail-auth's signer in the test process.

use mail_auth::common::crypto::{RsaKey, Sha256};
use mail_auth::common::headers::HeaderWriter as _;
use mail_auth::dkim::DkimSigner;
use rustls_pki_types::{PrivateKeyDer, PrivatePkcs1KeyDer};

fn key() -> RsaKey<Sha256> {
    let pem = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../conformance/corpus/keys/brightwell-s1.test-only.pem"),
    )
    .expect("the corpus test key");
    let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    const T: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut der = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in b64.bytes().filter(|c| *c != b'=') {
        acc = (acc << 6) | T.find(char::from(c)).expect("base64") as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            der.push((acc >> bits) as u8);
        }
    }
    RsaKey::<Sha256>::from_key_der(PrivateKeyDer::Pkcs1(PrivatePkcs1KeyDer::from(der)))
        .expect("an RSA key")
}

/// `raw` with a `DKIM-Signature` of `d=brightwell.example; s=s1` prepended. The `From` must be at
/// `brightwell.example` for DMARC to pass (`p=reject` there).
pub fn sign(raw: &str) -> String {
    let sig = DkimSigner::from_key(key())
        .domain("brightwell.example")
        .selector("s1")
        .headers(["From", "To", "Subject", "Message-ID", "Date"])
        .sign(raw.as_bytes())
        .expect("signed");
    format!("{}{raw}", sig.to_header())
}
