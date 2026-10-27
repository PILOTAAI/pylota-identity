//! Spike S13 (build plan M1): does a request signed by `core::httpsig` pass Cloudflare's Web Bot Auth
//! format check? `https://crawltest.com/cdn-cgi/web-bot-auth` answers 401 for a well-formed message with an
//! unknown key, 200 for a known key that verifies and 400 otherwise (Cloudflare "Web Bot Auth", read
//! 2026-10-27). A fresh key is never registered, so the pass is 401.
//!
//! A request with no signature must answer 400, so a 401 cannot come from an endpoint that refuses
//! everything the same way. The other variants are recorded, not judged: they show which defects the
//! format check catches (400) and which reach verification (401).
//!
//! Run: `cargo run -p pylota-identity-spike-s13`. Exit 0 when the well-formed request answers 401 and
//! the unsigned one 400.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer as _, SigningKey};
use pylota_identity_core::{httpsig, jwk};

const URL: &str = "https://crawltest.com/cdn-cgi/web-bot-auth";
const HOST: &str = "crawltest.com";
const AGENT: &str = "https://api.pylotamail.com";
const FROM: &str = "spike-s13@pylotamail.com";

fn main() {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("randomness");
    let key = SigningKey::from_bytes(&seed);
    let keyid = jwk::thumbprint(&key.verifying_key().to_bytes());

    let client = reqwest::blocking::Client::new();
    let quoted = format!("\"{AGENT}\"");
    let well_formed = send(&client, signed(&key, &keyid, &quoted, Variant::WellFormed));
    let unsigned = send(&client, Vec::new());
    println!("keyid {keyid}");
    println!("well-formed, unknown key:          {well_formed} (want 401)");
    println!("no signature:                      {unsigned} (want 400)");
    for (label, agent, variant) in [
        ("unquoted Signature-Agent", AGENT, Variant::WellFormed),
        (
            "signature-agent not covered",
            quoted.as_str(),
            Variant::AgentNotCovered,
        ),
        ("no tag", quoted.as_str(), Variant::NoTag),
        ("already expired", quoted.as_str(), Variant::Expired),
        (
            "signature over another base",
            quoted.as_str(),
            Variant::WrongBase,
        ),
    ] {
        println!(
            "{label:<34} {} (recorded)",
            send(&client, signed(&key, &keyid, agent, variant))
        );
    }
    let pass = well_formed == 401 && unsigned == 400;
    println!("S13 {}", if pass { "PASS" } else { "FAIL" });
    std::process::exit(if pass { 0 } else { 1 });
}

#[derive(Clone, Copy, PartialEq)]
enum Variant {
    WellFormed,
    AgentNotCovered,
    NoTag,
    Expired,
    WrongBase,
}

/// The headers of Agent signing keys §5.2 for a GET of `URL`, or one defect of `variant`.
fn signed(
    key: &SigningKey,
    keyid: &str,
    agent_header: &str,
    variant: Variant,
) -> Vec<(&'static str, String)> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    let (created, expires) = if variant == Variant::Expired {
        (now - 120, now - 60)
    } else {
        (now, now + 60)
    };
    let mut nonce = [0u8; 64];
    getrandom::fill(&mut nonce).expect("randomness");
    let mut params = vec![
        httpsig::Param::Created(created),
        httpsig::Param::Expires(expires),
        httpsig::Param::KeyId(keyid.to_owned()),
        httpsig::Param::Alg("ed25519".to_owned()),
        httpsig::Param::Nonce(STANDARD.encode(nonce)),
    ];
    if variant != Variant::NoTag {
        params.push(httpsig::Param::Tag("web-bot-auth".to_owned()));
    }
    let authority = httpsig::authority(HOST, None, 443).expect("ASCII host");
    let mut covered = vec![
        ("@authority", authority.as_str()),
        ("signature-agent", agent_header),
        ("from", FROM),
    ];
    if variant == Variant::AgentNotCovered {
        covered.remove(1);
    }
    let names: Vec<&str> = covered.iter().map(|(n, _)| *n).collect();
    let mut base = httpsig::signature_base(&covered, &params).expect("ASCII components");
    if variant == Variant::WrongBase {
        base.push('x');
    }
    let input = httpsig::signature_params(&names, &params).expect("valid params");
    let signature = STANDARD.encode(key.sign(base.as_bytes()).to_bytes());
    vec![
        ("Signature-Agent", agent_header.to_owned()),
        ("From", FROM.to_owned()),
        ("Signature-Input", format!("sig1={input}")),
        ("Signature", format!("sig1=:{signature}:")),
    ]
}

fn send(client: &reqwest::blocking::Client, headers: Vec<(&'static str, String)>) -> u16 {
    let mut request = client.get(URL);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    request
        .send()
        .expect("request to crawltest.com")
        .status()
        .as_u16()
}
