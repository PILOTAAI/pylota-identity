//! Members and invitations through workerd (Console › Members, Invitations; W9, W10, W36, W47).

use serde_json::json;

use crate::console::{Browser, as_member, email, html, link_for, workspace};
use crate::{d1, delete, fakes, get, platform, post};

fn member_id(tid: &str, who: &str) -> String {
    d1(&format!(
        "SELECT m.user_id FROM members m JOIN users u ON u.id = m.user_id WHERE m.tenant_id = '{tid}' AND u.email = '{who}'"
    ))[0]["user_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// W9: after removal the member's next request, with a session created before it, redirects away.
#[test]
fn w9_remove_revokes_sessions() {
    let _reset = fakes::Reset;
    let (tid, _, _) = workspace("w9");
    let who = email("w9");
    let mut b = as_member(&who, &tid, "member");
    assert_eq!(b.get("/console/inboxes").status, 200);
    let uid = member_id(&tid, &who);
    let r = delete(&format!("/v1/tenants/{tid}/members/{uid}"), &platform());
    assert_eq!(r.status, 204, "{}", r.body);
    let r = b.get("/console/inboxes");
    assert_eq!(r.status, 303);
    assert!(
        r.header("location")
            .unwrap()
            .starts_with("/console/sign-in")
    );
    let r = delete(&format!("/v1/tenants/{tid}/members/{uid}"), &platform());
    assert_eq!((r.status, r.code()), (404, "member_not_found"));
}

/// W10: removing, demoting or the owner leaving gets `409 owner_required`; a transfer to a
/// non-admin changes nothing; to an admin it leaves exactly one owner.
#[test]
fn w10_owner_required() {
    let _reset = fakes::Reset;
    let (tid, _, _) = workspace("w10");
    let owner = email("w10 owner");
    let mut o = as_member(&owner, &tid, "owner");
    let oid = member_id(&tid, &owner);
    let r = delete(&format!("/v1/tenants/{tid}/members/{oid}"), &platform());
    assert_eq!((r.status, r.code()), (409, "owner_required"));
    let mut a = as_member(&email("w10 admin"), &tid, "admin");
    let aid = member_id(&tid, &email("w10 admin"));
    let r = a.submit(
        "/console/members",
        &format!("/console/members/{oid}/role"),
        &[(&format!("role-{oid}"), "viewer")],
    );
    assert_eq!(r.status, 409, "an admin cannot demote the owner");
    let r = o.submit("/console/members", "/console/members/leave", &[]);
    assert_eq!(r.status, 409, "the owner cannot leave");
    let viewer = email("w10 viewer");
    as_member(&viewer, &tid, "viewer");
    let vid = member_id(&tid, &viewer);
    let r = o.submit(
        "/console/members",
        "/console/members/transfer",
        &[("user_id", &vid)],
    );
    assert_eq!(r.status, 400, "the new owner must be an admin");
    let owners = || {
        d1(&format!(
            "SELECT user_id FROM members WHERE tenant_id = '{tid}' AND role = 'owner'"
        ))
    };
    assert_eq!(owners()[0]["user_id"], oid.as_str());
    let r = o.submit(
        "/console/members",
        "/console/members/transfer",
        &[("user_id", &aid)],
    );
    assert_eq!(r.status, 303, "{}", r.body);
    let after = owners();
    assert_eq!(after.as_array().unwrap().len(), 1);
    assert_eq!(after[0]["user_id"], aid.as_str());
}

/// Concurrent transfers leave exactly one owner.
#[test]
fn ownership_transfer() {
    let _reset = fakes::Reset;
    let (tid, _, _) = workspace("transfer race");
    let owner = email("race owner");
    let o = as_member(&owner, &tid, "owner");
    let a1 = email("race a1");
    let a2 = email("race a2");
    as_member(&a1, &tid, "admin");
    as_member(&a2, &tid, "admin");
    let (id1, id2) = (member_id(&tid, &a1), member_id(&tid, &a2));
    std::thread::scope(|s| {
        for id in [&id1, &id2] {
            let mut b = o.clone();
            s.spawn(move || {
                b.submit(
                    "/console/members",
                    "/console/members/transfer",
                    &[("user_id", id)],
                )
                .status
            });
        }
    });
    let owners = d1(&format!(
        "SELECT COUNT(*) AS n FROM members WHERE tenant_id = '{tid}' AND role = 'owner'"
    ));
    assert_eq!(owners[0]["n"], 1);
}

/// W36: keys a person minted in the console are revoked with their removal (`creator_removed`); on a
/// demotion those holding a permission the new role lacks are revoked (`creator_role_changed`) and the
/// rest record the new role.
#[test]
fn w36_removed_member_keys_revoked() {
    let _reset = fakes::Reset;
    let (tid, iid, _) = workspace("w36");
    let admin = email("w36 admin");
    let mut a = as_member(&admin, &tid, "admin");
    let mint = |b: &mut Browser, name: &str, perms: &[&str]| {
        let mut form: Vec<(&str, &str)> = vec![("name", name), ("identity_id", "")];
        for p in perms {
            form.push(("permissions", p));
        }
        let r = b.submit("/console/keys", "/console/keys", &form);
        assert_eq!(r.status, 200, "{}", r.body);
        d1(&format!("SELECT id, created_by_user_id, created_by_role FROM api_keys WHERE tenant_id = '{tid}' AND name = '{name}'"))[0].clone()
    };
    let wide = mint(&mut a, "w36-wide", &["identities:read", "keys:manage"]);
    let narrow = mint(&mut a, "w36-narrow", &["identities:read"]);
    let uid = member_id(&tid, &admin);
    assert_eq!(wide["created_by_user_id"], uid.as_str());
    assert_eq!(wide["created_by_role"], "admin");
    let _ = iid;
    let mut o = as_member(&email("w36 owner"), &tid, "owner");
    let r = o.submit(
        "/console/members",
        &format!("/console/members/{uid}/role"),
        &[(&format!("role-{uid}"), "member")],
    );
    assert_eq!(r.status, 303, "{}", r.body);
    let state = |k: &serde_json::Value| {
        d1(&format!(
            "SELECT revoked_at, created_by_role FROM api_keys WHERE id = '{}'",
            k["id"].as_str().unwrap()
        ))[0]
            .clone()
    };
    assert!(
        state(&wide)["revoked_at"].is_i64(),
        "keys:manage is beyond a member"
    );
    assert!(state(&narrow)["revoked_at"].is_null());
    assert_eq!(state(&narrow)["created_by_role"], "member");
    let r = o.submit(
        "/console/members",
        &format!("/console/members/{uid}/remove"),
        &[],
    );
    assert_eq!(r.status, 303, "{}", r.body);
    assert!(state(&narrow)["revoked_at"].is_i64());
    let reasons = d1(&format!(
        "SELECT details_json FROM audit_log WHERE tenant_id = '{tid}' AND action = 'key.revoke' ORDER BY created_at"
    ));
    let r: Vec<String> = reasons
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["details_json"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        r[0].contains("creator_role_changed") && r[1].contains("creator_removed"),
        "{r:?}"
    );
}

/// W47: endpoints a removed member created are disabled (`creator_removed`) with an audit row each, and
/// the invitations they sent are revoked.
#[test]
fn w47_removed_member_endpoints_disabled() {
    let _reset = fakes::Reset;
    let (tid, _, _) = workspace("w47");
    let admin = email("w47 admin");
    let mut a = as_member(&admin, &tid, "admin");
    let (url, _) = crate::webhooks::receiver("w47");
    let r = a.submit(
        "/console/webhooks",
        "/console/webhooks",
        &[("url", &url), ("events", "message.received")],
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let invited = email("w47 invited");
    let r = a.submit(
        "/console/members",
        "/console/members/invite",
        &[("email", &invited), ("role", "viewer")],
    );
    assert_eq!(r.status, 303, "{}", r.body);
    let uid = member_id(&tid, &admin);
    let r = delete(&format!("/v1/tenants/{tid}/members/{uid}"), &platform());
    assert_eq!(r.status, 204);
    let ep = d1(&format!(
        "SELECT enabled, disabled_reason FROM webhook_endpoints WHERE tenant_id = '{tid}'"
    ));
    assert_eq!(
        (&ep[0]["enabled"], &ep[0]["disabled_reason"]),
        (&json!(0), &json!("creator_removed"))
    );
    let inv = d1(&format!(
        "SELECT status FROM invitations WHERE tenant_id = '{tid}' AND email = '{invited}'"
    ));
    assert_eq!(inv[0]["status"], "revoked");
    let audits = d1(&format!(
        "SELECT action FROM audit_log WHERE tenant_id = '{tid}' AND action IN ('webhook.disable','member.invite_revoke')"
    ));
    assert_eq!(audits.as_array().unwrap().len(), 2);
}

/// FR-CON-4: accept only by the explicit form, re-send, revoke and expire, with the seats after each.
#[test]
fn invitation_lifecycle() {
    let _reset = fakes::Reset;
    let (tid, _, _) = workspace("invitations");
    let seats = || {
        get(&format!("/v1/tenants/{tid}/members"), &platform()).body["seats"]["used"]
            .as_i64()
            .unwrap()
    };
    let base = seats();
    let who = email("invited");
    let r = post(
        &format!("/v1/tenants/{tid}/invitations"),
        &platform(),
        json!({ "email": who, "role": "member" }),
    );
    assert_eq!(r.status, 201, "{}", r.body);
    assert_eq!(seats(), base + 1);
    let first = link_for(&who);
    // Re-sent: a new token, no new seat; the old link no longer works.
    let r = post(
        &format!("/v1/tenants/{tid}/invitations"),
        &platform(),
        json!({ "email": who, "role": "member" }),
    );
    assert_eq!(r.status, 201);
    assert_eq!(seats(), base + 1);
    let t = crate::eventually(30, "the second invitation", || {
        let t = link_for(&who);
        (t != first).then_some(t)
    });
    let mut b = Browser::default();
    assert_eq!(
        b.get(&format!("/console/invitations/accept?t={first}"))
            .status,
        404
    );
    let page = b.get(&format!("/console/invitations/accept?t={t}"));
    assert!(html(&page).contains("Accept"));
    let joined = || {
        d1(&format!("SELECT COUNT(*) AS n FROM members m JOIN users u ON u.id = m.user_id WHERE m.tenant_id = '{tid}' AND u.email = '{who}'"))[0]["n"].as_i64().unwrap()
    };
    assert_eq!(joined(), 0, "the GET accepts nothing");
    let r = b.post("/console/invitations/accept", &[("t", &t)]);
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(joined(), 1);
    assert_eq!(
        b.get("/console").status,
        200,
        "a session in the new workspace"
    );
    assert_eq!(
        seats(),
        base + 1,
        "the invitation's seat became the member's"
    );
    // Revoke frees the seat; expiry too.
    let other = email("invited 2");
    let r = post(
        &format!("/v1/tenants/{tid}/invitations"),
        &platform(),
        json!({ "email": other, "role": "viewer" }),
    );
    let inv = r.body["id"].as_str().unwrap().to_owned();
    assert_eq!(
        delete(&format!("/v1/tenants/{tid}/invitations/{inv}"), &platform()).status,
        204
    );
    assert_eq!(
        delete(&format!("/v1/tenants/{tid}/invitations/{inv}"), &platform()).code(),
        "invitation_not_found"
    );
    let third = email("invited 3");
    post(
        &format!("/v1/tenants/{tid}/invitations"),
        &platform(),
        json!({ "email": third, "role": "viewer" }),
    );
    fakes::advance(7 * 86_400_000 + 60_000);
    fakes::advance_to_minute(1);
    crate::cron("*/15 * * * *");
    let st = d1(&format!(
        "SELECT status FROM invitations WHERE tenant_id = '{tid}' AND email = '{third}'"
    ));
    assert_eq!(st[0]["status"], "expired");
    // An address this deployment hosts, or a member, is refused.
    let (_, _, hosted) = workspace("invitations hosted");
    let r = post(
        &format!("/v1/tenants/{tid}/invitations"),
        &platform(),
        json!({ "email": hosted, "role": "viewer" }),
    );
    assert_eq!((r.status, r.code()), (400, "invalid_request"));
    let r = post(
        &format!("/v1/tenants/{tid}/invitations"),
        &platform(),
        json!({ "email": who, "role": "viewer" }),
    );
    assert_eq!((r.status, r.code()), (400, "invalid_request"));
}
