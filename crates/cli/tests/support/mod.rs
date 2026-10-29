//! The recorded fakes of the CLI tests (CLI and setup § Tests): one loopback HTTP server that answers as
//! the Cloudflare API, GitHub releases, DNS-over-HTTPS and the parts of the Worker that `setup`, `doctor`
//! and `domains subscribe` call, with a real SQLite database behind its D1 query API; a runner that
//! answers as Node.js, npm and Wrangler against the same state; and signed release bundles.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use pylota_identity_cli::{Cmd, Env, Proc, Runner};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

pub const ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
pub const MAIL_DOMAIN: &str = "agents.example";
pub const API_HOST: &str = "mail.example.com";
pub const VERSION: &str = pylota_identity_cli::VERSION;

/// The repository root.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// ---- Release bundles --------------------------------------------------------------------------------

/// A minisign key pair from a seed: the base64 public key and a signer.
pub struct Key {
    sk: ed25519_dalek::SigningKey,
    id: [u8; 8],
}

impl Key {
    pub fn new(seed: u8) -> Self {
        Self {
            sk: ed25519_dalek::SigningKey::from_bytes(&[seed; 32]),
            id: [seed, 1, 2, 3, 4, 5, 6, 7],
        }
    }

    pub fn public(&self) -> String {
        let mut b = b"Ed".to_vec();
        b.extend_from_slice(&self.id);
        b.extend_from_slice(self.sk.verifying_key().as_bytes());
        STANDARD.encode(b)
    }

    /// A prehashed (`ED`) minisign signature of `data` with this trusted comment.
    pub fn sign(&self, data: &[u8], trusted: &str) -> String {
        use blake2::Digest as _;
        use ed25519_dalek::Signer as _;
        let h = blake2::Blake2b512::digest(data);
        let sig = self.sk.sign(&h).to_bytes();
        let mut line = b"ED".to_vec();
        line.extend_from_slice(&self.id);
        line.extend_from_slice(&sig);
        let mut global = sig.to_vec();
        global.extend_from_slice(trusted.as_bytes());
        let gsig = self.sk.sign(&global).to_bytes();
        format!(
            "untrusted comment: signature from the idvy test key\n{}\ntrusted comment: {trusted}\n{}\n",
            STANDARD.encode(line),
            STANDARD.encode(gsig)
        )
    }
}

/// The files of a release tarball: the Worker build stand-ins, the repository's migrations, template
/// and a Wrangler lockfile, and `VERSION`.
pub fn bundle_files(version: &str) -> Vec<(String, Vec<u8>)> {
    let r = root();
    let mut files = vec![
        (
            "build/index.js".to_owned(),
            b"export default {};\n".to_vec(),
        ),
        (
            "build/index_bg.wasm".to_owned(),
            b"\0asm\x01\0\0\0".to_vec(),
        ),
        ("build/worker/shim.mjs".to_owned(), b"export {};\n".to_vec()),
        (
            "deploy/wrangler.toml.tmpl".to_owned(),
            std::fs::read(r.join("deploy/wrangler.toml.tmpl")).unwrap(),
        ),
        (
            "wrangler/package.json".to_owned(),
            br#"{"private":true,"dependencies":{"wrangler":"4.139.0"}}"#.to_vec(),
        ),
        (
            "wrangler/package-lock.json".to_owned(),
            br#"{"lockfileVersion":3,"packages":{}}"#.to_vec(),
        ),
        ("VERSION".to_owned(), format!("{version}\n").into_bytes()),
    ];
    for e in std::fs::read_dir(r.join("migrations/d1"))
        .unwrap()
        .flatten()
    {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.ends_with(".sql") {
            files.push((
                format!("migrations/d1/{n}"),
                std::fs::read(e.path()).unwrap(),
            ));
        }
    }
    files
}

/// A gzipped tarball of `files`; `symlink` adds a symbolic link entry.
pub fn tarball(files: &[(String, Vec<u8>)], symlink: Option<(&str, &str)>) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut t = tar::Builder::new(gz);
    for (name, bytes) in files {
        let mut h = tar::Header::new_old();
        h.set_size(bytes.len() as u64);
        h.set_mode(0o644);
        h.set_entry_type(tar::EntryType::Regular);
        // Written raw, so a hostile name such as `../escape` reaches the archive as it is.
        h.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
        h.set_cksum();
        t.append(&h, bytes.as_slice()).unwrap();
    }
    if let Some((name, target)) = symlink {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_size(0);
        h.set_cksum();
        t.append_link(&mut h, name, target).unwrap();
    }
    t.into_inner().unwrap().finish().unwrap()
}

/// The three assets of a release: tarball, `SHA256SUMS` and its signature.
pub fn release_assets(version: &str, key: &Key) -> BTreeMap<String, Vec<u8>> {
    let tar = tarball(&bundle_files(version), None);
    sign_assets(version, key, tar, &format!("pylota-identity v{version}"))
}

pub fn sign_assets(
    version: &str,
    key: &Key,
    tar: Vec<u8>,
    trusted: &str,
) -> BTreeMap<String, Vec<u8>> {
    let name = format!("pylota-identity-worker-{version}.tar.gz");
    let sums = format!(
        "{}  {name}\n",
        pylota_identity_cli::bundle::verify::sha256_hex(&tar)
    );
    let sig = key.sign(sums.as_bytes(), trusted);
    BTreeMap::from([
        (name, tar),
        ("SHA256SUMS".to_owned(), sums.into_bytes()),
        ("SHA256SUMS.sig".to_owned(), sig.into_bytes()),
    ])
}

// ---- State ------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Script {
    pub secrets: BTreeMap<String, String>,
    pub deployed: Option<String>,
    pub deploys: u32,
    pub ratelimit_ids: Vec<u32>,
}

/// Everything the fakes remember.
pub struct State {
    pub base: String,
    pub calls: Vec<(String, String)>,
    pub zones: Vec<Value>,
    pub records: BTreeMap<String, Vec<Value>>,
    pub routing: BTreeMap<String, Value>,
    pub catch_all: BTreeMap<String, Value>,
    pub sending: BTreeMap<String, Vec<Value>>,
    pub dbs: Vec<Value>,
    pub sql: BTreeMap<String, rusqlite::Connection>,
    pub buckets: BTreeSet<(String, String)>,
    pub lifecycle: BTreeMap<String, Value>,
    pub queues: Vec<Value>,
    pub indexes: BTreeMap<String, Value>,
    pub subscriptions: Vec<Value>,
    pub scripts: BTreeMap<String, Script>,
    pub releases: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    pub tenants: Vec<Value>,
    pub idempotency: BTreeMap<String, Value>,
    /// Answer every other `/v1/*` route with a generic document (`cli::output::json_every_command`).
    pub generic_api: bool,
    /// Drop the connection of the next request whose path starts with this, without answering.
    pub drop_next: Option<String>,
    /// Requests seen by the generic API: method, path and `Idempotency-Key`.
    pub api_requests: Vec<(String, String, Option<String>)>,
    pub seq: u64,
    pub runner_calls: Vec<Cmd>,
}

impl State {
    fn next_id(&mut self, prefix: &str) -> String {
        self.seq += 1;
        format!("{prefix}{:026}", self.seq)
    }

    pub fn count(&self, method: &str, path: &str) -> usize {
        self.calls
            .iter()
            .filter(|(m, p)| m == method && p == path)
            .count()
    }

    pub fn count_prefix(&self, method: &str, prefix: &str) -> usize {
        self.calls
            .iter()
            .filter(|(m, p)| m == method && p.starts_with(prefix))
            .count()
    }

    /// The D1 database of the deployment.
    pub fn db(&self) -> &rusqlite::Connection {
        self.sql.values().next().expect("a D1 database")
    }

    pub fn zone_id(&self, name: &str) -> String {
        self.zones
            .iter()
            .find(|z| z["name"] == name)
            .and_then(|z| z["id"].as_str())
            .unwrap_or("")
            .to_owned()
    }

    pub fn add_zone(&mut self, name: &str) -> String {
        let id = format!("zone{:028}", self.zones.len() + 1);
        self.zones.push(json!({ "id": id, "name": name, "name_servers": ["ada.ns.cloudflare.com", "bob.ns.cloudflare.com"] }));
        self.routing.insert(
            id.clone(),
            json!({ "enabled": false, "support_subaddress": false }),
        );
        self.catch_all.insert(id.clone(), json!({ "enabled": false, "actions": [{ "type": "drop" }], "matchers": [{ "type": "all" }] }));
        id
    }
}

pub type Shared = Arc<Mutex<State>>;

fn routing_records(domain: &str) -> Vec<Value> {
    vec![
        json!({ "type": "MX", "name": domain, "content": "route1.mx.cloudflare.net", "priority": 13 }),
        json!({ "type": "MX", "name": domain, "content": "route2.mx.cloudflare.net", "priority": 86 }),
        json!({ "type": "TXT", "name": domain, "content": "v=spf1 include:_spf.mx.cloudflare.net ~all" }),
    ]
}

fn sending_records(domain: &str) -> Vec<Value> {
    vec![
        json!({ "type": "TXT", "name": format!("cf2024-1._domainkey.{domain}"), "content": "v=DKIM1; h=sha256; k=rsa; p=MIIB" }),
        json!({ "type": "MX", "name": format!("cf-bounce.{domain}"), "content": "route1.mx.cloudflare.net", "priority": 10 }),
        json!({ "type": "TXT", "name": format!("cf-bounce.{domain}"), "content": "v=spf1 include:_spf.mx.cloudflare.net ~all" }),
        json!({ "type": "TXT", "name": format!("_dmarc.{domain}"), "content": "v=DMARC1; p=reject" }),
    ]
}

// ---- The HTTP server -----------------------------------------------------------------------------------

pub struct Fake {
    pub state: Shared,
    pub base: String,
    _rt: tokio::runtime::Runtime,
}

impl Fake {
    /// The fakes with the account's two zones and one published release of this CLI's version.
    pub fn start(key: &Key) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let listener = rt
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let state = Arc::new(Mutex::new(State {
            base: base.clone(),
            calls: vec![],
            zones: vec![],
            records: BTreeMap::new(),
            routing: BTreeMap::new(),
            catch_all: BTreeMap::new(),
            sending: BTreeMap::new(),
            dbs: vec![],
            sql: BTreeMap::new(),
            buckets: BTreeSet::new(),
            lifecycle: BTreeMap::new(),
            queues: vec![],
            indexes: BTreeMap::new(),
            subscriptions: vec![],
            scripts: BTreeMap::new(),
            releases: BTreeMap::new(),
            tenants: vec![],
            idempotency: BTreeMap::new(),
            generic_api: false,
            drop_next: None,
            api_requests: vec![],
            seq: 0,
            runner_calls: vec![],
        }));
        {
            let mut s = state.lock().unwrap();
            s.add_zone(MAIL_DOMAIN);
            s.add_zone("example.com");
            s.releases
                .insert(VERSION.to_owned(), release_assets(VERSION, key));
        }
        let st = state.clone();
        rt.spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    break;
                };
                let st = st.clone();
                tokio::spawn(async move { serve(sock, st).await });
            }
        });
        Self {
            state,
            base,
            _rt: rt,
        }
    }

    pub fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.state.lock().unwrap())
    }
}

async fn serve(mut sock: tokio::net::TcpStream, st: Shared) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let head_end = loop {
        let Ok(n) = sock.read(&mut tmp).await else {
            return;
        };
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split(' ');
    let method = parts.next().unwrap_or("").to_owned();
    let target = parts.next().unwrap_or("").to_owned();
    let headers: BTreeMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let len: usize = headers
        .get("content-length")
        .and_then(|l| l.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end..].to_vec();
    while body.len() < len {
        let Ok(n) = sock.read(&mut tmp).await else {
            return;
        };
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_owned(), q.to_owned()),
        None => (target.clone(), String::new()),
    };
    let q: BTreeMap<String, String> = query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect();
    let resp = {
        let mut s = st.lock().unwrap();
        if s.drop_next.as_deref().is_some_and(|p| path.starts_with(p)) {
            s.drop_next = None;
            s.api_requests.push((
                method.clone(),
                path.clone(),
                headers.get("idempotency-key").cloned(),
            ));
            None
        } else {
            s.calls.push((method.clone(), path.clone()));
            Some(route(&mut s, &method, &path, &q, &headers, &body))
        }
    };
    let Some((status, ctype, bytes)) = resp else {
        let _ = sock.shutdown().await;
        return;
    };
    let reason = if status < 300 { "OK" } else { "Error" };
    let mut out = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        bytes.len()
    )
    .into_bytes();
    out.extend_from_slice(&bytes);
    let _ = sock.write_all(&out).await;
    let _ = sock.shutdown().await;
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap_or(b'?'));
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

type Resp = (u16, &'static str, Vec<u8>);

fn cf_ok(result: Value) -> Resp {
    (
        200,
        "application/json",
        json!({ "success": true, "errors": [], "messages": [], "result": result,
        "result_info": { "page": 1, "per_page": 50, "total_pages": 1 } })
        .to_string()
        .into_bytes(),
    )
}

fn cf_err(status: u16, code: i64, msg: &str) -> Resp {
    (status, "application/json", json!({ "success": false, "errors": [{ "code": code, "message": msg }], "messages": [], "result": null }).to_string().into_bytes())
}

fn api(status: u16, v: Value) -> Resp {
    (status, "application/json", v.to_string().into_bytes())
}

fn api_err(status: u16, code: &str) -> Resp {
    api(
        status,
        json!({ "error": { "code": code, "message": code, "retryable": false, "request_id": "req_test" } }),
    )
}

fn route(
    s: &mut State,
    method: &str,
    path: &str,
    q: &BTreeMap<String, String>,
    h: &BTreeMap<String, String>,
    body: &[u8],
) -> Resp {
    let b: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let seg: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if let Some(rest) = path.strip_prefix("/client/v4") {
        return cloudflare(s, method, rest, q, h, &b, body);
    }
    match (method, seg.as_slice()) {
        (
            "GET",
            [
                "github",
                "repos",
                "PILOTAAI",
                "pylota-identity",
                "releases",
                "tags",
                tag,
            ],
        ) => {
            let v = tag.trim_start_matches('v');
            match s.releases.get(v) {
                Some(assets) => api(
                    200,
                    json!({ "tag_name": tag, "assets": assets.keys().map(|n| json!({
                    "name": n, "browser_download_url": format!("{}/assets/{v}/{n}", s.base) })).collect::<Vec<_>>() }),
                ),
                None => api(404, json!({ "message": "Not Found" })),
            }
        }
        (
            "GET",
            [
                "github",
                "repos",
                "PILOTAAI",
                "pylota-identity",
                "releases",
                "latest",
            ],
        ) => api(200, json!({ "tag_name": format!("v{VERSION}") })),
        ("GET", ["assets", v, name]) => match s.releases.get(*v).and_then(|a| a.get(*name)) {
            Some(bytes) => (200, "application/octet-stream", bytes.clone()),
            None => api(404, json!({})),
        },
        ("GET", ["doh" | "dns-query" | "resolve"]) => {
            let (name, rtype) = (
                q.get("name").cloned().unwrap_or_default(),
                q.get("type").cloned().unwrap_or_default(),
            );
            let answers: Vec<Value> = s
                .records
                .values()
                .flatten()
                .filter(|r| r["name"] == name.as_str() && r["type"] == rtype.as_str())
                .map(|r| {
                    let c = r["content"].as_str().unwrap_or("");
                    let data = match rtype.as_str() {
                        "MX" => format!("{} {c}.", r["priority"]),
                        "TXT" => format!("\"{c}\""),
                        _ => c.to_owned(),
                    };
                    json!({ "name": name, "type": 0, "data": data })
                })
                .collect();
            api(200, json!({ "Status": 0, "Answer": answers }))
        }
        _ => worker(s, method, &seg, q, h, &b),
    }
}

fn cloudflare(
    s: &mut State,
    method: &str,
    path: &str,
    q: &BTreeMap<String, String>,
    h: &BTreeMap<String, String>,
    b: &Value,
    raw: &[u8],
) -> Resp {
    let seg: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let jur = h
        .get("cf-r2-jurisdiction")
        .cloned()
        .unwrap_or_else(|| "default".into());
    match (method, seg.as_slice()) {
        ("GET", ["zones"]) => cf_ok(Value::Array(
            s.zones
                .iter()
                .filter(|z| q.get("name").is_none_or(|n| z["name"] == n.as_str()))
                .cloned()
                .collect(),
        )),
        ("GET", ["zones", z]) => match s.zones.iter().find(|x| x["id"] == *z) {
            Some(zone) => cf_ok(zone.clone()),
            None => cf_err(404, 1001, "no zone"),
        },
        ("GET", ["zones", z, "dns_records"]) => cf_ok(Value::Array(
            s.records
                .get(*z)
                .into_iter()
                .flatten()
                .filter(|r| q.get("type").is_none_or(|t| r["type"] == t.as_str()))
                .filter(|r| q.get("name.exact").is_none_or(|n| r["name"] == n.as_str()))
                .cloned()
                .collect(),
        )),
        ("POST", ["zones", z, "dns_records"]) => {
            let id = s.next_id("rec");
            let mut r = b.clone();
            r["id"] = json!(id);
            s.records
                .entry((*z).to_owned())
                .or_default()
                .push(r.clone());
            cf_ok(r)
        }
        ("DELETE", ["zones", z, "dns_records", id]) => {
            s.records
                .entry((*z).to_owned())
                .or_default()
                .retain(|r| r["id"] != *id);
            cf_ok(json!({ "id": id }))
        }
        ("GET", ["zones", z, "email", "routing"]) => {
            cf_ok(s.routing.get(*z).cloned().unwrap_or(Value::Null))
        }
        ("PATCH", ["zones", z, "email", "routing"]) => {
            let r = s.routing.entry((*z).to_owned()).or_insert(json!({}));
            for (k, v) in b.as_object().into_iter().flatten() {
                r[k] = v.clone();
            }
            cf_ok(r.clone())
        }
        ("POST", ["zones", z, "email", "routing", "dns"]) => {
            let name = b["name"].as_str().unwrap_or("").to_owned();
            s.routing.entry((*z).to_owned()).or_insert(json!({}))["enabled"] = json!(true);
            for mut r in routing_records(&name) {
                let id = s.next_id("rec");
                r["id"] = json!(id);
                s.records.entry((*z).to_owned()).or_default().push(r);
            }
            cf_ok(json!(routing_records(&name)))
        }
        ("GET", ["zones", z, "email", "routing", "dns"]) => {
            if q.contains_key("subdomain") {
                return cf_ok(json!([]));
            }
            let zone = s
                .zones
                .iter()
                .find(|x| x["id"] == *z)
                .and_then(|x| x["name"].as_str())
                .unwrap_or("")
                .to_owned();
            cf_ok(json!(routing_records(&zone)))
        }
        ("GET", ["zones", z, "email", "routing", "rules", "catch_all"]) => {
            cf_ok(s.catch_all.get(*z).cloned().unwrap_or(Value::Null))
        }
        ("PUT", ["zones", z, "email", "routing", "rules", "catch_all"]) => {
            s.catch_all.insert((*z).to_owned(), b.clone());
            cf_ok(b.clone())
        }
        ("GET", ["zones", z, "email", "sending", "subdomains"]) => {
            cf_ok(json!(s.sending.get(*z).cloned().unwrap_or_default()))
        }
        ("POST", ["zones", z, "email", "sending", "subdomains"]) => {
            let tag = s.next_id("tag");
            let name = b["name"].as_str().unwrap_or("").to_owned();
            let sub = json!({ "tag": tag, "name": name, "drop_suppressed_recipients": true, "preview_enabled": false });
            s.sending
                .entry((*z).to_owned())
                .or_default()
                .push(sub.clone());
            for mut r in sending_records(&name) {
                let id = s.next_id("rec");
                r["id"] = json!(id);
                s.records.entry((*z).to_owned()).or_default().push(r);
            }
            cf_ok(sub)
        }
        ("PATCH", ["zones", z, "email", "sending", "subdomains", tag]) => {
            for sub in s.sending.entry((*z).to_owned()).or_default() {
                if sub["tag"] == *tag {
                    for (k, v) in b.as_object().into_iter().flatten() {
                        sub[k] = v.clone();
                    }
                }
            }
            cf_ok(json!({}))
        }
        ("GET", ["zones", z, "email", "sending", "subdomains", tag, "dns"]) => {
            let name = s
                .sending
                .get(*z)
                .into_iter()
                .flatten()
                .find(|x| x["tag"] == *tag)
                .and_then(|x| x["name"].as_str())
                .unwrap_or("")
                .to_owned();
            cf_ok(json!(sending_records(&name)))
        }
        ("GET", ["accounts", _, "d1", "database"]) => cf_ok(Value::Array(
            s.dbs
                .iter()
                .filter(|d| q.get("name").is_none_or(|n| d["name"] == n.as_str()))
                .cloned()
                .collect(),
        )),
        ("POST", ["accounts", _, "d1", "database"]) => {
            let uuid = format!("d1-{:032}", s.dbs.len() + 1);
            let db = json!({ "uuid": uuid, "name": b["name"], "jurisdiction": b["jurisdiction"].as_str().unwrap_or("") });
            s.dbs.push(db.clone());
            let c = rusqlite::Connection::open_in_memory().unwrap();
            c.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
            s.sql.insert(uuid, c);
            cf_ok(db)
        }
        ("POST", ["accounts", _, "d1", "database", id, "query"]) => {
            let Some(c) = s.sql.get(*id) else {
                return cf_err(404, 7404, "no database");
            };
            let sql = b["sql"].as_str().unwrap_or("");
            let params: Vec<Option<String>> = b["params"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| p.as_str().map(str::to_owned))
                .collect();
            match d1(c, sql, &params) {
                Ok(rows) => cf_ok(json!([{ "results": rows, "success": true, "meta": {} }])),
                Err(e) => cf_err(400, 7500, &e),
            }
        }
        ("GET", ["accounts", _, "r2", "buckets", name]) => {
            if s.buckets.contains(&((*name).to_owned(), jur.clone())) {
                cf_ok(json!({ "name": name, "jurisdiction": jur }))
            } else {
                cf_err(404, 10006, "no bucket")
            }
        }
        ("POST", ["accounts", _, "r2", "buckets"]) => {
            s.buckets
                .insert((b["name"].as_str().unwrap_or("").to_owned(), jur.clone()));
            cf_ok(json!({ "name": b["name"], "jurisdiction": jur }))
        }
        ("GET", ["accounts", _, "r2", "buckets", name, "lifecycle"]) => cf_ok(
            s.lifecycle
                .get(*name)
                .cloned()
                .unwrap_or(json!({ "rules": [] })),
        ),
        ("PUT", ["accounts", _, "r2", "buckets", name, "lifecycle"]) => {
            s.lifecycle.insert((*name).to_owned(), b.clone());
            cf_ok(json!({}))
        }
        ("GET", ["accounts", _, "queues"]) => cf_ok(Value::Array(s.queues.clone())),
        ("POST", ["accounts", _, "queues"]) => {
            let id = s.next_id("q");
            let qv = json!({ "queue_id": id, "queue_name": b["queue_name"] });
            s.queues.push(qv.clone());
            cf_ok(qv)
        }
        ("GET", ["accounts", _, "vectorize", "v2", "indexes", name]) => {
            match s.indexes.get(*name) {
                Some(i) => cf_ok(i.clone()),
                None => cf_err(404, 3000, "no index"),
            }
        }
        ("POST", ["accounts", _, "vectorize", "v2", "indexes"]) => {
            let mut i = b.clone();
            i["metadata"] = json!([]);
            s.indexes
                .insert(b["name"].as_str().unwrap_or("").to_owned(), i.clone());
            cf_ok(i)
        }
        (
            "GET",
            [
                "accounts",
                _,
                "vectorize",
                "v2",
                "indexes",
                name,
                "metadata_index",
                "list",
            ],
        ) => cf_ok(
            json!({ "metadataIndexes": s.indexes.get(*name).map(|i| i["metadata"].clone()).unwrap_or(json!([])) }),
        ),
        (
            "POST",
            [
                "accounts",
                _,
                "vectorize",
                "v2",
                "indexes",
                name,
                "metadata_index",
                "create",
            ],
        ) => {
            if let Some(i) = s.indexes.get_mut(*name) {
                i["metadata"].as_array_mut().unwrap().push(
                    json!({ "propertyName": b["propertyName"], "indexType": b["indexType"] }),
                );
            }
            cf_ok(json!({}))
        }
        ("GET", ["accounts", _, "event_subscriptions", "subscriptions"]) => {
            cf_ok(Value::Array(s.subscriptions.clone()))
        }
        ("GET", ["accounts", _, "workers", "scripts", name, "secrets"]) => {
            match s.scripts.get(*name) {
                Some(sc) => cf_ok(Value::Array(
                    sc.secrets
                        .keys()
                        .map(|k| json!({ "name": k, "type": "secret_text" }))
                        .collect(),
                )),
                None => cf_err(404, 10007, "no such script"),
            }
        }
        ("GET", ["accounts", _, "workers", "scripts"]) => cf_ok(Value::Array(
            s.scripts.keys().map(|k| json!({ "id": k })).collect(),
        )),
        ("GET", ["accounts", _, "workers", "scripts", name, "settings"]) => cf_ok(
            json!({ "bindings": s.scripts.get(*name).map(|sc| sc.ratelimit_ids.iter()
            .map(|n| json!({ "type": "ratelimit", "namespace_id": n.to_string() })).collect::<Vec<_>>()).unwrap_or_default() }),
        ),
        ("POST", ["accounts", _, "analytics_engine", "sql"]) => {
            let _ = raw;
            api(200, json!({ "data": [{ "n": 0 }] }))
        }
        _ => cf_err(404, 7003, &format!("no route {method} {path}")),
    }
}

/// One D1 query-API statement against SQLite: rows of a `SELECT`, `[]` otherwise; a statement without
/// parameters may hold several statements (a migration).
fn d1(
    c: &rusqlite::Connection,
    sql: &str,
    params: &[Option<String>],
) -> Result<Vec<Value>, String> {
    if params.is_empty() && !sql.trim_start().to_ascii_uppercase().starts_with("SELECT") {
        return c
            .execute_batch(sql)
            .map(|_| vec![])
            .map_err(|e| e.to_string());
    }
    let mut st = c.prepare(sql).map_err(|e| e.to_string())?;
    let names: Vec<String> = st.column_names().iter().map(|s| (*s).to_owned()).collect();
    let bound = rusqlite::params_from_iter(params.iter());
    let mut rows = st.query(bound).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(r) = rows.next().map_err(|e| e.to_string())? {
        let mut o = serde_json::Map::new();
        for (i, n) in names.iter().enumerate() {
            let v = match r.get_ref(i).map_err(|e| e.to_string())? {
                rusqlite::types::ValueRef::Null => Value::Null,
                rusqlite::types::ValueRef::Integer(x) => json!(x),
                rusqlite::types::ValueRef::Real(x) => json!(x),
                rusqlite::types::ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
                rusqlite::types::ValueRef::Blob(b) => json!(STANDARD.encode(b)),
            };
            o.insert(n.clone(), v);
        }
        out.push(Value::Object(o));
    }
    Ok(out)
}

/// The Worker routes `setup`, `doctor` and `domains subscribe` use, over the same SQLite database.
fn worker(
    s: &mut State,
    method: &str,
    seg: &[&str],
    q: &BTreeMap<String, String>,
    h: &BTreeMap<String, String>,
    b: &Value,
) -> Resp {
    let name = s.scripts.keys().next().cloned().unwrap_or_default();
    let deployed = s.scripts.get(&name).and_then(|sc| sc.deployed.clone());
    if let ["health"] = seg {
        return match deployed {
            Some(v) => api(200, json!({ "status": "ok", "version": v })),
            None => api(404, json!({ "message": "no worker" })),
        };
    }
    let key = h
        .get("authorization")
        .and_then(|a| a.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_owned();
    if s.generic_api && seg.first() == Some(&"v1") {
        s.api_requests.push((
            method.to_owned(),
            format!("/{}", seg.join("/")),
            h.get("idempotency-key").cloned(),
        ));
        return generic(seg);
    }
    let me = match authenticate(s, &key) {
        Some(m) => m,
        None => return api_err(401, "unauthorized"),
    };
    let now = 1_793_000_000_000i64;
    match (method, seg) {
        ("GET", ["v1", "me"]) => api(200, me),
        ("GET", ["v1", "tenants"]) => api(200, json!({ "data": s.tenants, "next_cursor": null })),
        ("POST", ["v1", "tenants"]) => {
            if let Some(t) = h.get("idempotency-key").and_then(|k| s.idempotency.get(k)) {
                return api(201, t.clone());
            }
            let id = s.next_id("ten_");
            let slug = b["slug"].as_str().unwrap_or("").to_owned();
            let suffix = b["address_suffix"].as_str().unwrap_or("").to_owned();
            let t = json!({ "id": id, "slug": slug, "name": b["name"], "mode": "live", "status": "active", "address_suffix": suffix });
            s.db()
                .execute(
                    "INSERT INTO tenants (id, slug, name, mode, address_suffix, suffix_fold, policy_json, quota_do_id, notify_do_id, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, 'live', ?4, ?4, '{}', 'q', '', ?5, ?5)",
                    rusqlite::params![id, slug, b["name"].as_str(), suffix, now],
                )
                .unwrap();
            s.tenants.push(t.clone());
            if let Some(k) = h.get("idempotency-key") {
                s.idempotency.insert(k.clone(), t.clone());
            }
            api(201, t)
        }
        ("POST", ["v1", "tenants", _t, "identities"]) => api(
            201,
            json!({ "id": "idn_00000000000000000000DOCTOR", "username": "idvy-doctor" }),
        ),
        ("POST", ["v1", "identities", _, "messages"]) => api(
            202,
            json!({ "message": { "id": "msg_00000000000000000000000001", "status": "queued" }, "deduplicated": false }),
        ),
        ("GET", ["v1", "identities", _, "wait"]) => api(
            200,
            json!({ "message": null, "verification": null, "timed_out": true }),
        ),
        ("DELETE", ["v1", "identities", _, "messages", _]) => api(202, json!({})),
        ("GET", ["v1", "tenants", t, "domains"]) => {
            let rows = d1(
                s.db(),
                "SELECT id FROM domains WHERE tenant_id = ?1",
                &[Some((*t).to_owned())],
            )
            .unwrap_or_default();
            let data: Vec<Value> = rows
                .iter()
                .filter_map(|r| r["id"].as_str())
                .map(|id| domain_json(s, id))
                .collect();
            api(200, json!({ "data": data, "next_cursor": null }))
        }
        ("GET", ["v1", "domains", id]) => {
            let d = domain_json(s, id);
            if d.is_null() {
                api_err(404, "domain_not_found")
            } else {
                api(200, d)
            }
        }
        _ => {
            let _ = q;
            api_err(404, "route_not_found")
        }
    }
}

/// A domain row as the API's object (`delivery_events` derived as the Worker derives it).
fn domain_json(s: &State, id: &str) -> Value {
    let rows = d1(s.db(), "SELECT id, tenant_id, name, transport, sending, state, event_subscription_id FROM domains WHERE id = ?1", &[Some(id.to_owned())]).unwrap_or_default();
    let Some(r) = rows.first() else {
        return Value::Null;
    };
    let sending = r["sending"] == 1;
    let events = match (
        sending,
        r["transport"].as_str(),
        r["event_subscription_id"].is_null(),
    ) {
        (false, _, _) => "none",
        (true, Some("cloudflare"), true) => "manual",
        _ => "active",
    };
    json!({ "id": r["id"], "tenant_id": r["tenant_id"], "name": r["name"], "transport": r["transport"], "sending": sending,
            "state": r["state"], "delivery_events": events, "method": "cloudflare_zone" })
}

/// `GET /v1/me` for a key in `api_keys`, checked against the pepper Wrangler uploaded.
fn authenticate(s: &State, key: &str) -> Option<Value> {
    let parts = pylota_identity_core::keys::parse(key)?;
    let pepper = s
        .scripts
        .values()
        .find_map(|sc| sc.secrets.get("PM_KEY_PEPPER"))?;
    let pepper: [u8; 32] = STANDARD.decode(pepper).ok()?.try_into().ok()?;
    let rows = d1(
        s.db(),
        "SELECT id, hash, level, permissions_json FROM api_keys WHERE lookup = ?1",
        &[Some(parts.lookup.to_owned())],
    )
    .ok()?;
    let r = rows.first()?;
    if !pylota_identity_core::keys::verify(&pepper, key, r["hash"].as_str()?) {
        return None;
    }
    Some(
        json!({ "key_id": r["id"], "name": "setup-bootstrap", "level": r["level"], "mode": "live", "partner_id": null,
                 "tenant_id": null, "identity_id": null,
                 "permissions": serde_json::from_str::<Value>(r["permissions_json"].as_str()?).ok()?, "mcp_tools": null, "expires_at": null }),
    )
}

/// A generic answer for any `/v1/*` route.
fn generic(seg: &[&str]) -> Resp {
    let last = seg.last().copied().unwrap_or("");
    let id = format!(
        "{}_00000000000000000000000001",
        match seg.get(1).copied().unwrap_or("") {
            "tenants" => "ten",
            "identities" => "idn",
            "domains" => "dom",
            "webhooks" => "whk",
            "keys" => "key",
            "partners" => "ptn",
            _ => "obj",
        }
    );
    if last == "raw"
        || last == "attachments"
        || seg.len() >= 2 && seg[seg.len() - 2] == "attachments"
    {
        return (200, "application/octet-stream", b"bytes".to_vec());
    }
    api(
        200,
        json!({ "id": id, "data": [], "next_cursor": null, "hits": [], "message": { "id": "msg_1", "status": "queued" },
                     "deduplicated": false, "timed_out": false, "secret": "whsec_x", "status": "answered", "answer": { "text": "ok" },
                     "evidence": [], "trace": [], "records": [], "pages": [] }),
    )
}

// ---- The runner ------------------------------------------------------------------------------------

/// Node.js, npm and Wrangler, answered from the shared state; `sh` runs for real (key commands).
pub struct FakeRunner {
    pub state: Shared,
}

impl Runner for FakeRunner {
    fn run(&self, cmd: &Cmd) -> Result<Proc, String> {
        self.state.lock().unwrap().runner_calls.push(cmd.clone());
        let ok = |out: &str| {
            Ok(Proc {
                code: 0,
                stdout: out.as_bytes().to_vec(),
                stderr: vec![],
            })
        };
        let args: Vec<&str> = cmd.args.iter().map(String::as_str).collect();
        match cmd.program.as_str() {
            "node" => ok("v22.12.0\n"),
            "npm" => ok("10.9.0\n"),
            "sh" | "cmd" => {
                let mut child = std::process::Command::new(&cmd.program)
                    .args(&cmd.args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| e.to_string())?;
                let limit = std::time::Duration::from_secs(cmd.timeout_s.unwrap_or(60));
                let deadline = std::time::Instant::now() + limit;
                while child.try_wait().map_err(|e| e.to_string())?.is_none() {
                    if std::time::Instant::now() > deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(format!("{} timed out", cmd.program));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                let out = child.wait_with_output().map_err(|e| e.to_string())?;
                Ok(Proc {
                    code: out.status.code().unwrap_or(1),
                    stdout: out.stdout,
                    stderr: out.stderr,
                })
            }
            p if p.ends_with("/wrangler") => {
                let mut s = self.state.lock().unwrap();
                match args.as_slice() {
                    ["--version"] => ok(" ⛅️ wrangler 4.139.0\n"),
                    ["deploy", "--config", file, ..] => {
                        let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
                        let doc: toml::Table = toml::from_str(&text).map_err(|e| e.to_string())?;
                        let name = doc["name"].as_str().unwrap_or("").to_owned();
                        let version =
                            std::fs::read_to_string(cmd.cwd.as_ref().unwrap().join("VERSION"))
                                .map_err(|e| e.to_string())?;
                        let ids: Vec<u32> = doc
                            .get("ratelimits")
                            .and_then(|r| r.as_array())
                            .into_iter()
                            .flatten()
                            .filter_map(|r| r.get("namespace_id")?.as_str()?.parse().ok())
                            .collect();
                        let sc = s.scripts.entry(name).or_default();
                        sc.deployed = Some(version.trim().to_owned());
                        sc.deploys += 1;
                        sc.ratelimit_ids = ids;
                        ok(
                            "Uploaded pylota-identity\nCurrent Version ID: 095f00a7-0000-0000-0000-000000000000\n",
                        )
                    }
                    ["secret", "put", name, "--name", worker] => {
                        let v = String::from_utf8_lossy(cmd.stdin.as_deref().unwrap_or_default())
                            .into_owned();
                        s.scripts
                            .entry((*worker).to_owned())
                            .or_default()
                            .secrets
                            .insert((*name).to_owned(), v);
                        ok("✨ Success! Uploaded secret\n")
                    }
                    ["queues", "subscription", "create", queue, rest @ ..] => {
                        let flag = |f: &str| {
                            rest.iter()
                                .position(|a| *a == f)
                                .and_then(|i| rest.get(i + 1))
                                .map(|v| (*v).to_owned())
                        };
                        let id = s.next_id("sub");
                        s.subscriptions.push(json!({ "id": id, "name": flag("--name").unwrap_or_default(),
                            "source": { "type": "email.sending", "zone_id": flag("--zone-id"), "domain": flag("--domain") },
                            "destination": { "queue": queue } }));
                        ok("Created subscription\n")
                    }
                    other => Ok(Proc {
                        code: 1,
                        stdout: vec![],
                        stderr: format!("unexpected wrangler {other:?}").into_bytes(),
                    }),
                }
            }
            other => Err(format!("{other}: not found")),
        }
    }
}

// ---- Running the CLI ------------------------------------------------------------------------------------

/// A writer the test reads afterwards.
#[derive(Clone, Default)]
pub struct Buf(pub Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The answer of one invocation.
#[derive(Debug)]
pub struct Run {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Run {
    pub fn json(&self) -> Value {
        serde_json::from_str(self.out.trim())
            .unwrap_or_else(|e| panic!("not one JSON document ({e}): {}", self.out))
    }
}

/// A home directory, a working directory and the fakes.
pub struct Cli {
    pub home: PathBuf,
    pub vars: BTreeMap<String, String>,
    pub fake: Option<Arc<Fake>>,
    pub key: Key,
}

impl Cli {
    pub fn new(fake: Option<Arc<Fake>>) -> Self {
        let home =
            std::env::temp_dir().join(format!("idvy-test-{}-{}", std::process::id(), unique()));
        std::fs::create_dir_all(&home).unwrap();
        let mut vars = BTreeMap::from([("HOME".to_owned(), home.display().to_string())]);
        if fake.is_some() {
            vars.insert("CLOUDFLARE_API_TOKEN".into(), "cf-test-token".into());
            vars.insert("CLOUDFLARE_ACCOUNT_ID".into(), ACCOUNT.into());
        }
        Self {
            home,
            vars,
            fake,
            key: Key::new(7),
        }
    }

    pub fn with_fake() -> (Self, Arc<Fake>) {
        let key = Key::new(7);
        let fake = Arc::new(Fake::start(&key));
        (Self::new(Some(fake.clone())), fake)
    }

    pub fn env(&self, stdin: &[u8]) -> (Env, Buf, Buf) {
        let (out, err) = (Buf::default(), Buf::default());
        let base = self
            .fake
            .as_ref()
            .map(|f| f.base.clone())
            .unwrap_or_else(|| "http://127.0.0.1:9".into());
        let runner: Arc<dyn Runner> = match &self.fake {
            Some(f) => Arc::new(FakeRunner {
                state: f.state.clone(),
            }),
            None => Arc::new(FakeRunner {
                state: Arc::new(Mutex::new(Fake::empty_state())),
            }),
        };
        let env = Env {
            vars: self.vars.clone(),
            cwd: self.home.clone(),
            stdin: Box::new(std::io::Cursor::new(stdin.to_vec())),
            stdin_tty: false,
            stdout_tty: false,
            stderr_tty: false,
            out: Box::new(out.clone()),
            err: Box::new(err.clone()),
            http: Some(reqwest::Client::builder().no_proxy().build().unwrap()),
            cloudflare_api: format!("{base}/client/v4"),
            github_api: format!("{base}/github"),
            runner,
            now_ms: None,
            release_keys: Some(vec![self.key.public()]),
            hosts: BTreeMap::from([
                (API_HOST.to_owned(), base.clone()),
                ("cloudflare-dns.com".to_owned(), base.clone()),
                ("dns.google".to_owned(), base.clone()),
            ]),
        };
        (env, out, err)
    }

    /// Runs `idvy <args>` with `stdin`.
    pub fn run_in(&self, args: &[&str], stdin: &[u8]) -> Run {
        let (env, out, err) = self.env(stdin);
        let mut argv = vec!["idvy".to_owned()];
        argv.extend(args.iter().map(|a| (*a).to_owned()));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let code = rt.block_on(pylota_identity_cli::run(argv, env));
        let s = |b: &Buf| String::from_utf8_lossy(&b.0.lock().unwrap()).into_owned();
        Run {
            code,
            out: s(&out),
            err: s(&err),
        }
    }

    pub fn run(&self, args: &[&str]) -> Run {
        self.run_in(args, b"")
    }

    pub fn deploy_dir(&self) -> PathBuf {
        self.home.join("deploy")
    }

    /// Writes the config file.
    pub fn write_config(&self, text: &str, mode: u32) -> PathBuf {
        let dir = self.home.join(".config/pylota-identity");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.toml");
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(text.as_bytes()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        p
    }
}

impl Fake {
    fn empty_state() -> State {
        State {
            base: String::new(),
            calls: vec![],
            zones: vec![],
            records: BTreeMap::new(),
            routing: BTreeMap::new(),
            catch_all: BTreeMap::new(),
            sending: BTreeMap::new(),
            dbs: vec![],
            sql: BTreeMap::new(),
            buckets: BTreeSet::new(),
            lifecycle: BTreeMap::new(),
            queues: vec![],
            indexes: BTreeMap::new(),
            subscriptions: vec![],
            scripts: BTreeMap::new(),
            releases: BTreeMap::new(),
            tenants: vec![],
            idempotency: BTreeMap::new(),
            generic_api: false,
            drop_next: None,
            api_requests: vec![],
            seq: 0,
            runner_calls: vec![],
        }
    }
}

fn unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed) * 1000
        + std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as u64
            % 1000
}

/// `idvy setup` with every value given.
pub fn setup_args() -> Vec<&'static str> {
    vec![
        "setup",
        "--domain",
        API_HOST,
        "--mail-domain",
        MAIL_DOMAIN,
        "--jurisdiction",
        "eu",
        "--owner-email",
        "sam@acmecarhire.example",
        "--owner-name",
        "Sam Patel",
        "--yes",
    ]
}
