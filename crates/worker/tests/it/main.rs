//! `it::` tests against the Worker in workerd (`cargo xtask itest`, Testing § 6). The harness passes
//! `PM_ITEST_URL`, `PM_ITEST_CA`, `PM_ITEST_KEY` (a platform key with every platform permission),
//! `PM_ITEST_TOKEN`, and the local D1 location for the few fixtures only setup writes directly.

mod addresses;
mod agentic;
mod auth;
mod bench;
mod cloud;
mod conformance;
mod delivery;
mod dkim;
mod domains;
mod erasure;
mod fakes;
mod idempotency;
mod identities;
mod inbound;
mod index;
mod intents;
mod keyring;
mod keys;
mod lists;
mod mailbox;
mod messages;
mod partners;
mod policy;
mod quarantine;
mod routes;
mod search;
mod security;
mod send;
mod ses;
mod tenants;
mod testmode;
mod thread;
mod triage;
mod wait;
mod webhooks;

use std::process::Command;
use std::sync::OnceLock;

use serde_json::{Value, json};

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

impl Resp {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn code(&self) -> &str {
        self.body["error"]["code"].as_str().unwrap_or("")
    }

    pub fn details(&self) -> &Value {
        &self.body["error"]["details"]
    }

    /// The body with its request ID removed, for comparing two errors.
    pub fn without_request_id(&self) -> Value {
        let mut b = self.body.clone();
        b["error"]["request_id"] = Value::Null;
        b
    }
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is set by `cargo xtask itest`"))
}

fn client() -> &'static reqwest::blocking::Client {
    static C: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    C.get_or_init(|| {
        let pem = std::fs::read(env("PM_ITEST_CA")).expect("the run's certificate");
        let url = env("PM_ITEST_URL");
        let port: u16 = url
            .rsplit(':')
            .next()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8799);
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        reqwest::blocking::Client::builder()
            .add_root_certificate(reqwest::Certificate::from_pem(&pem).expect("a PEM certificate"))
            .resolve("localhost", addr)
            .resolve("console.localhost", addr)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("the test client")
    })
}

/// One request; `key` is a bearer token.
pub fn call(
    method: &str,
    path: &str,
    key: Option<&str>,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> Resp {
    fakes::start();
    let url = format!("{}{path}", env("PM_ITEST_URL"));
    let mut req = client().request(reqwest::Method::from_bytes(method.as_bytes()).unwrap(), url);
    if let Some(k) = key {
        req = req.bearer_auth(k);
    }
    for (n, v) in headers {
        req = req.header(*n, *v);
    }
    if let Some(b) = body {
        req = req
            .header("content-type", "application/json")
            .body(b.to_string());
    }
    let r = req.send().expect("the Worker answers");
    let status = r.status().as_u16();
    let headers = r
        .headers()
        .iter()
        .map(|(n, v)| (n.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
        .collect();
    let text = r.text().unwrap_or_default();
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
    Resp {
        status,
        headers,
        body,
    }
}

pub fn get(path: &str, key: &str) -> Resp {
    call("GET", path, Some(key), None, &[])
}

pub fn post(path: &str, key: &str, body: Value) -> Resp {
    call("POST", path, Some(key), Some(body), &[])
}

pub fn patch(path: &str, key: &str, body: Value) -> Resp {
    call("PATCH", path, Some(key), Some(body), &[])
}

pub fn delete(path: &str, key: &str) -> Resp {
    call("DELETE", path, Some(key), None, &[])
}

/// A test hook call (`x-pm-test-token`).
pub fn hook(method: &str, path: &str, body: Option<Value>) -> Resp {
    let token = env("PM_ITEST_TOKEN");
    call(
        method,
        path,
        None,
        body,
        &[("x-pm-test-token", token.as_str())],
    )
}

pub fn platform() -> String {
    env("PM_ITEST_KEY")
}

/// A unique slug from the test's name (Testing § 6.7).
pub fn slug(name: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name
        .bytes()
        .chain(std::process::id().to_le_bytes())
        .chain(nanos().to_le_bytes())
    {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("t{:010x}", h & 0xff_ffff_ffff)
}

fn nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Creates a tenant with `key` and returns it.
pub fn tenant(key: &str, name: &str) -> Value {
    let r = post(
        "/v1/tenants",
        key,
        json!({ "slug": slug(name), "name": name, "mode": "live" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    r.body
}

/// Mints a key and returns `(id, secret)`.
pub fn mint(key: &str, body: Value) -> (String, String) {
    let r = post("/v1/keys", key, body);
    assert_eq!(r.status, 201, "{}", r.body);
    (
        r.body["id"].as_str().unwrap().to_owned(),
        r.body["secret"].as_str().unwrap().to_owned(),
    )
}

/// A partner and a partner key holding `permissions`.
pub fn partner(name: &str, permissions: &[&str]) -> (String, String) {
    let p = post("/v1/partners", &platform(), json!({ "name": name }));
    assert_eq!(p.status, 201, "{}", p.body);
    let id = p.body["id"].as_str().unwrap().to_owned();
    let (_, secret) = mint(
        &platform(),
        json!({ "name": name, "level": "partner", "partner_id": id, "permissions": permissions }),
    );
    (id, secret)
}

/// Runs SQL on the local D1 (for fixtures only `idvy setup` or the console writes). The file is the
/// running workerd's, so a busy database is tried again.
pub fn d1(sql: &str) -> Value {
    let mut tries = 0;
    let out = loop {
        let out = Command::new(env("PM_ITEST_WRANGLER"))
            .args([
                "d1",
                "execute",
                "pylota-identity",
                "--local",
                "--json",
                "--persist-to",
            ])
            .arg(env("PM_ITEST_STATE"))
            .arg("--config")
            .arg(env("PM_ITEST_CONFIG"))
            .args(["--command", sql])
            .env("CI", "1")
            .output()
            .expect("wrangler d1 execute");
        tries += 1;
        if out.status.success()
            || tries == 5
            || !String::from_utf8_lossy(&out.stderr).contains("SQLITE_BUSY")
        {
            break out;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).expect("JSON from wrangler");
    v[0]["results"].clone()
}

/// A tenant key that a workspace owner's console session minted (W35): written as the console will.
pub fn owner_key(tenant_id: &str, permissions: &[&str]) -> String {
    let pepper: Vec<u8> = (0..32)
        .map(|i| u8::from_str_radix(&env("PM_ITEST_PEPPER")[2 * i..2 * i + 2], 16).unwrap())
        .collect();
    let pepper: [u8; 32] = pepper.try_into().unwrap();
    let mut r = [0u8; 40];
    std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom").unwrap(), &mut r).unwrap();
    let token = pylota_identity_core::keys::generate(
        pylota_identity_core::keys::Mode::Live,
        r[..8].try_into().unwrap(),
        r[8..].try_into().unwrap(),
    );
    let lookup = pylota_identity_core::keys::parse(&token)
        .unwrap()
        .lookup
        .to_owned();
    let hash = pylota_identity_core::keys::hash(&pepper, &token);
    let id = format!("key_01JOWN{}00000000", lookup.to_ascii_uppercase());
    d1(&format!(
        "INSERT INTO api_keys (id, lookup, hash, name, level, tenant_id, mode, permissions_json, created_by_role, created_by_level, created_at) VALUES ('{id}', '{lookup}', '{hash}', 'owner', 'tenant', '{tenant_id}', 'live', '{}', 'owner', 'console', 1)",
        serde_json::to_string(permissions).unwrap()
    ));
    token
}

/// An event-stream request read until `stop` holds for the text so far (or the stream ends); the
/// connection is then dropped. Answers the status and the text read, and when the stream ended by
/// itself.
pub fn stream_post(
    path: &str,
    key: &str,
    body: Value,
    mut stop: impl FnMut(&str) -> bool,
) -> (u16, String, bool) {
    use std::io::Read as _;
    fakes::start();
    let mut r = client()
        .post(format!("{}{path}", env("PM_ITEST_URL")))
        .bearer_auth(key)
        .header("accept", "text/event-stream")
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .expect("the Worker answers");
    let status = r.status().as_u16();
    let mut text = String::new();
    let mut buf = [0u8; 4096];
    loop {
        match r.read(&mut buf) {
            Ok(0) => return (status, text, true),
            Ok(n) => {
                text.push_str(&String::from_utf8_lossy(&buf[..n]));
                if stop(&text) {
                    return (status, text, false);
                }
            }
            Err(_) => return (status, text, true),
        }
    }
}

/// Triggers a cron through wrangler's scheduled-event endpoint.
pub fn cron(expr: &str) {
    let q = expr.replace(' ', "+");
    for path in [
        format!("/cdn-cgi/handler/scheduled?cron={q}"),
        format!("/__scheduled?cron={q}"),
    ] {
        if call("GET", &path, None, None, &[]).status < 300 {
            return;
        }
    }
    panic!("no scheduled-event endpoint answered");
}

/// Waits for the next wall-clock minute unless `need` seconds remain in this one: workerd's local rate
/// limiter counts in fixed one-minute windows (`floor(now / period)`), so a burst that must land in
/// one window starts at the top of a minute.
pub fn fresh_minute(need: u64) {
    let into = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        % 60_000;
    if into + need * 1000 > 59_000 {
        std::thread::sleep(std::time::Duration::from_millis(60_000 - into + 200));
    }
}

/// Creates an identity in `tenant_id` and returns it.
pub fn identity(key: &str, tenant_id: &str, username: &str) -> Value {
    let r = post(
        &format!("/v1/tenants/{tenant_id}/identities"),
        key,
        json!({ "username": username, "display_name": username }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    r.body
}

/// The default tenant (`address_suffix = ''`), created by the harness as setup does.
pub fn default_tenant() -> String {
    d1("SELECT id FROM tenants WHERE address_suffix = ''")[0]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The system identity's ID, seeded by the harness as setup step 22 does.
pub fn system_identity() -> String {
    d1("SELECT id FROM identities WHERE is_system = 1")[0]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Runs an identity's mailbox alarm now (`/__test/alarm`).
pub fn alarm(identity_id: &str) {
    let r = hook(
        "POST",
        "/__test/alarm",
        Some(json!({ "identity_id": identity_id })),
    );
    assert!(r.body.get("ok").is_some(), "{}", r.body);
}

/// The `event_index` rows of an identity, oldest first: `(type, id)`.
pub fn indexed(identity_id: &str) -> Vec<(String, String)> {
    d1(&format!("SELECT type, id FROM event_index WHERE identity_id = '{identity_id}' ORDER BY occurred_at, id"))
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["type"].as_str().unwrap().to_owned(), r["id"].as_str().unwrap().to_owned()))
        .collect()
}

/// Delivers `raw` through wrangler's local email endpoint (Testing § 6.4); the status is `200`
/// (accepted), `400` (`setReject`) or `500` (a temporary failure).
pub fn inject(from: &str, to: &str, raw: &str) -> u16 {
    fakes::start();
    let url = format!(
        "{}/cdn-cgi/local/email?from={}&to={}",
        env("PM_ITEST_URL"),
        from.replace('+', "%2B"),
        to.replace('+', "%2B")
    );
    client()
        .post(url)
        .body(raw.to_owned())
        .send()
        .expect("the email endpoint answers")
        .status()
        .as_u16()
}

/// Runs the `email()` handler through `/__test/inbound` and returns `(outcome, smtp)`.
pub fn inbound(from: &str, to: &str, raw: &str) -> (String, String) {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut b64 = String::new();
    for c in raw.as_bytes().chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, &b)| a | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            b64.push(if i <= c.len() {
                char::from(T[((n >> (18 - 6 * i)) & 63) as usize])
            } else {
                '='
            });
        }
    }
    let body = json!({ "mail_from": from, "rcpt_to": to, "raw_base64": b64 });
    let mut r = hook("POST", "/__test/inbound", Some(body.clone()));
    // Wrangler's dev proxy now and then drops a request before the Worker answers ("Network
    // connection lost"); a redelivery is what a sending MTA would do, and ingest is idempotent.
    if r.status == 500 && r.body.to_string().contains("Network connection lost") {
        r = hook("POST", "/__test/inbound", Some(body));
    }
    assert_eq!(r.status, 200, "{}", r.body);
    (
        r.body["outcome"].as_str().unwrap_or("").to_owned(),
        r.body["smtp"].as_str().unwrap_or("").to_owned(),
    )
}

/// A plain-text message with a unique `Message-ID`.
pub fn mail(from: &str, to: &str, subject: &str, body: &str) -> String {
    format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nMessage-ID: <{}@it.example>\r\nDate: Mon, 26 Oct 2026 10:00:00 +0000\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n",
        slug(subject)
    )
}

/// Polls `f` every 250 ms for up to `secs` seconds until it answers `Some`.
pub fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// A tenant, an identity in it, and a tenant key holding the mail permissions:
/// `(tenant id, identity id, address, key)`.
pub fn mailbox_fixture(name: &str) -> (String, String, String, String) {
    let p = platform();
    let t = tenant(&p, name);
    let tid = t["id"].as_str().unwrap().to_owned();
    let idn = identity(&p, &tid, &slug(name)[..8]);
    let (_, key) = mint(
        &p,
        json!({ "name": name, "level": "tenant", "tenant_id": tid, "permissions": [
            "messages:read", "messages:write", "quarantine:review", "attachments:read", "search:read",
            "identities:read", "identities:write", "erasure:manage"
        ] }),
    );
    (
        tid,
        idn["id"].as_str().unwrap().to_owned(),
        idn["primary_address"].as_str().unwrap().to_owned(),
        key,
    )
}

/// The identity's messages through the read API (`status` filter with review), newest first.
pub fn messages_of(identity_id: &str, key: &str, query: &str) -> Vec<Value> {
    let r = get(
        &format!("/v1/identities/{identity_id}/messages?limit=100{query}"),
        key,
    );
    // A request in flight when a fault hook panics the isolate dies with it: polling tries again.
    if r.status >= 500 {
        return Vec::new();
    }
    assert_eq!(r.status, 200, "{}", r.body);
    r.body["data"].as_array().cloned().unwrap_or_default()
}

/// Waits until the identity holds `n` messages in `status`.
pub fn wait_stored(identity_id: &str, key: &str, status: &str, n: usize) -> Vec<Value> {
    eventually(30, &format!("{n} {status} message(s)"), || {
        let m = messages_of(identity_id, key, &format!("&status={status}"));
        (m.len() >= n).then_some(m)
    })
}

/// Rows of one SQL statement on an identity's mailbox (`/__test/mailbox-query`).
pub fn mailbox_query(identity_id: &str, sql: &str) -> Vec<Value> {
    let body = json!({ "identity_id": identity_id, "sql": sql });
    let mut r = hook("POST", "/__test/mailbox-query", Some(body.clone()));
    // Wrangler's dev proxy now and then drops a request (as in `inbound`); the statements are reads or
    // idempotent fixture writes.
    if r.body.to_string().contains("Network connection lost") {
        r = hook("POST", "/__test/mailbox-query", Some(body));
    }
    r.body["ok"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| panic!("{}", r.body))
}

/// Sends `body` to `queue` from inside the Worker (`/__test/queue-send`).
pub fn queue_send(queue: &str, body: Value) {
    let r = hook(
        "POST",
        "/__test/queue-send",
        Some(json!({ "queue": queue, "body": body })),
    );
    assert_eq!(r.status, 200, "{}", r.body);
}
