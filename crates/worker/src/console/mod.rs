//! The console at `/console` ([Console and workspaces]): server-rendered pages for the people who run
//! the agents, on the same services as the API. Every request runs through the console's route table:
//! `Origin` on every `POST`, the session (one D1 query), the role's permission, the CSRF token and, for
//! a sensitive action, a recent sign-in. Actions call the REST routes through [`App::serve_as`] with the
//! session's principal, so validation, errors, idempotency and audit are the API's.
//!
//! [Console and workspaces]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md

pub mod layout;
pub mod pages;
pub mod router;
pub mod session;
pub mod signin;

use maud::Markup;
use pylota_identity_api_types::ErrorCode;
use pylota_identity_core::console::{self as cc, Role};
use pylota_identity_platform::app::HttpRequestIn;
use pylota_identity_platform::ratelimit::RlBucket;
use pylota_identity_platform::{ControlDb as _, Platform};
use serde_json::Value;

use crate::app::{App, Call};
use crate::auth::KeyScope;
use crate::ctx::Reply;
use crate::db::tenants as tdb;
use crate::errors::ApiError;
use router::{Auth, Csrf, Matched, Page};
use session::Session;

/// The response headers of every console page (Console › CSRF).
pub const CSP: &str = "default-src 'none'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

/// One console request.
pub struct Cx<'a, P: Platform> {
    pub app: &'a App<P>,
    pub request_id: &'a str,
    pub now: i64,
    pub m: Matched,
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub form: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub ip: String,
    pub session: Option<Session>,
    pub tenant: Option<tdb::Row>,
    pub principal: Option<KeyScope>,
}

impl<P: Platform> Cx<'_, P> {
    pub fn p(&self) -> &P {
        self.app.p.as_ref()
    }

    pub fn q(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// A form field (`application/x-www-form-urlencoded`).
    pub fn f(&self, name: &str) -> Option<&str> {
        self.form
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Every value of a repeated form field.
    pub fn fs(&self, name: &str) -> Vec<&str> {
        self.form
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn param(&self, name: &str) -> &str {
        self.m.param(name)
    }

    pub fn session(&self) -> &Session {
        self.session.as_ref().expect("a signed-in route")
    }

    pub fn tenant(&self) -> &tdb::Row {
        self.tenant.as_ref().expect("a workspace route")
    }

    pub fn role(&self) -> Role {
        self.session().role().unwrap_or(Role::Viewer)
    }

    pub fn csrf(&self) -> String {
        self.session
            .as_ref()
            .map(|s| cc::csrf_token(&s.csrf_secret))
            .unwrap_or_default()
    }

    pub fn can(&self, perm: &str) -> bool {
        self.principal.as_ref().is_some_and(|k| k.has(perm))
    }

    /// The page frame for the signed-in person.
    pub fn frame<'b>(&'b self, csrf: &'b str) -> Option<layout::Frame<'b>> {
        let s = self.session.as_ref()?;
        Some(layout::Frame {
            email: &s.email,
            workspace: self.tenant.as_ref().map(|t| t.name.as_str()),
            test_mode: self.tenant.as_ref().is_some_and(|t| t.mode == "test"),
            permissions: self
                .principal
                .as_ref()
                .map_or(&[][..], |k| k.permissions.as_slice()),
            csrf,
        })
    }

    /// One REST call as the session's principal; a `204` answers `null`.
    pub async fn api(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<Value, ApiError> {
        let key = self
            .principal
            .clone()
            .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated, "Sign in first."))?;
        let mut headers = vec![("content-type".to_owned(), "application/json".to_owned())];
        if let Some(ip) = self.header("cf-connecting-ip") {
            headers.push(("cf-connecting-ip".into(), ip.to_owned()));
        }
        let call = Call {
            method: method.to_owned(),
            path: path.to_owned(),
            query: query
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
            headers,
            body: body.map(|b| b.to_string().into_bytes()),
        };
        let r = self.app.serve_as(key, call, self.request_id).await?;
        Ok(r.body.unwrap_or(Value::Null))
    }

    /// A page for the signed-in person.
    pub fn page(&self, title: &str, body: Markup) -> Reply {
        let csrf = self.csrf();
        html(200, layout::page(title, self.frame(&csrf).as_ref(), body))
    }
}

/// An HTML reply.
pub fn html(status: u16, m: Markup) -> Reply {
    Reply {
        status,
        body: None,
        raw: Some(("text/html; charset=utf-8", m.into_string().into_bytes())),
        headers: Vec::new(),
        stream: None,
    }
}

/// `303 See Other` to `location`.
pub fn see_other(location: &str) -> Reply {
    Reply {
        status: 303,
        body: None,
        raw: Some(("text/plain; charset=utf-8", Vec::new())),
        headers: vec![("location".into(), location.to_owned())],
        stream: None,
    }
}

/// An error as a console page: the API's status, message and fix.
pub fn error_page<P: Platform>(cx: Option<&Cx<'_, P>>, e: &ApiError) -> Reply {
    let status = e.code.http_status();
    let fix = e.fix.clone().unwrap_or_default();
    let body = maud::html! {
        (layout::error(&e.message))
        @if !fix.is_empty() { p { (fix) } }
        @if let Some(d) = e.details.as_ref() {
            @if let Some(errs) = d.get("errors").and_then(Value::as_array) {
                ul { @for x in errs { li { (layout::text(&x["path"])) ": " (layout::text(&x["message"])) } } }
            }
        }
        p class="muted" { "Code: " code { (e.code.as_str()) } }
    };
    let title = match status {
        403 => "Not allowed",
        404 => "Not found",
        409 => "Conflict",
        429 => "Too many requests",
        _ => "Something went wrong",
    };
    let mut r = match cx {
        Some(cx) => cx.page(title, body),
        None => html(200, layout::page(title, None, body)),
    };
    r.status = status;
    r
}

/// Decodes `application/x-www-form-urlencoded`.
pub fn form_decode(body: &[u8]) -> Vec<(String, String)> {
    let s = String::from_utf8_lossy(body);
    s.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (pct(k), pct(v))
        })
        .collect()
}

fn pct(s: &str) -> String {
    let s = s.replace('+', " ");
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn forbidden(message: &str) -> ApiError {
    ApiError::new(ErrorCode::PermissionDenied, message)
}

/// Serves one console request; `None` when the path is not the console's.
pub async fn serve<P: Platform>(
    app: &App<P>,
    req: &HttpRequestIn,
    request_id: &str,
    path: &str,
    query: Vec<(String, String)>,
) -> Reply {
    use pylota_identity_platform::Clock as _;
    let p = app.p.as_ref();
    let cfg = p.config();
    let now = p.clock().now_ms();
    let m = match router::find(&req.method, path) {
        Ok(m) => m,
        Err(other) => {
            let e = if other {
                ApiError::new(
                    ErrorCode::MethodNotAllowed,
                    "This page does not take that method.",
                )
            } else {
                ApiError::new(ErrorCode::RouteNotFound, "No page here.")
            };
            return error_page::<P>(None, &e);
        }
    };
    // With the console off, only the invitation-accept pair answers; the rest is the API's 404.
    if cfg.console.is_none() && !m.route.when_off {
        let mut r = Reply::json(
            404,
            crate::errors::envelope(
                &ApiError::new(ErrorCode::RouteNotFound, "No route matches this path."),
                request_id,
                &cfg.docs_url,
            ),
        );
        r.status = 404;
        return r;
    }
    let body = req.body.as_ref().map(|b| b.to_vec()).unwrap_or_default();
    let mut cx = Cx {
        app,
        request_id,
        now,
        m,
        method: req.method.clone(),
        path: path.to_owned(),
        query,
        form: if req.method == "POST" {
            form_decode(&body)
        } else {
            Vec::new()
        },
        headers: req.headers.clone(),
        ip: req
            .cf_connecting_ip
            .clone()
            .or_else(|| req.header("cf-connecting-ip").map(str::to_owned))
            .unwrap_or_default(),
        session: None,
        tenant: None,
        principal: None,
    };
    match gate(&mut cx).await {
        Ok(Some(early)) => early,
        Ok(None) => match pages::dispatch(&cx).await {
            Ok(r) => r,
            Err(e) => error_page(Some(&cx), &e),
        },
        Err(e) => error_page(cx.session.as_ref().map(|_| &cx), &e),
    }
}

/// Origin, session, role, CSRF and re-authentication, in that order. `Ok(Some(_))` answers early
/// (a redirect).
async fn gate<P: Platform>(cx: &mut Cx<'_, P>) -> Result<Option<Reply>, ApiError> {
    let route = cx.m.route;
    let p = cx.app.p.as_ref();
    if route.method == "POST" {
        let want = format!("https://{}", p.config().console_host);
        if cx.header("origin") != Some(want.as_str()) {
            return Err(forbidden("The form did not come from this console."));
        }
    }
    if matches!(
        route.page,
        Page::SignInRequest | Page::SignInLinkUse | Page::SignInCode
    ) {
        crate::ratelimit::check(p, RlBucket::SignIn, &cc::client_network(&cx.ip)).await?;
    }
    if route.auth == Auth::Public {
        // A public page still knows a signed-in person (invitation acceptance, the sign-in page).
        if let Some(v) = session::cookie_value(cx.header("cookie")) {
            cx.session = session::load(p, &v, cx.now).await?;
        }
        return Ok(None);
    }
    let Some(value) = session::cookie_value(cx.header("cookie")) else {
        return Ok(Some(to_sign_in(cx)));
    };
    let Some(s) = session::load(p, &value, cx.now).await? else {
        return Ok(Some(to_sign_in(cx)));
    };
    cx.session = Some(s.clone());
    if let (Some(tid), Some(role)) = (s.tenant_id.as_deref(), s.role()) {
        let t: Option<tdb::Row> = p.db().first(tdb::get(tid)).await?;
        if let Some(t) = t.filter(|t| matches!(t.status.as_str(), "active" | "suspended")) {
            cx.principal = Some(crate::members::principal(&s.user_id, &s.email, &t, role));
            cx.tenant = Some(t);
        }
    }
    if route.csrf == Csrf::Session {
        let ok = cx
            .f("_csrf")
            .is_some_and(|t| cc::same(t, &cc::csrf_token(&s.csrf_secret)));
        if !ok {
            return Err(forbidden("The form has expired; open the page again."));
        }
    }
    if route.auth == Auth::Workspace {
        // No membership in the active workspace (removed, W9): the workspace picker.
        if cx.tenant.is_none() {
            return Ok(Some(see_other("/console/workspaces")));
        }
        if let Some(perm) = route.perm
            && !cx.can(perm)
        {
            return Err(ApiError::permission_denied(&[perm]));
        }
    }
    if route.sensitive && !cc::recently_signed_in(s.authenticated_at, cx.now) {
        let next = cx.header("referer").and_then(path_of).unwrap_or("/console");
        return Ok(Some(see_other(&format!(
            "/console/reauth?next={}",
            urlencode(cc::safe_next(next).unwrap_or("/console"))
        ))));
    }
    Ok(None)
}

fn path_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.find('/').map(|i| &rest[i..])
}

fn to_sign_in<P: Platform>(cx: &Cx<'_, P>) -> Reply {
    if cx.method == "GET" {
        see_other(&format!("/console/sign-in?next={}", urlencode(&cx.path)))
    } else {
        see_other("/console/sign-in")
    }
}

/// Percent-encodes a query value.
pub fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms_and_encoding() {
        assert_eq!(
            form_decode(b"a=1&b=x+y%2Fz&c"),
            [
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "x y/z".to_owned()),
                ("c".to_owned(), String::new())
            ]
        );
        assert_eq!(urlencode("/console/keys?x=1"), "/console/keys%3Fx%3D1");
        assert_eq!(
            path_of("https://h.example/console/keys?a"),
            Some("/console/keys?a")
        );
    }
}
