//! The console's deny-by-default route table ([Console › Roles]): every route is registered with its
//! method, path, the permission its role check needs, how it authenticates, its CSRF rule and whether it
//! is sensitive. A path no entry matches is a `404` page.
//!
//! [Console › Roles]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/console.md#roles

/// How a route authenticates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Auth {
    /// No session (sign-in, invitation acceptance, the stylesheet).
    Public,
    /// A signed-in person, with or without an active workspace.
    Person,
    /// A signed-in person acting in their active workspace, holding `perm`.
    Workspace,
}

/// The CSRF rule of a `POST` (Console › CSRF).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Csrf {
    /// `GET`: none.
    None,
    /// Forms used before a session exists: `Origin` alone.
    Origin,
    /// The session's token and `Origin`.
    Session,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Css,
    SignIn,
    SignInRequest,
    SignInLink,
    SignInLinkUse,
    SignInCode,
    SignOut,
    InvitationAccept,
    InvitationAcceptPost,
    Invitations,
    InvitationAcceptMine,
    Reauth,
    ReauthSend,
    ReauthCode,
    Workspaces,
    WorkspaceSwitch,
    Home,
    Inboxes,
    InboxCreate,
    Inbox,
    InboxStatus,
    KeyCreateIdentity,
    KeyRotateIdentity,
    KeyRevokeIdentity,
    Thread,
    Attachment,
    Search,
    Quarantine,
    QuarantineRelease,
    Keys,
    KeyCreate,
    KeyRevoke,
    Domains,
    Domain,
    DomainCreate,
    DomainVerify,
    DomainDelete,
    Webhooks,
    WebhookCreate,
    WebhookDelete,
    Members,
    MemberInvite,
    InvitationRevoke,
    MemberRole,
    MemberRemove,
    OwnershipTransfer,
    Leave,
    Audit,
    Settings,
    SettingsProfile,
    SettingsWorkspace,
    SignOutEverywhere,
    DeleteWorkspace,
    Policy,
    PolicySave,
}

#[derive(Clone, Copy, Debug)]
pub struct Route {
    pub method: &'static str,
    pub pattern: &'static str,
    pub page: Page,
    pub auth: Auth,
    /// The permission the role check needs (`Auth::Workspace`).
    pub perm: Option<&'static str>,
    pub csrf: Csrf,
    /// Needs a sign-in within the last 10 minutes (Console › Re-authentication).
    pub sensitive: bool,
    /// Served with `PM_CONSOLE=off` (the invitation-accept pair).
    pub when_off: bool,
}

const fn get(pattern: &'static str, page: Page, auth: Auth, perm: Option<&'static str>) -> Route {
    Route {
        method: "GET",
        pattern,
        page,
        auth,
        perm,
        csrf: Csrf::None,
        sensitive: false,
        when_off: false,
    }
}

const fn post(
    pattern: &'static str,
    page: Page,
    auth: Auth,
    perm: Option<&'static str>,
    sensitive: bool,
) -> Route {
    Route {
        method: "POST",
        pattern,
        page,
        auth,
        perm,
        csrf: match auth {
            Auth::Public => Csrf::Origin,
            _ => Csrf::Session,
        },
        sensitive,
        when_off: false,
    }
}

const fn off(mut r: Route) -> Route {
    r.when_off = true;
    r
}

use Auth::{Person, Public, Workspace as W};

pub static ROUTES: &[Route] = &[
    get("/console/assets/console.css", Page::Css, Public, None),
    get("/console/sign-in", Page::SignIn, Public, None),
    post("/console/sign-in", Page::SignInRequest, Public, None, false),
    get("/console/sign-in/link", Page::SignInLink, Public, None),
    post(
        "/console/sign-in/link",
        Page::SignInLinkUse,
        Public,
        None,
        false,
    ),
    post(
        "/console/sign-in/code",
        Page::SignInCode,
        Public,
        None,
        false,
    ),
    off(get(
        "/console/invitations/accept",
        Page::InvitationAccept,
        Public,
        None,
    )),
    off(post(
        "/console/invitations/accept",
        Page::InvitationAcceptPost,
        Public,
        None,
        false,
    )),
    post("/console/sign-out", Page::SignOut, Person, None, false),
    get("/console/invitations", Page::Invitations, Person, None),
    post(
        "/console/invitations/{invitation_id}/accept",
        Page::InvitationAcceptMine,
        Person,
        None,
        false,
    ),
    get("/console/reauth", Page::Reauth, Person, None),
    post("/console/reauth", Page::ReauthSend, Person, None, false),
    post(
        "/console/reauth/code",
        Page::ReauthCode,
        Person,
        None,
        false,
    ),
    get("/console/workspaces", Page::Workspaces, Person, None),
    post(
        "/console/workspaces/switch",
        Page::WorkspaceSwitch,
        Person,
        None,
        false,
    ),
    get("/console/settings", Page::Settings, Person, None),
    post(
        "/console/settings/profile",
        Page::SettingsProfile,
        Person,
        None,
        false,
    ),
    post(
        "/console/settings/sign-out-everywhere",
        Page::SignOutEverywhere,
        Person,
        None,
        false,
    ),
    post(
        "/console/settings/workspace",
        Page::SettingsWorkspace,
        W,
        Some("tenants:erase"),
        true,
    ),
    post(
        "/console/settings/delete-workspace",
        Page::DeleteWorkspace,
        W,
        Some("tenants:erase"),
        true,
    ),
    get("/console", Page::Home, W, Some("identities:read")),
    get(
        "/console/inboxes",
        Page::Inboxes,
        W,
        Some("identities:read"),
    ),
    post(
        "/console/inboxes",
        Page::InboxCreate,
        W,
        Some("identities:write"),
        false,
    ),
    get(
        "/console/inboxes/{identity_id}",
        Page::Inbox,
        W,
        Some("identities:read"),
    ),
    post(
        "/console/inboxes/{identity_id}/status",
        Page::InboxStatus,
        W,
        Some("identities:write"),
        false,
    ),
    post(
        "/console/inboxes/{identity_id}/keys",
        Page::KeyCreateIdentity,
        W,
        Some("identities:write"),
        true,
    ),
    post(
        "/console/inboxes/{identity_id}/keys/rotate",
        Page::KeyRotateIdentity,
        W,
        Some("identities:write"),
        true,
    ),
    post(
        "/console/inboxes/{identity_id}/keys/{kid}/revoke",
        Page::KeyRevokeIdentity,
        W,
        Some("identities:write"),
        true,
    ),
    get(
        "/console/inboxes/{identity_id}/threads/{thread_id}",
        Page::Thread,
        W,
        Some("messages:read"),
    ),
    get(
        "/console/inboxes/{identity_id}/messages/{message_id}/attachments/{attachment_id}",
        Page::Attachment,
        W,
        Some("attachments:read"),
    ),
    get("/console/search", Page::Search, W, Some("search:read")),
    get(
        "/console/quarantine",
        Page::Quarantine,
        W,
        Some("quarantine:review"),
    ),
    post(
        "/console/quarantine/{identity_id}/{message_id}/release",
        Page::QuarantineRelease,
        W,
        Some("quarantine:review"),
        true,
    ),
    get("/console/keys", Page::Keys, W, Some("keys:manage")),
    post(
        "/console/keys",
        Page::KeyCreate,
        W,
        Some("keys:manage"),
        true,
    ),
    post(
        "/console/keys/{key_id}/revoke",
        Page::KeyRevoke,
        W,
        Some("keys:manage"),
        false,
    ),
    get("/console/domains", Page::Domains, W, Some("domains:read")),
    post(
        "/console/domains",
        Page::DomainCreate,
        W,
        Some("domains:write"),
        true,
    ),
    get(
        "/console/domains/{domain_id}",
        Page::Domain,
        W,
        Some("domains:read"),
    ),
    post(
        "/console/domains/{domain_id}/verify",
        Page::DomainVerify,
        W,
        Some("domains:write"),
        false,
    ),
    post(
        "/console/domains/{domain_id}/delete",
        Page::DomainDelete,
        W,
        Some("domains:write"),
        true,
    ),
    get(
        "/console/webhooks",
        Page::Webhooks,
        W,
        Some("webhooks:manage"),
    ),
    post(
        "/console/webhooks",
        Page::WebhookCreate,
        W,
        Some("webhooks:manage"),
        false,
    ),
    post(
        "/console/webhooks/{webhook_id}/delete",
        Page::WebhookDelete,
        W,
        Some("webhooks:manage"),
        false,
    ),
    get("/console/members", Page::Members, W, Some("members:read")),
    post(
        "/console/members/invite",
        Page::MemberInvite,
        W,
        Some("members:manage"),
        true,
    ),
    post(
        "/console/members/invitations/{invitation_id}/revoke",
        Page::InvitationRevoke,
        W,
        Some("members:manage"),
        false,
    ),
    post(
        "/console/members/{user_id}/role",
        Page::MemberRole,
        W,
        Some("members:manage"),
        true,
    ),
    post(
        "/console/members/{user_id}/remove",
        Page::MemberRemove,
        W,
        Some("members:manage"),
        true,
    ),
    post(
        "/console/members/transfer",
        Page::OwnershipTransfer,
        W,
        Some("tenants:erase"),
        true,
    ),
    post(
        "/console/members/leave",
        Page::Leave,
        W,
        Some("members:read"),
        false,
    ),
    get("/console/audit", Page::Audit, W, Some("audit:read")),
    get(
        "/console/settings/policy",
        Page::Policy,
        W,
        Some("members:read"),
    ),
    post(
        "/console/settings/policy",
        Page::PolicySave,
        W,
        Some("policy:write"),
        true,
    ),
];

/// A matched route and its path parameters.
pub struct Matched {
    pub route: &'static Route,
    pub params: Vec<(&'static str, String)>,
}

impl Matched {
    pub fn param(&self, name: &str) -> &str {
        self.params
            .iter()
            .find(|(n, _)| *n == name)
            .map_or("", |(_, v)| v.as_str())
    }
}

fn matches(pattern: &'static str, path: &str) -> Option<Vec<(&'static str, String)>> {
    let ps: Vec<&str> = pattern.split('/').collect();
    let xs: Vec<&str> = path.split('/').collect();
    if ps.len() != xs.len() {
        return None;
    }
    let mut out = Vec::new();
    for (p, x) in ps.into_iter().zip(xs) {
        if let Some(name) = p.strip_prefix('{').and_then(|n| n.strip_suffix('}')) {
            if x.is_empty()
                || !x
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return None;
            }
            out.push((name, x.to_owned()));
        } else if p != x {
            return None;
        }
    }
    Some(out)
}

/// The route for a method and path: `Err(true)` when only other methods match (405), `Err(false)`
/// when nothing does (404).
pub fn find(method: &str, path: &str) -> Result<Matched, bool> {
    let path = if path.len() > 1 {
        path.trim_end_matches('/')
    } else {
        path
    };
    let mut other = false;
    for r in ROUTES {
        if let Some(params) = matches(r.pattern, path) {
            if r.method == method || (method == "HEAD" && r.method == "GET") {
                return Ok(Matched { route: r, params });
            }
            other = true;
        }
    }
    Err(other)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every write is a `POST` with a CSRF rule; a sensitive route is a write; every workspace route
    /// names its permission; only the invitation-accept pair is served with the console off.
    #[test]
    fn table_rules() {
        for r in ROUTES {
            assert_eq!(r.method == "POST", r.csrf != Csrf::None, "{}", r.pattern);
            assert!(!r.sensitive || r.method == "POST", "{}", r.pattern);
            assert_eq!(r.auth == Auth::Workspace, r.perm.is_some(), "{}", r.pattern);
            assert_eq!(
                r.when_off,
                r.pattern == "/console/invitations/accept",
                "{}",
                r.pattern
            );
        }
        let m = find("GET", "/console/inboxes/idn_01ARYZ6S410000000000000000").unwrap();
        assert_eq!(m.route.page, Page::Inbox);
        assert_eq!(m.param("identity_id"), "idn_01ARYZ6S410000000000000000");
        assert!(matches!(find("PUT", "/console/inboxes"), Err(true)));
        assert!(matches!(find("GET", "/console/nope"), Err(false)));
        assert!(matches!(find("GET", "/console/inboxes/a%2Fb"), Err(false)));
    }
}
