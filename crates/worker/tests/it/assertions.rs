//! Agent assertions through workerd (Agent signing keys § 4; O4–O6, O7, O27, O29, FR-IDN-6, FR-IDN-7).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pylota_identity_core::jwt;
use serde_json::{Value, json};

use crate::{Resp, call, d1, fakes, identity, mint, platform, post, tenant};

/// An identity and a tenant key that can sign and manage its keys.
pub struct Signer {
    pub tid: String,
    pub iid: String,
    pub address: String,
    pub key: String,
}

pub fn signer(tag: &str) -> Signer {
    let p = platform();
    let t = tenant(&p, tag);
    let tid = t["id"].as_str().unwrap().to_owned();
    let i = identity(&p, &tid, "agent");
    let (_, key) = mint(
        &p,
        json!({ "name": tag, "level": "tenant", "tenant_id": tid, "permissions": [
            "identities:read", "identities:write", "identities:sign"
        ] }),
    );
    Signer {
        tid,
        iid: i["id"].as_str().unwrap().to_owned(),
        address: i["primary_address"].as_str().unwrap().to_owned(),
        key,
    }
}

pub fn assert_as(s: &Signer, body: Value) -> Resp {
    post(
        &format!("/v1/identities/{}/assertions", s.iid),
        &s.key,
        body,
    )
}

/// A minted token, which must be accepted.
pub fn token(s: &Signer) -> Value {
    let r = assert_as(s, json!({ "audience": "https://portal.supplier.example" }));
    assert_eq!(r.status, 201, "{}", r.body);
    r.body
}

/// The identity's JWKS without a key: `(status, keys)`.
pub fn jwks(iid: &str) -> (u16, Vec<Value>, Resp) {
    let r = call(
        "GET",
        &format!("/.well-known/jwks/{iid}.json"),
        None,
        None,
        &[],
    );
    let keys = r.body["keys"].as_array().cloned().unwrap_or_default();
    (r.status, keys, r)
}

/// The claims of `token`, verified against the identity's published key `kid`.
pub fn verified(iid: &str, token: &str) -> Value {
    let head = jwt::header(token).unwrap();
    let (_, keys, _) = jwks(iid);
    let k = keys
        .iter()
        .find(|k| k["kid"] == head["kid"])
        .unwrap_or_else(|| panic!("kid {} not published", head["kid"]));
    let x: [u8; 32] = URL_SAFE_NO_PAD
        .decode(k["x"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let (_, payload) = jwt::verify(token, &x, Some(jwt::ASSERTION_TYP)).unwrap();
    serde_json::from_slice(&payload).unwrap()
}

/// O4–O6: audience, expiry and `ext` rules; the claims of § 4.2.
#[test]
fn claims_and_limits() {
    let _reset = fakes::Reset;
    let s = signer("claims_and_limits");
    for bad in [
        json!({}),
        json!({ "audience": "" }),
        json!({ "audience": "a".repeat(257) }),
        json!({ "audience": "café" }),
    ] {
        let r = assert_as(&s, bad.clone());
        assert_eq!(
            (
                r.status,
                r.code(),
                r.details()["errors"][0]["path"].as_str()
            ),
            (400, "invalid_request", Some("audience")),
            "{bad}"
        );
    }
    for e in [59, 601] {
        let r = assert_as(&s, json!({ "audience": "x", "expires_in": e }));
        assert_eq!((r.status, r.code()), (400, "invalid_request"), "{e}");
    }
    for ext in [
        json!({ "iss": "evil" }),
        json!({ "email": "a@b" }),
        json!({ "unverified": {} }),
        json!({ "pad": "x".repeat(2100) }),
    ] {
        let r = assert_as(&s, json!({ "audience": "x", "ext": ext }));
        assert_eq!((r.status, r.code()), (400, "invalid_request"), "{ext}");
    }
    // Nothing was signed, so no key exists yet.
    let n = d1(&format!(
        "SELECT COUNT(*) AS n FROM identity_keys WHERE identity_id = '{}'",
        s.iid
    ));
    assert_eq!(n[0]["n"], 0);

    let r = assert_as(
        &s,
        json!({ "audience": "https://portal.supplier.example", "nonce": "b3f1", "ext": { "booking_ref": "BK-2291" } }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let tok = r.body["assertion"].as_str().unwrap();
    let c = verified(&s.iid, tok);
    assert_eq!(c["iss"], format!("https://{}", api_host()));
    assert_eq!(c["sub"], s.iid);
    assert_eq!(c["aud"], "https://portal.supplier.example");
    assert_eq!(c["exp"].as_i64().unwrap() - c["iat"].as_i64().unwrap(), 300);
    assert_eq!(c["nbf"], c["iat"]);
    assert_eq!(c["email"], s.address);
    assert_eq!(
        (&c["email_verified"], &c["ai_agent"]),
        (&json!(true), &json!(true))
    );
    assert_eq!(c["unverified"]["name"], "agent");
    assert_eq!(c["unverified"]["accountable_human"], false);
    assert_eq!(
        (&c["nonce"], &c["ext"]["booking_ref"]),
        (&json!("b3f1"), &json!("BK-2291"))
    );
    assert_eq!(c["jti"].as_str().unwrap().len(), 26);
    assert_eq!(r.body["kid"], jwt::header(tok).unwrap()["kid"]);
    assert_eq!(
        r.body["jwks_uri"],
        format!("https://{}/.well-known/jwks/{}.json", api_host(), s.iid)
    );
    // An Idempotency-Key is ignored: every call mints a new token.
    let path = format!("/v1/identities/{}/assertions", s.iid);
    let body = json!({ "audience": "x" });
    let a = call(
        "POST",
        &path,
        Some(&s.key),
        Some(body.clone()),
        &[("idempotency-key", "same")],
    );
    let b = call(
        "POST",
        &path,
        Some(&s.key),
        Some(body),
        &[("idempotency-key", "same")],
    );
    assert_eq!((a.status, b.status), (201, 201));
    assert_ne!(a.body["assertion"], b.body["assertion"]);
    let stored = d1(&format!(
        "SELECT COUNT(*) AS n FROM idempotency_records WHERE identity_id = '{}'",
        s.iid
    ));
    assert_eq!(stored[0]["n"], 0);
}

fn api_host() -> String {
    std::env::var("PM_ITEST_URL")
        .unwrap()
        .trim_start_matches("https://")
        .to_owned()
}

/// O29: the `email` claim and the signed `From` name the primary address only while mail to it
/// reaches the deployment; otherwise the platform address.
#[test]
fn o29_vouched_address() {
    let _reset = fakes::Reset;
    let (s, did, zone) = crate::addresses::on_domain("o29");
    let r = crate::addresses::create(&s, "desk", &did);
    assert_eq!(r.status, 201, "{}", r.body);
    let aid = r.body["id"].as_str().unwrap().to_owned();
    let r = crate::addresses::promote(&s, &aid, json!({}));
    assert_eq!(r.status, 200, "{}", r.body);
    let primary = format!("desk@{zone}");
    let platform_addr = s.addr.clone();
    let (_, key) = mint(
        &platform(),
        json!({ "name": "o29", "level": "tenant", "tenant_id": s.tid, "permissions": ["identities:sign"] }),
    );
    patch_policy(&s.tid, true);
    fakes::set_config("PM_WEB_BOT_AUTH", Some("on"));
    let signer = Signer {
        tid: s.tid.clone(),
        iid: s.iid.clone(),
        address: primary.clone(),
        key,
    };
    let email = |sg: &Signer| -> (String, String) {
        let t = token(sg);
        let c = verified(&sg.iid, t["assertion"].as_str().unwrap());
        assert_eq!(c["email_verified"], true);
        let h = post(
            &format!("/v1/identities/{}/http-signatures", sg.iid),
            &sg.key,
            json!({ "url": "https://www.brightwell.example/" }),
        );
        assert_eq!(h.status, 200, "{}", h.body);
        (
            c["email"].as_str().unwrap().to_owned(),
            h.body["headers"]["From"].as_str().unwrap().to_owned(),
        )
    };
    assert_eq!(
        email(&signer),
        (primary.clone(), primary.clone()),
        "healthy"
    );
    let set = |sql: &str| {
        d1(&sql.replace("{did}", &did).replace("{aid}", &aid));
    };
    for (change, restore) in [
        (
            "UPDATE domains SET state = 'suspended' WHERE id = '{did}'",
            "UPDATE domains SET state = 'healthy' WHERE id = '{did}'",
        ),
        (
            "UPDATE domains SET state = 'failing' WHERE id = '{did}'",
            "UPDATE domains SET state = 'healthy' WHERE id = '{did}'",
        ),
        (
            "UPDATE domains SET inbound = 'none' WHERE id = '{did}'",
            "UPDATE domains SET inbound = 'routing' WHERE id = '{did}'",
        ),
        (
            "UPDATE domains SET inbound = 'forward' WHERE id = '{did}'; UPDATE addresses SET forwarding = 'failed' WHERE id = '{aid}'",
            "UPDATE addresses SET forwarding = 'ok' WHERE id = '{aid}'",
        ),
        (
            "UPDATE addresses SET forwarding = 'unverified' WHERE id = '{aid}'",
            "UPDATE addresses SET forwarding = 'ok' WHERE id = '{aid}'",
        ),
    ] {
        set(change);
        assert_eq!(
            email(&signer),
            (platform_addr.clone(), platform_addr.clone()),
            "{change}"
        );
        set(restore);
    }
    // Forwarding with a passed check, and a healthy domain again, restore the primary address.
    assert_eq!(
        email(&signer),
        (primary.clone(), primary.clone()),
        "forward ok"
    );
    set(
        "UPDATE domains SET inbound = 'routing' WHERE id = '{did}'; UPDATE addresses SET forwarding = NULL WHERE id = '{aid}'",
    );
    assert_eq!(email(&signer), (primary.clone(), primary), "restored");
}

pub fn patch_policy(tid: &str, allowed: bool) {
    let r = crate::patch(
        &format!("/v1/tenants/{tid}/policy"),
        &platform(),
        json!({ "web_bot_auth": { "allowed": allowed } }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
}

/// O27: the SDK verifier accepts a fresh token and rejects a wrong audience, an expired token, an
/// unknown kid, `alg: none`, a crafted `sub` (with no request) and a JWKS answer that is a redirect or
/// not `application/jwk-set+json`.
#[test]
fn sdk_verifies() {
    use pylota_identity::assertions::{AssertionError, Verifier, VerifierConfig};
    let _reset = fakes::Reset;
    let s = signer("sdk_verifies");
    let issuer = format!("https://{}", api_host());
    let aud = "https://portal.supplier.example";
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let http = || {
        let pem = std::fs::read(std::env::var("PM_ITEST_CA").unwrap()).unwrap();
        let port: u16 = api_host().rsplit(':').next().unwrap().parse().unwrap();
        reqwest::Client::builder()
            .add_root_certificate(reqwest::Certificate::from_pem(&pem).unwrap())
            .resolve(
                "localhost",
                std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            )
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    };
    let verifier = |audience: &str| {
        Verifier::new(VerifierConfig::new([issuer.clone()], audience)).with_http_client(http())
    };
    let tok = token(&s)["assertion"].as_str().unwrap().to_owned();
    let now = fakes::worker_now() / 1000;
    let v = verifier(aud);
    let a = rt.block_on(v.verify_assertion_at(&tok, now)).unwrap();
    assert_eq!(
        (a.sub.as_str(), a.email.as_str()),
        (s.iid.as_str(), s.address.as_str())
    );
    assert!(matches!(
        rt.block_on(v.verify_assertion_at(&tok, now)),
        Err(AssertionError::Replayed)
    ));
    assert!(matches!(
        rt.block_on(verifier("https://other.example").verify_assertion_at(&tok, now)),
        Err(AssertionError::Audience)
    ));
    assert!(matches!(
        rt.block_on(verifier(aud).verify_assertion_at(&tok, now + 300 + 61)),
        Err(AssertionError::Expired)
    ));
    // An unknown kid: the token's header names a key the identity never had.
    let parts: Vec<&str> = tok.split('.').collect();
    let enc = |v: &Value| URL_SAFE_NO_PAD.encode(v.to_string());
    let unknown = format!(
        "{}.{}.{}",
        enc(&json!({ "alg": "EdDSA", "typ": "agent-assertion+jwt", "kid": "A".repeat(43) })),
        parts[1],
        parts[2]
    );
    assert!(matches!(
        rt.block_on(verifier(aud).verify_assertion_at(&unknown, now)),
        Err(AssertionError::UnknownKey)
    ));
    let none = format!(
        "{}.{}.",
        enc(&json!({ "alg": "none", "typ": "agent-assertion+jwt" })),
        parts[1]
    );
    assert!(matches!(
        rt.block_on(verifier(aud).verify_assertion_at(&none, now)),
        Err(AssertionError::Algorithm)
    ));
    // A crafted sub: refused before any request (the issuer here does not even resolve).
    let crafted =
        enc(&json!({ "iss": "https://unresolvable.invalid", "sub": "../../v1/links/abc?" }));
    let off = Verifier::new(VerifierConfig::new(["https://unresolvable.invalid"], aud));
    assert!(matches!(
        rt.block_on(off.verify_assertion_at(&format!("{}.{crafted}.{}", parts[0], parts[2]), now)),
        Err(AssertionError::Malformed)
    ));
    // A key set answered with a redirect, or with another content type: a local server stands in for
    // the issuer (the signature is never reached).
    for (answer, what) in [
        (
            "HTTP/1.1 302 Found\r\nLocation: https://elsewhere.example/keys.json\r\nContent-Length: 0\r\n\r\n",
            "redirect",
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"keys\":[]}",
            "content type",
        ),
    ] {
        let iss = serve_once(answer);
        let claims = enc(&json!({ "iss": iss, "sub": s.iid }));
        let local = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let v = Verifier::new(VerifierConfig::new([iss.clone()], aud)).with_http_client(local);
        let got =
            rt.block_on(v.verify_assertion_at(&format!("{}.{claims}.{}", parts[0], parts[2]), now));
        let ok = matches!(
            (&got, what),
            (
                Err(AssertionError::Jwks(pylota_identity::Error::Status {
                    status: 302,
                    ..
                })),
                "redirect",
            ) | (
                Err(AssertionError::Jwks(pylota_identity::Error::Decode(_))),
                "content type"
            )
        );
        assert!(ok, "{what}: {got:?}");
    }
    // A paused identity's key set is a 404.
    let r = crate::patch(
        &format!("/v1/identities/{}", s.iid),
        &platform(),
        json!({ "status": "paused" }),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(matches!(
        rt.block_on(verifier(aud).verify_assertion_at(&tok, now)),
        Err(AssertionError::Jwks(_))
    ));
}

/// A plain-HTTP server on a free local port that answers one request with `answer`; its origin.
fn serve_once(answer: &'static str) -> String {
    use std::io::{Read as _, Write as _};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        if let Ok((mut c, _)) = l.accept() {
            let mut buf = [0u8; 4096];
            let _ = c.read(&mut buf);
            let _ = c.write_all(answer.as_bytes());
        }
    });
    origin
}

/// O7: identity erasure deletes the keys and tombstones each kid; it is never published again.
#[test]
fn erasure_tombstones_kid() {
    let _reset = fakes::Reset;
    let s = signer("erasure_tombstones_kid");
    let kid = token(&s)["kid"].as_str().unwrap().to_owned();
    let r = crate::erasure::erase(
        &platform(),
        json!({ "tenant_id": s.tid, "scope": "identity", "identity_id": s.iid }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    let done = crate::erasure::run(r.body["id"].as_str().unwrap());
    assert_eq!(done["status"], "completed", "{done}");
    let left = d1(&format!(
        "SELECT COUNT(*) AS n FROM identity_keys WHERE identity_id = '{}'",
        s.iid
    ));
    assert_eq!(left[0]["n"], 0);
    let t = d1(&format!(
        "SELECT kid FROM key_tombstones WHERE kid = '{kid}'"
    ));
    assert_eq!(t.as_array().unwrap().len(), 1);
    let (status, _, r) = jwks(&s.iid);
    assert_eq!((status, r.code()), (404, "identity_not_found"));
}
