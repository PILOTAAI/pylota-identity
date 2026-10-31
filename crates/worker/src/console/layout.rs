//! The console's page frame and form helpers (`maud`): HTML and one stylesheet, no script, no font and
//! no request to another origin (FR-CON-1). Every form posts with the session's CSRF token.

use maud::{DOCTYPE, Markup, PreEscaped, html};

/// What a page knows about the person viewing it.
pub struct Frame<'a> {
    pub email: &'a str,
    pub workspace: Option<&'a str>,
    pub test_mode: bool,
    pub permissions: &'a [String],
    pub csrf: &'a str,
}

impl Frame<'_> {
    pub fn can(&self, perm: &str) -> bool {
        self.permissions.iter().any(|p| p == perm)
    }
}

pub const CSS: &str = r#":root{color-scheme:light;--fg:#1a1a1a;--muted:#4a4a4a;--line:#c8c8c8;--accent:#0b4f9c;--bad:#a1121b;--ok:#1b5e20;--bg:#ffffff;--soft:#f3f4f6}
*{box-sizing:border-box}
body{margin:0;font:16px/1.5 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;color:var(--fg);background:var(--bg)}
a{color:var(--accent)}
a:focus,button:focus,input:focus,select:focus,textarea:focus{outline:3px solid #1f6feb;outline-offset:2px}
.skip{position:absolute;left:-999px}.skip:focus{left:8px;top:8px;background:var(--bg);padding:4px}
header.top{display:flex;flex-wrap:wrap;gap:1rem;align-items:center;justify-content:space-between;padding:.75rem 1.25rem;border-bottom:1px solid var(--line);background:var(--soft)}
header.top .brand{font-weight:700;color:var(--fg);text-decoration:none}
nav.main ul{list-style:none;margin:0;padding:0;display:flex;flex-wrap:wrap;gap:.25rem 1rem}
main{max-width:72rem;margin:0 auto;padding:1.25rem}
h1{font-size:1.6rem;margin:.25rem 0 1rem}h2{font-size:1.25rem;margin:1.5rem 0 .5rem}
table{border-collapse:collapse;width:100%;margin:.5rem 0 1rem}
th,td{border-bottom:1px solid var(--line);padding:.4rem .5rem;text-align:left;vertical-align:top}
th{background:var(--soft)}
form.inline{display:inline}
fieldset{border:1px solid var(--line);padding:.75rem 1rem;margin:0 0 1rem}
label{display:block;font-weight:600;margin:.5rem 0 .2rem}
input[type=text],input[type=email],input[type=number],input[type=search],select,textarea{font:inherit;padding:.35rem .5rem;border:1px solid #767676;border-radius:3px;max-width:100%;width:28rem}
textarea{height:8rem}
button{font:inherit;padding:.35rem .9rem;border:1px solid var(--accent);background:var(--accent);color:#fff;border-radius:3px;cursor:pointer}
button.secondary{background:var(--bg);color:var(--accent)}
button.danger{background:var(--bad);border-color:var(--bad)}
.notice{border-left:4px solid var(--accent);background:var(--soft);padding:.5rem .75rem;margin:0 0 1rem}
.error{border-left:4px solid var(--bad);background:#fdecee;padding:.5rem .75rem;margin:0 0 1rem}
.badge{display:inline-block;border:1px solid var(--muted);border-radius:3px;padding:0 .35rem;font-size:.85rem}
.muted{color:var(--muted)}
pre.mail{white-space:pre-wrap;word-break:break-word;background:var(--soft);padding:.75rem;border:1px solid var(--line)}
iframe.mail{width:100%;height:28rem;border:1px solid var(--line)}
code{word-break:break-all}
"#;

/// A full page: frame, navigation for what the person may see, and `body`.
pub fn page(title: &str, frame: Option<&Frame<'_>>, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " · Pylota Identity" }
                link rel="stylesheet" href="/console/assets/console.css";
            }
            body {
                a class="skip" href="#content" { "Skip to content" }
                header class="top" {
                    a class="brand" href="/console" { "Pylota Identity" }
                    @if let Some(f) = frame {
                        (nav(f))
                        div {
                            @if let Some(w) = f.workspace {
                                span { (w) }
                                @if f.test_mode { " " span class="badge" { "Test" } }
                                " · "
                            }
                            a href="/console/workspaces" { "Workspaces" }
                            " · "
                            a href="/console/settings" { (f.email) }
                            " "
                            (post_form("/console/sign-out", f.csrf, "inline", html! {
                                button class="secondary" type="submit" { "Sign out" }
                            }))
                        }
                    }
                }
                main id="content" {
                    h1 { (title) }
                    (body)
                }
            }
        }
    }
}

fn nav(f: &Frame<'_>) -> Markup {
    let items: [(&str, &str, &str); 11] = [
        ("/console", "Home", "identities:read"),
        ("/console/inboxes", "Inboxes", "identities:read"),
        ("/console/search", "Search", "search:read"),
        ("/console/quarantine", "Quarantine", "quarantine:review"),
        ("/console/domains", "Domains", "domains:read"),
        ("/console/keys", "API keys", "keys:manage"),
        ("/console/webhooks", "Webhooks", "webhooks:manage"),
        ("/console/members", "Members", "members:read"),
        ("/console/audit", "Audit log", "audit:read"),
        ("/console/settings/policy", "Policy", "members:read"),
        ("/console/settings", "Settings", "members:read"),
    ];
    html! {
        nav class="main" aria-label="Console" {
            ul {
                @if f.workspace.is_some() {
                    @for (href, label, perm) in items {
                        @if f.can(perm) { li { a href=(href) { (label) } } }
                    }
                }
            }
        }
    }
}

/// A `POST` form carrying the CSRF token.
pub fn post_form(action: &str, csrf: &str, class: &str, inner: Markup) -> Markup {
    html! {
        form method="post" action=(action) class=(class) {
            input type="hidden" name="_csrf" value=(csrf);
            (inner)
        }
    }
}

/// A form used before a session exists (checked by `Origin` alone).
pub fn public_form(action: &str, inner: Markup) -> Markup {
    html! { form method="post" action=(action) { (inner) } }
}

/// A one-button form.
pub fn button(action: &str, csrf: &str, label: &str, danger: bool) -> Markup {
    post_form(
        action,
        csrf,
        "inline",
        html! { button type="submit" class=(if danger { "danger" } else { "" }) { (label) } },
    )
}

pub fn notice(text: &str) -> Markup {
    html! { p class="notice" role="status" { (text) } }
}

pub fn error(text: &str) -> Markup {
    html! { p class="error" role="alert" { (text) } }
}

/// A labelled text input.
pub fn field(name: &str, label: &str, kind: &str, value: &str, required: bool) -> Markup {
    let id = format!("f-{name}");
    html! {
        label for=(id) { (label) }
        input type=(kind) id=(id) name=(name) value=(value) required[required];
    }
}

/// A labelled select.
pub fn select(name: &str, label: &str, options: &[(&str, &str)], selected: &str) -> Markup {
    let id = format!("f-{name}");
    html! {
        label for=(id) { (label) }
        select id=(id) name=(name) {
            @for (v, l) in options {
                option value=(v) selected[*v == selected] { (l) }
            }
        }
    }
}

/// A value of an API object as text: strings as they are, `null` as a dash, the rest as JSON.
pub fn text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "–".into(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// HTML that is already safe (the sanitised mail of a `srcdoc`, escaped by the caller).
pub fn raw(s: String) -> PreEscaped<String> {
    PreEscaped(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Text is escaped; there is no script element or attribute in the frame.
    #[test]
    fn escapes_and_has_no_script() {
        let f = Frame {
            email: "ola@x.example",
            workspace: Some("<b>W</b>"),
            test_mode: true,
            permissions: &["identities:read".to_owned()],
            csrf: "tok",
        };
        let out = page(
            "T<script>",
            Some(&f),
            html! { p { "<img src=x onerror=alert(1)>" } },
        )
        .into_string();
        assert!(!out.contains("<script"), "{out}");
        assert!(!out.contains("<img src=x"));
        assert!(out.contains("&lt;b&gt;W&lt;/b&gt;"));
        assert!(out.contains(r#"name="_csrf" value="tok""#));
        assert!(out.contains(r#"<html lang="en">"#));
    }
}
