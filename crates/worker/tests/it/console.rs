//! The console through workerd (Console and workspaces › Tests): sign-in, sessions, CSRF, roles and
//! scope, mail rendering, the policy page and the render budget. A [`Browser`] keeps one cookie jar and
//! sends the `Origin` a browser would.

use serde_json::{Value, json};

use crate::{Resp, call, d1, fakes, hook, identity, platform, post, tenant};

pub const HOST: &str = "console.localhost";

fn origin() -> String {
    let port = std::env::var("PM_ITEST_URL")
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap()
        .to_owned();
    format!("https://{HOST}:{port}")
}

/// One browser: its session cookie, if any.
#[derive(Clone, Default)]
pub struct Browser {
    pub cookie: Option<String>,
}

impl Browser {
    fn send(
        &mut self,
        method: &str,
        path: &str,
        form: Option<&[(&str, &str)]>,
        origin_header: Option<&str>,
    ) -> Resp {
        fakes::start();
        let url = format!("{}{path}", origin());
        let mut req =
            crate::client().request(reqwest::Method::from_bytes(method.as_bytes()).unwrap(), url);
        if let Some(c) = &self.cookie {
            req = req.header("cookie", format!("__Host-pm_session={c}"));
        }
        if let Some(o) = origin_header {
            req = req.header("origin", o);
        }
        if let Some(f) = form {
            let body: String = f
                .iter()
                .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
                .collect::<Vec<_>>()
                .join("&");
            req = req
                .header("content-type", "application/x-www-form-urlencoded")
                .body(body);
        }
        let r = req.send().expect("the Worker answers");
        let status = r.status().as_u16();
        let headers: Vec<(String, String)> = r
            .headers()
            .iter()
            .map(|(n, v)| (n.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
            .collect();
        for (n, v) in &headers {
            if n == "set-cookie" && v.starts_with("__Host-pm_session=") {
                let value = v["__Host-pm_session=".len()..]
                    .split(';')
                    .next()
                    .unwrap_or("");
                self.cookie = (!value.is_empty()).then(|| value.to_owned());
            }
        }
        let text = r.text().unwrap_or_default();
        Resp {
            status,
            headers,
            body: Value::String(text),
        }
    }

    pub fn get(&mut self, path: &str) -> Resp {
        self.send("GET", path, None, None)
    }

    /// A form `POST` with the page's CSRF token and this console's `Origin`.
    pub fn post(&mut self, path: &str, form: &[(&str, &str)]) -> Resp {
        let o = origin();
        self.send("POST", path, Some(form), Some(&o))
    }

    /// A `POST` as `post`, adding the CSRF token of the page at `from`.
    pub fn submit(&mut self, from: &str, path: &str, form: &[(&str, &str)]) -> Resp {
        let page = self.get(from);
        let token = csrf(&html(&page));
        let mut f: Vec<(&str, &str)> = vec![("_csrf", token.as_str())];
        f.extend_from_slice(form);
        self.post(path, &f)
    }

    pub fn post_with_origin(&mut self, path: &str, form: &[(&str, &str)], o: Option<&str>) -> Resp {
        self.send("POST", path, Some(form), o)
    }
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

pub fn html(r: &Resp) -> String {
    r.body.as_str().unwrap_or("").to_owned()
}

/// The first `_csrf` value of a page.
pub fn csrf(page: &str) -> String {
    page.split(r#"name="_csrf" value=""#)
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap_or("")
        .to_owned()
}

/// A signed-in browser for `email` with `role` in `tenant_id` (`/__test/console-session`).
pub fn as_member(email: &str, tenant_id: &str, role: &str) -> Browser {
    session_with(email, Some(tenant_id), role, 0)
}

pub fn session_with(email: &str, tenant_id: Option<&str>, role: &str, ago_ms: i64) -> Browser {
    let r = hook(
        "POST",
        "/__test/console-session",
        Some(
            json!({ "email": email, "tenant_id": tenant_id, "role": role, "authenticated_ago_ms": ago_ms }),
        ),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    Browser {
        cookie: r.body["cookie"].as_str().map(str::to_owned),
    }
}

/// A workspace with one inbox: `(tenant id, identity id, address)`.
pub fn workspace(tag: &str) -> (String, String, String) {
    let t = tenant(&platform(), tag);
    let tid = t["id"].as_str().unwrap().to_owned();
    let i = identity(&platform(), &tid, "desk");
    (
        tid,
        i["id"].as_str().unwrap().to_owned(),
        i["primary_address"].as_str().unwrap().to_owned(),
    )
}

pub fn email(tag: &str) -> String {
    format!("{}@people.example", crate::slug(tag))
}

/// The six-digit code of the last sign-in email to `to`.
pub fn code_for(to: &str, after: usize) -> String {
    crate::eventually(30, "the sign-in email", || {
        let m = fakes::mail_sent(to);
        if m.len() <= after {
            return None;
        }
        let text = m.last()?["text"].as_str()?.to_owned();
        text.split("enter this code: ")
            .nth(1)
            .map(|r| r.chars().take(6).collect())
    })
}

pub fn link_for(to: &str) -> String {
    crate::eventually(30, "the sign-in email", || {
        let text = fakes::mail_sent(to).last()?["text"].as_str()?.to_owned();
        text.split("?t=")
            .nth(1)
            .map(|r| r.split_whitespace().next().unwrap_or("").to_owned())
    })
}

/// Signs `email` in through the email form and the code; answers the browser.
pub fn sign_in(email: &str) -> Browser {
    let mut b = Browser::default();
    let before = fakes::mail_sent(email).len();
    let r = b.post("/console/sign-in", &[("email", email)]);
    assert_eq!(r.status, 200, "{}", r.body);
    let code = code_for(email, before);
    let r = b.post(
        "/console/sign-in/code",
        &[("email", email), ("code", &code)],
    );
    assert_eq!(r.status, 303, "{}", r.body);
    assert!(b.cookie.is_some());
    b
}

/// W15: a fourth request in 10 minutes is refused; a token burns after 10 failed codes; `RL_SIGNIN`
/// refuses an 11th request in 60 s from one network, also from another address in the same IPv6 /64;
/// known and unknown addresses get identical responses and only the known one gets mail.
#[test]
fn w15_signin_limits() {
    let _reset = fakes::Reset;
    let (tid, _, _) = workspace("w15");
    let known = email("w15 known");
    as_member(&known, &tid, "member");
    let unknown = email("w15 unknown");
    crate::fresh_minute(20);
    let ip = |n: u32| format!("2001:db8:15:{n}::1");
    let mut b = Browser::default();
    let send_from = |b: &mut Browser, who: &str, net: &str| {
        let o = origin();
        let mut req_form = vec![("email", who)];
        let _ = &mut req_form;
        fakes::start();
        let r = crate::client()
            .post(format!("{}/console/sign-in", origin()))
            .header("origin", o)
            .header("cf-connecting-ip", net)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("email={}", enc(who)))
            .send()
            .unwrap();
        let s = r.status().as_u16();
        let _ = b;
        (s, r.text().unwrap_or_default())
    };
    let k1 = send_from(&mut b, &known, &ip(1));
    let u1 = send_from(&mut b, &unknown, &ip(2));
    assert_eq!(k1, u1, "the same page for known and unknown addresses");
    assert_eq!(k1.0, 200);
    for _ in 0..2 {
        assert_eq!(send_from(&mut b, &known, &ip(3)).0, 200);
    }
    assert_eq!(
        send_from(&mut b, &known, &ip(4)).0,
        429,
        "a fourth request in 10 minutes"
    );
    assert_eq!(send_from(&mut b, &unknown, &ip(5)).0, 200);
    crate::eventually(30, "the known address's mail", || {
        (fakes::mail_sent(&known).len() >= 3).then_some(())
    });
    assert!(
        fakes::mail_sent(&unknown).is_empty(),
        "no mail to an unknown address"
    );
    // A token burns after 10 failed codes.
    let other = email("w15 burn");
    as_member(&other, &tid, "viewer");
    let before = fakes::mail_sent(&other).len();
    assert_eq!(send_from(&mut b, &other, &ip(6)).0, 200);
    let good = code_for(&other, before);
    let bad = if good == "000000" { "111111" } else { "000000" };
    for i in 0..10 {
        let r = post_code(&other, bad, &format!("2001:db8:16:{i}::1"));
        assert_eq!(r, 400, "attempt {i}");
    }
    assert_eq!(
        post_code(&other, &good, "2001:db8:17::1"),
        400,
        "burned after 10 failures"
    );
    // RL_SIGNIN: 10 a minute per network; a /64 is one network.
    crate::fresh_minute(15);
    let net = |n: u32| format!("2001:db8:99:1::{n}");
    let mut statuses = Vec::new();
    for n in 0..11 {
        statuses.push(send_from(&mut b, &email(&format!("w15 rl {n}")), &net(n + 1)).0);
    }
    assert!(statuses[..10].iter().all(|s| *s == 200), "{statuses:?}");
    assert_eq!(statuses[10], 429, "{statuses:?}");
}

fn post_code(email: &str, code: &str, ip: &str) -> u16 {
    fakes::start();
    crate::client()
        .post(format!("{}/console/sign-in/code", origin()))
        .header("origin", origin())
        .header("cf-connecting-ip", ip)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("email={}&code={code}", enc(email)))
        .send()
        .unwrap()
        .status()
        .as_u16()
}

/// The link's `GET` changes nothing; link and code are single use and burn each other.
#[test]
fn signin_link_and_code() {
    let _reset = fakes::Reset;
    let (tid, _, _) = workspace("signin link");
    let who = email("signin link");
    as_member(&who, &tid, "admin");
    let mut b = Browser::default();
    let before = fakes::mail_sent(&who).len();
    b.post("/console/sign-in", &[("email", &who)]);
    let code = code_for(&who, before);
    let t = link_for(&who);
    for _ in 0..2 {
        let r = b.get(&format!("/console/sign-in/link?t={t}"));
        assert_eq!(r.status, 200);
        assert!(html(&r).contains(r#"name="t""#));
    }
    let r = b.post("/console/sign-in/link", &[("t", &t)]);
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(r.header("location"), Some("/console"));
    assert!(b.cookie.is_some());
    assert_eq!(b.get("/console").status, 200);
    // The link again, and the code of the same email: both burned.
    let mut c = Browser::default();
    assert_eq!(c.post("/console/sign-in/link", &[("t", &t)]).status, 400);
    assert_eq!(
        c.post("/console/sign-in/code", &[("email", &who), ("code", &code)])
            .status,
        400
    );
    // Expired after 10 minutes.
    let before = fakes::mail_sent(&who).len();
    c.post("/console/sign-in", &[("email", &who)]);
    let code = code_for(&who, before);
    fakes::advance(10 * 60_000 + 1_000);
    assert_eq!(
        c.post("/console/sign-in/code", &[("email", &who), ("code", &code)])
            .status,
        400
    );
    // A fresh request signs in with its code.
    let mut d = sign_in(&who);
    assert_eq!(d.get("/console").status, 200);
}

/// W16: a `POST` without the token, with another session's token, without `Origin`, with
/// `Origin: null`, a foreign origin or `http://` with the right host gets `403`; the cookie has
/// `__Host-`, `Secure`, `HttpOnly` and `SameSite=Lax`; pages carry the console's headers.
#[test]
#[allow(clippy::type_complexity)]
fn w16_csrf() {
    let _reset = fakes::Reset;
    let (tid, _, _) = workspace("w16");
    let who = email("w16");
    as_member(&who, &tid, "owner");
    let mut b = Browser::default();
    let before = fakes::mail_sent(&who).len();
    b.post("/console/sign-in", &[("email", &who)]);
    let code = code_for(&who, before);
    let r = b.post("/console/sign-in/code", &[("email", &who), ("code", &code)]);
    let cookie = r.header("set-cookie").unwrap().to_owned();
    assert!(cookie.starts_with("__Host-pm_session="), "{cookie}");
    for attr in ["Path=/", "Secure", "HttpOnly", "SameSite=Lax"] {
        assert!(cookie.contains(attr), "{cookie}");
    }
    let page = b.get("/console/settings");
    assert_eq!(page.header("referrer-policy"), Some("same-origin"));
    assert!(
        page.header("content-security-policy")
            .unwrap()
            .contains("default-src 'none'")
    );
    assert_eq!(page.header("cache-control"), Some("no-store"));
    let token = csrf(&html(&page));
    let other = as_member(&email("w16 other"), &tid, "admin");
    let mut o2 = other.clone();
    let other_token = csrf(&html(&o2.get("/console/settings")));
    let path = "/console/settings/profile";
    let port_origin = origin();
    let http = port_origin.replace("https://", "http://");
    let cases: Vec<(Vec<(&str, &str)>, Option<&str>)> = vec![
        (vec![("name", "x")], Some(&port_origin)),
        (
            vec![("_csrf", &other_token), ("name", "x")],
            Some(&port_origin),
        ),
        (vec![("_csrf", &token), ("name", "x")], None),
        (vec![("_csrf", &token), ("name", "x")], Some("null")),
        (
            vec![("_csrf", &token), ("name", "x")],
            Some("https://evil.example"),
        ),
        (vec![("_csrf", &token), ("name", "x")], Some(&http)),
    ];
    for (form, o) in &cases {
        let r = b.post_with_origin(path, form, *o);
        assert_eq!(r.status, 403, "{form:?} {o:?}");
    }
    let ok = b.post_with_origin(
        path,
        &[("_csrf", &token), ("name", "Ola")],
        Some(&port_origin),
    );
    assert_eq!(ok.status, 303, "{}", ok.body);
}

/// W17: hostile HTML renders only in a token-less sandboxed `srcdoc` frame under a policy without a
/// script source; the text view is the default and escapes everything.
#[test]
fn w17_hostile_html() {
    let _reset = fakes::Reset;
    let s = crate::send::open_sender("w17 console", "live");
    let from = "hostile@w17.example";
    let html_body = r#"<p>Hi</p><script>alert(1)</script><img src="https://tracker.example/p.gif" onerror="alert(2)"><form action="https://evil.example"><input name=x></form><a href="javascript:alert(3)">x</a>"#;
    let raw = format!(
        "From: {from}\r\nTo: {}\r\nSubject: <b>w17</b>\r\nMessage-ID: <w17-{}@w17.example>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nPlain <script>x</script>\r\n--b\r\nContent-Type: text/html\r\n\r\n{html_body}\r\n--b--\r\n",
        s.addr,
        crate::slug("w17")
    );
    let (o, _) = crate::inbound(from, &s.addr, &raw);
    assert_eq!(o, "accepted");
    let m = crate::wait_stored(&s.iid, &s.key, "received", 1)[0].clone();
    let thr = m["thread_id"].as_str().unwrap();
    let mid = m["id"].as_str().unwrap();
    let mut b = as_member(&email("w17"), &s.tid, "viewer");
    let text = b.get(&format!("/console/inboxes/{}/threads/{thr}", s.iid));
    assert_eq!(text.status, 200, "{}", text.body);
    let page = html(&text);
    assert!(!page.contains("<script"), "text view escapes");
    assert!(page.contains("&lt;script&gt;"));
    assert!(page.contains("&lt;b&gt;w17&lt;/b&gt;"));
    let h = b.get(&format!(
        "/console/inboxes/{}/threads/{thr}?html={mid}",
        s.iid
    ));
    let page = html(&h);
    assert!(
        page.contains(r#"<iframe class="mail" sandbox="" title="Message HTML" srcdoc=""#),
        "{page}"
    );
    let srcdoc = page
        .split("srcdoc=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    for bad in [
        "<script",
        "onerror",
        "javascript:",
        "<form",
        "tracker.example",
    ] {
        assert!(
            !srcdoc.to_ascii_lowercase().contains(bad),
            "{bad} in {srcdoc}"
        );
    }
    let csp = h.header("content-security-policy").unwrap();
    assert!(
        !csp.contains("script-src") && csp.contains("img-src 'self' data:"),
        "{csp}"
    );
    let remote = b.get(&format!(
        "/console/inboxes/{}/threads/{thr}?html={mid}&images=remote",
        s.iid
    ));
    assert!(
        remote
            .header("content-security-policy")
            .unwrap()
            .contains("img-src 'self' data: https:")
    );
}

/// The console attachment route serves the API's bytes and serving headers for every role, the same
/// `404` for another workspace's IDs and nothing for a quarantined message; a 1 MiB `cid:` PNG becomes
/// a `data:` URI and a 3 MiB one a link; the page loads nothing from the API host.
#[test]
fn attachments_and_inline_images() {
    let _reset = fakes::Reset;
    let s = crate::send::open_sender("console attachments", "live");
    let from = "pics@attach.example";
    let png = |n: usize| {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        v.resize(n, 7);
        pylota_identity_core::links::encode_base64(&v)
    };
    let fold = |b: String| -> String {
        b.as_bytes()
            .chunks(76)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join("\r\n")
    };
    let raw = format!(
        "From: {from}\r\nTo: {}\r\nSubject: pics\r\nMessage-ID: <pics-{}@attach.example>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=r\r\n\r\n--r\r\nContent-Type: text/html\r\n\r\n<p><img src=\"cid:small@x\"><img src=\"cid:big@x\"></p>\r\n--r\r\nContent-Type: image/png\r\nContent-ID: <small@x>\r\nContent-Disposition: inline; filename=small.png\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n--r\r\nContent-Type: image/png\r\nContent-ID: <big@x>\r\nContent-Disposition: inline; filename=big.png\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n--r--\r\n",
        s.addr,
        crate::slug("pics"),
        fold(png(1024 * 1024)),
        fold(png(3 * 1024 * 1024)),
    );
    let (o, _) = crate::inbound(from, &s.addr, &raw);
    assert_eq!(o, "accepted");
    let m = crate::wait_stored(&s.iid, &s.key, "received", 1)[0].clone();
    let (thr, mid) = (m["thread_id"].as_str().unwrap(), m["id"].as_str().unwrap());
    let att = m["attachments"][0]["id"].as_str().unwrap().to_owned();
    for role in ["viewer", "member", "admin", "owner"] {
        let mut b = as_member(&email(&format!("att {role}")), &s.tid, role);
        let r = b.get(&format!(
            "/console/inboxes/{}/messages/{mid}/attachments/{att}",
            s.iid
        ));
        assert_eq!(r.status, 200, "{role}: {}", r.body);
        assert!(
            r.header("content-disposition")
                .unwrap()
                .starts_with("attachment")
        );
        assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
    }
    let mut b = as_member(&email("att page"), &s.tid, "viewer");
    let page = html(&b.get(&format!(
        "/console/inboxes/{}/threads/{thr}?html={mid}",
        s.iid
    )));
    assert!(
        page.contains("data:image/png;base64,"),
        "the small image is inlined"
    );
    assert!(
        page.contains("Inline images not shown"),
        "the big one is a link"
    );
    assert!(page.contains(&format!(
        "/console/inboxes/{}/messages/{mid}/attachments/",
        s.iid
    )));
    let api_host = std::env::var("PM_ITEST_URL").unwrap();
    assert!(!page.contains(&api_host), "nothing from the API host");
    // Another workspace's browser gets the same 404 as a missing ID.
    let (other, _, _) = workspace("att other");
    let mut x = as_member(&email("att x"), &other, "owner");
    let foreign = x.get(&format!(
        "/console/inboxes/{}/messages/{mid}/attachments/{att}",
        s.iid
    ));
    let missing = x.get(&format!(
        "/console/inboxes/{}/messages/{mid}/attachments/att_01ARYZ6S410000000000000000",
        "idn_01ARYZ6S410000000000000000"
    ));
    assert_eq!((foreign.status, missing.status), (404, 404));
}

/// W18 (table test): each role against the console routes gets exactly the allowed outcome; another
/// workspace's IDs give `404`; a `tenant_id` in a form is ignored; only the owner's principal holds
/// `tenants:erase` (W35).
#[test]
fn w18_role_and_scope() {
    let _reset = fakes::Reset;
    let (tid, iid, _) = workspace("w18");
    let get_pages = [
        (
            "/console",
            ["owner", "admin", "member", "viewer"].as_slice(),
        ),
        ("/console/inboxes", &["owner", "admin", "member", "viewer"]),
        ("/console/members", &["owner", "admin", "member", "viewer"]),
        ("/console/domains", &["owner", "admin", "member", "viewer"]),
        (
            "/console/settings/policy",
            &["owner", "admin", "member", "viewer"],
        ),
        ("/console/quarantine", &["owner", "admin", "member"]),
        ("/console/keys", &["owner", "admin"]),
        ("/console/webhooks", &["owner", "admin"]),
        ("/console/audit", &["owner", "admin"]),
    ];
    for role in ["owner", "admin", "member", "viewer"] {
        let mut b = as_member(&email(&format!("w18 {role}")), &tid, role);
        for (path, allowed) in &get_pages {
            let r = b.get(path);
            let want = if allowed.contains(&role) { 200 } else { 403 };
            assert_eq!(r.status, want, "{role} {path}");
        }
        // A write the role may not do: 403, nothing changes.
        let r = b.submit(
            "/console/settings",
            "/console/inboxes",
            &[("username", "w18x"), ("display_name", "x")],
        );
        let want = if matches!(role, "owner" | "admin") {
            303
        } else {
            403
        };
        assert_eq!(r.status, want, "{role} create inbox: {}", r.body);
        let r = b.submit(
            "/console/settings",
            "/console/settings/delete-workspace",
            &[("confirm", "nope")],
        );
        let want = if role == "owner" { 400 } else { 403 };
        assert_eq!(r.status, want, "{role} delete workspace (W35)");
    }
    // Another workspace's identity: the same 404 as a missing one.
    let (other, _, _) = workspace("w18 other");
    let mut x = as_member(&email("w18 x"), &other, "owner");
    let foreign = x.get(&format!("/console/inboxes/{iid}"));
    let missing = x.get("/console/inboxes/idn_01ARYZ6S410000000000000000");
    assert_eq!((foreign.status, missing.status), (404, 404));
    // A tenant_id in a form is ignored: the inbox lands in the session's workspace.
    let mut o = as_member(&email("w18 owner2"), &other, "owner");
    let user = format!("w18f{}", &crate::slug("w18f")[..5]);
    let r = o.submit(
        "/console/inboxes",
        "/console/inboxes",
        &[
            ("username", &user),
            ("display_name", "F"),
            ("tenant_id", &tid),
        ],
    );
    assert_eq!(r.status, 303, "{}", r.body);
    let rows = d1(&format!(
        "SELECT tenant_id FROM identities WHERE username = '{user}'"
    ));
    assert_eq!(rows[0]["tenant_id"], other.as_str());
}

/// FR-CON-5: a sensitive action more than 10 minutes after sign-in redirects to re-authentication; a
/// code there rotates the session; the action then runs and writes an audit row with the person.
#[test]
fn reauth_sensitive() {
    let _reset = fakes::Reset;
    let (tid, iid, _) = workspace("reauth");
    let who = email("reauth");
    let mut b = session_with(&who, Some(&tid), "admin", 11 * 60_000);
    let page = format!("/console/inboxes/{iid}");
    let r = b.submit(&page, &format!("/console/inboxes/{iid}/keys"), &[]);
    assert_eq!(r.status, 303);
    assert!(
        r.header("location")
            .unwrap()
            .starts_with("/console/reauth?next="),
        "{:?}",
        r.header("location")
    );
    let old = b.cookie.clone();
    let before = fakes::mail_sent(&who).len();
    let r = b.submit("/console/reauth", "/console/reauth", &[("next", &page)]);
    assert_eq!(r.status, 200, "{}", r.body);
    let code = code_for(&who, before);
    let r = b.submit(
        "/console/reauth",
        "/console/reauth/code",
        &[("code", &code), ("next", &page)],
    );
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(r.header("location"), Some(page.as_str()));
    assert_ne!(b.cookie, old, "the session is rotated");
    let r = b.submit(&page, &format!("/console/inboxes/{iid}/keys"), &[]);
    assert_eq!(r.status, 303, "{}", r.body);
    assert!(r.header("location").unwrap().starts_with(&page));
    let rows = d1(&format!(
        "SELECT actor_user_id, actor_key_id, details_json FROM audit_log WHERE tenant_id = '{tid}' AND action = 'identity_key.create'"
    ));
    assert_eq!(rows.as_array().unwrap().len(), 1, "{rows}");
    assert!(
        rows[0]["actor_user_id"]
            .as_str()
            .unwrap()
            .starts_with("usr_")
    );
    assert!(rows[0]["actor_key_id"].is_null());
    assert!(
        rows[0]["details_json"]
            .as_str()
            .unwrap()
            .contains("\"via\":\"console\"")
    );
}

/// Sessions: 7 days rolling, 30 days absolute; sign-out and sign-out everywhere.
#[test]
fn session_lifetime() {
    let _reset = fakes::Reset;
    let (tid, _, _) = workspace("session life");
    let who = email("session life");
    let mut b = as_member(&who, &tid, "viewer");
    for _ in 0..4 {
        fakes::advance(6 * 86_400_000);
        assert_eq!(b.get("/console").status, 200, "rolling");
    }
    fakes::advance(7 * 86_400_000);
    assert_eq!(b.get("/console").status, 303, "past 30 days");
    let mut a = as_member(&who, &tid, "viewer");
    let mut c = as_member(&who, &tid, "viewer");
    let r = a.submit(
        "/console/settings",
        "/console/settings/sign-out-everywhere",
        &[],
    );
    assert_eq!(r.status, 303);
    assert_eq!(c.get("/console").status, 303, "the other session ended");
    assert_eq!(a.get("/console").status, 200);
    let r = a.submit("/console", "/console/sign-out", &[]);
    assert_eq!(r.status, 303);
    assert_eq!(a.get("/console").status, 303);
}

/// `idvy setup --owner-email` (and `POST /v1/tenants` with `owner`) emails the owner a sign-in link,
/// which signs them in.
#[test]
fn first_owner_signin() {
    let _reset = fakes::Reset;
    let who = email("first owner");
    let r = post(
        "/v1/tenants",
        &platform(),
        json!({ "slug": crate::slug("first owner"), "name": "First", "mode": "live", "owner": { "email": who, "name": "Ola" } }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let t = link_for(&who);
    let mut b = Browser::default();
    assert_eq!(b.post("/console/sign-in/link", &[("t", &t)]).status, 303);
    let home = b.get("/console");
    assert_eq!(home.status, 200);
    assert!(html(&home).contains("owner of this workspace"));
}

/// The identity page: every role sees the keys and the JWKS link; owner and admin create, rotate and
/// revoke after re-authentication, each with an `identity_key.*` audit row and event; a paused
/// identity's keys can still be revoked.
#[test]
fn identity_keys_page() {
    let _reset = fakes::Reset;
    let (tid, iid, _) = workspace("keys page");
    let page = format!("/console/inboxes/{iid}");
    let mut v = as_member(&email("keys viewer"), &tid, "viewer");
    let r = v.get(&page);
    assert!(html(&r).contains(&format!("/.well-known/jwks/{iid}.json")));
    assert_eq!(v.submit(&page, &format!("{page}/keys"), &[]).status, 403);
    let mut a = as_member(&email("keys admin"), &tid, "admin");
    assert_eq!(a.submit(&page, &format!("{page}/keys"), &[]).status, 303);
    assert_eq!(
        a.submit(&page, &format!("{page}/keys/rotate"), &[]).status,
        303
    );
    let keys = crate::get(&format!("/v1/identities/{iid}/keys"), &platform());
    let active = keys.body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["status"] == "active")
        .unwrap()["kid"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = crate::patch(
        &format!("/v1/identities/{iid}"),
        &platform(),
        json!({ "status": "paused" }),
    );
    assert_eq!(r.status, 200);
    assert_eq!(
        a.submit(&page, &format!("{page}/keys/{active}/revoke"), &[])
            .status,
        303
    );
    let acts: Vec<String> = d1(&format!(
        "SELECT action FROM audit_log WHERE tenant_id = '{tid}' AND action LIKE 'identity_key.%' ORDER BY created_at"
    ))
    .as_array()
    .unwrap()
    .iter()
    .map(|r| r["action"].as_str().unwrap().to_owned())
    .collect();
    assert_eq!(
        acts,
        [
            "identity_key.create",
            "identity_key.rotate",
            "identity_key.revoke"
        ]
    );
    for e in [
        "identity.key_created",
        "identity.key_rotated",
        "identity.key_revoked",
    ] {
        assert_eq!(crate::send::events_of(&iid, e).len(), 1, "{e}");
    }
}

/// `it::console::policy_page`: every role sees the page; only owner and admin get the form; a save
/// sends only changed fields with an audit row naming the person; above a ceiling the field's refusal
/// is shown; a retention decrease asks for confirmation first.
#[test]
fn policy_page() {
    let _reset = fakes::Reset;
    let (tid, _, _) = workspace("policy page");
    for role in ["member", "viewer"] {
        let mut b = as_member(&email(&format!("policy {role}")), &tid, role);
        let r = b.get("/console/settings/policy");
        assert_eq!(r.status, 200);
        assert!(!html(&r).contains("Save the policy"), "{role}");
        assert_eq!(
            b.submit("/console/settings/policy", "/console/settings/policy", &[])
                .status,
            403
        );
    }
    let mut o = as_member(&email("policy owner"), &tid, "owner");
    let page = html(&o.get("/console/settings/policy"));
    assert!(page.contains("Save the policy"));
    let version = page
        .split(r#"name="policy_version" value=""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    let r = o.submit(
        "/console/settings/policy",
        "/console/settings/policy",
        &[
            ("policy_version", &version),
            ("tenant_daily_send_cap", "50"),
        ],
    );
    assert_eq!(r.status, 303, "{}", r.body);
    let rows = d1(&format!(
        "SELECT actor_user_id, details_json FROM audit_log WHERE tenant_id = '{tid}' AND action = 'tenant.policy_update'"
    ));
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert!(
        rows[0]["details_json"]
            .as_str()
            .unwrap()
            .contains("tenant_daily_send_cap")
    );
    assert!(
        rows[0]["actor_user_id"]
            .as_str()
            .unwrap()
            .starts_with("usr_")
    );
    // Above the deployment's ceiling: refused, shown with the field.
    let page = html(&o.get("/console/settings/policy"));
    let version = page
        .split(r#"name="policy_version" value=""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    let r = o.submit(
        "/console/settings/policy",
        "/console/settings/policy",
        &[
            ("policy_version", &version),
            ("tenant_daily_send_cap", "99999999"),
        ],
    );
    assert_eq!(r.status, 403, "{}", r.body);
    assert!(html(&r).contains("tenant_daily_send_cap"));
    // A retention decrease: a confirmation page first, nothing written.
    let r = o.submit(
        "/console/settings/policy",
        "/console/settings/policy",
        &[("policy_version", &version), ("retention.raw_days", "1")],
    );
    assert_eq!(r.status, 200);
    assert!(html(&r).contains("confirm_deletion"));
    assert_eq!(
        d1(&format!(
            "SELECT COUNT(*) AS n FROM audit_log WHERE tenant_id = '{tid}' AND action = 'tenant.policy_update'"
        ))[0]["n"],
        1
    );
    let r = o.submit(
        "/console/settings/policy",
        "/console/settings/policy",
        &[
            ("policy_version", &version),
            ("retention.raw_days", "1"),
            ("confirm_deletion", "yes"),
        ],
    );
    assert_eq!(r.status, 303, "{}", r.body);
}

/// `PM_CONSOLE=off` removes every console route except the invitation-accept pair; the members API
/// keeps working.
#[test]
fn disabled() {
    let _reset = fakes::Reset;
    let (tid, _, _) = workspace("console off");
    fakes::set_config("PM_CONSOLE", Some("off"));
    let mut b = Browser::default();
    let r = b.get("/console/sign-in");
    assert_eq!(r.status, 404);
    assert!(html(&r).contains("route_not_found"));
    let invited = email("console off invited");
    let r = post(
        &format!("/v1/tenants/{tid}/invitations"),
        &platform(),
        json!({ "email": invited, "role": "member" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    let t = link_for(&invited);
    assert_eq!(
        b.get(&format!("/console/invitations/accept?t={t}")).status,
        200
    );
    let r = b.post("/console/invitations/accept", &[("t", &t)]);
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(b.cookie.is_none(), "no session with the console off");
    let m = crate::get(&format!("/v1/tenants/{tid}/members"), &platform());
    assert!(
        m.body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["email"] == invited.as_str())
    );
}

/// NFR-CON-1: server render time p95 ≤ 300 ms on every page of a fixture workspace.
#[test]
fn render_budget() {
    let _reset = fakes::Reset;
    let (tid, iid, _) = workspace("render budget");
    let mut b = as_member(&email("render owner"), &tid, "owner");
    let pages = [
        "/console".to_owned(),
        "/console/inboxes".into(),
        format!("/console/inboxes/{iid}"),
        "/console/members".into(),
        "/console/keys".into(),
        "/console/domains".into(),
        "/console/webhooks".into(),
        "/console/audit".into(),
        "/console/settings".into(),
        "/console/settings/policy".into(),
        "/console/quarantine".into(),
        "/console/workspaces".into(),
    ];
    for p in &pages {
        let mut times = Vec::new();
        for _ in 0..10 {
            let t = std::time::Instant::now();
            let r = b.get(p);
            times.push(t.elapsed().as_millis());
            assert_eq!(r.status, 200, "{p}");
        }
        times.sort_unstable();
        let p95 = times[(times.len() * 95).div_ceil(100) - 1];
        assert!(p95 <= 300, "{p}: p95 {p95} ms");
    }
    let _ = call;
}
