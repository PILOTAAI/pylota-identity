//! Which endpoints receive an event ([Webhooks › Endpoint resolution and filters]): the one match
//! function of the `Fanout` consumer and of replay.
//!
//! [Webhooks › Endpoint resolution and filters]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/webhooks.md

use serde::Deserialize;

/// Event types `*` does not include: an endpoint receives them only when it names them.
pub const OPT_IN_EVENT_TYPES: &[&str] = &["message.labels_changed"];

/// The columns of an endpoint that decide what it receives.
#[derive(Clone, Debug, Deserialize)]
pub struct Scope {
    pub id: String,
    pub tenant_id: Option<String>,
    pub partner_id: Option<String>,
    pub event_types_json: String,
    pub identity_ids_json: Option<String>,
}

/// What the match reads about an event.
#[derive(Clone, Copy, Debug, Default)]
pub struct Facts<'a> {
    pub event_type: &'a str,
    pub tenant_id: Option<&'a str>,
    /// `event_index.partner_id`: the tenant's partner, or for `webhook.disabled` the disabled endpoint's.
    pub partner_id: Option<&'a str>,
    pub identity_id: Option<&'a str>,
    /// The event belongs to the system identity (A15).
    pub system_identity: bool,
    /// For `webhook.disabled`: the endpoint it is about.
    pub about_endpoint: Option<&'a str>,
}

fn list(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Whether endpoint `e` receives the event.
pub fn matches(e: &Scope, f: &Facts<'_>) -> bool {
    if f.event_type == "webhook.test" {
        return false;
    }
    let types = list(&e.event_types_json);
    let typed = types.iter().any(|t| t == f.event_type)
        || (types.iter().any(|t| t == "*") && !OPT_IN_EVENT_TYPES.contains(&f.event_type));
    if !typed {
        return false;
    }
    if let Some(ids) = e.identity_ids_json.as_deref().filter(|j| *j != "null") {
        let ids = list(ids);
        if !f.identity_id.is_some_and(|i| ids.iter().any(|x| x == i)) {
            return false;
        }
    }
    let platform = e.tenant_id.is_none() && e.partner_id.is_none();
    if f.system_identity {
        return platform;
    }
    if f.event_type == "webhook.disabled" {
        if f.about_endpoint == Some(e.id.as_str()) || e.tenant_id.is_some() {
            return false;
        }
        return platform || (e.partner_id.is_some() && e.partner_id.as_deref() == f.partner_id);
    }
    match (&e.tenant_id, &e.partner_id) {
        (None, None) => true,
        (Some(t), _) => f.tenant_id == Some(t.as_str()),
        (None, Some(p)) => f.tenant_id.is_some() && f.partner_id == Some(p.as_str()),
    }
}

/// The permissions an endpoint's event types need beyond `webhooks:manage` (J29).
pub fn required_permissions(event_types: &[String]) -> Vec<&'static str> {
    let any = |pred: &dyn Fn(&str) -> bool| event_types.iter().any(|t| pred(t));
    let mut out = Vec::new();
    if any(&|t| {
        t == "*"
            || t.starts_with("message.")
            || t == "verification.received"
            || t == "calendar.received"
    }) {
        out.push("messages:read");
    }
    if any(&|t| t == "*" || t == "message.quarantined" || t == "message.screened") {
        out.push("quarantine:review");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(
        id: &str,
        tenant: Option<&str>,
        partner: Option<&str>,
        types: &str,
        ids: Option<&str>,
    ) -> Scope {
        Scope {
            id: id.into(),
            tenant_id: tenant.map(Into::into),
            partner_id: partner.map(Into::into),
            event_types_json: types.into(),
            identity_ids_json: ids.map(Into::into),
        }
    }

    #[test]
    fn scope_and_filters() {
        let all = r#"["*"]"#;
        let platform = ep("p", None, None, all, None);
        let partner_p = ep("pp", None, Some("ptn_p"), all, None);
        let partner_q = ep("pq", None, Some("ptn_q"), all, None);
        let tenant = ep("t", Some("ten_1"), None, all, None);
        let of_p = Facts {
            event_type: "message.received",
            tenant_id: Some("ten_1"),
            partner_id: Some("ptn_p"),
            identity_id: Some("idn_1"),
            ..Facts::default()
        };
        assert!(matches(&platform, &of_p) && matches(&partner_p, &of_p) && matches(&tenant, &of_p));
        assert!(!matches(&partner_q, &of_p));
        let unpartnered = Facts {
            partner_id: None,
            tenant_id: Some("ten_2"),
            ..of_p
        };
        assert!(!matches(&partner_p, &unpartnered) && !matches(&tenant, &unpartnered));
        // System identity: platform endpoints only.
        let sys = Facts {
            system_identity: true,
            ..of_p
        };
        assert!(matches(&platform, &sys) && !matches(&partner_p, &sys) && !matches(&tenant, &sys));
        // webhook.disabled about P's endpoint: P's other endpoints and platform endpoints.
        let dis = Facts {
            event_type: "webhook.disabled",
            tenant_id: None,
            partner_id: Some("ptn_p"),
            identity_id: None,
            system_identity: false,
            about_endpoint: Some("pp"),
        };
        let partner_p2 = ep("pp2", None, Some("ptn_p"), all, None);
        assert!(matches(&platform, &dis) && matches(&partner_p2, &dis));
        assert!(
            !matches(&partner_p, &dis) && !matches(&partner_q, &dis) && !matches(&tenant, &dis)
        );
        // Opt-in types and identity filters; webhook.test never fans out.
        let labels = Facts {
            event_type: "message.labels_changed",
            ..of_p
        };
        assert!(!matches(&platform, &labels));
        assert!(matches(
            &ep("x", None, None, r#"["message.labels_changed"]"#, None),
            &labels
        ));
        let filtered = ep("f", Some("ten_1"), None, all, Some(r#"["idn_2"]"#));
        assert!(!matches(&filtered, &of_p));
        assert!(!matches(
            &ep("d", None, None, all, Some(r#"["idn_1"]"#)),
            &Facts {
                identity_id: None,
                event_type: "domain.verified",
                ..of_p
            }
        ));
        assert!(!matches(
            &platform,
            &Facts {
                event_type: "webhook.test",
                ..of_p
            }
        ));
    }

    #[test]
    fn mail_events_need_permissions() {
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            required_permissions(&s(&["*"])),
            ["messages:read", "quarantine:review"]
        );
        assert_eq!(
            required_permissions(&s(&["message.received"])),
            ["messages:read"]
        );
        assert_eq!(
            required_permissions(&s(&["message.quarantined"])),
            ["messages:read", "quarantine:review"]
        );
        assert!(required_permissions(&s(&["domain.verified"])).is_empty());
    }
}
