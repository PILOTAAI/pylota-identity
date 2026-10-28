//! `it::` tests against the Worker in workerd (`cargo xtask itest`, Testing § 6). The harness passes
//! `PM_ITEST_URL`, `PM_ITEST_CA`, `PM_ITEST_KEY` (a platform key with every platform permission),
//! `PM_ITEST_TOKEN`, and the local D1 location for the few fixtures only setup writes directly.

mod auth;
mod fakes;
mod identities;
mod intents;
mod keyring;
mod keys;
mod mailbox;
mod partners;
mod policy;
mod routes;
mod security;
mod tenants;

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

/// Runs SQL on the local D1 (for fixtures only `idvy setup` or the console writes).
pub fn d1(sql: &str) -> Value {
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
