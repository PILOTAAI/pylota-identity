//! Subject-access exports through workerd (Privacy and erasure › Tests): the ZIP, the one-use
//! signed link and the permissions (I3, I14).

use serde_json::{Value, json};

use crate::erasure::{job_alarm, run_job};
use crate::send::{Sender, open_sender, sender_in};
use crate::{d1, eventually, fakes, fetch_bytes, get, inbound, mail, mint, platform, post};

/// Creates an export with `key`; answers the response.
fn create(key: &str, body: Value) -> crate::Resp {
    post("/v1/exports", key, body)
}

/// Drives the export's job to its end; answers the export.
fn finish(exp: &str) -> Value {
    let job = d1(&format!("SELECT job_id FROM exports WHERE id = '{exp}'"))[0]["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    eventually(120, "the export to end", || {
        job_alarm(&job);
        let r = get(&format!("/v1/exports/{exp}"), &platform());
        (!matches!(r.body["status"].as_str(), Some("queued" | "running"))).then_some(r.body)
    })
}

/// The path of a download URL.
fn path_of(url: &str) -> String {
    format!("/v1/links/{}", url.rsplit_once("/v1/links/").unwrap().1)
}

/// The ZIP behind a fresh link: `(name, bytes)` entries.
fn download(exp: &str, key: &str) -> Vec<(String, Vec<u8>)> {
    let r = get(&format!("/v1/exports/{exp}"), key);
    assert_eq!(r.status, 200, "{}", r.body);
    let (status, bytes) = fetch_bytes(&path_of(r.body["download_url"].as_str().unwrap()));
    assert_eq!(status, 200);
    pylota_identity_core::zip::read(&bytes).expect("a valid ZIP")
}

fn messages_json(entries: &[(String, Vec<u8>)]) -> Value {
    let (_, b) = entries
        .iter()
        .find(|(n, _)| n == "messages.json")
        .expect("messages.json");
    serde_json::from_slice(b).unwrap()
}

/// A counterparty's mail in two identities.
fn counterparty_mail(tag: &str, cp: &str) -> (Sender, Sender) {
    let a = open_sender(tag, "live");
    let b = sender_in(
        &a.tid,
        &format!("{tag} b"),
        json!({ "require_known_recipient": false }),
    );
    for s in [&a, &b] {
        let (o, _) = inbound(
            cp,
            &s.addr,
            &mail(cp, &s.addr, &format!("{tag} to {}", s.iid), "Hi"),
        );
        assert_eq!(o, "accepted");
        crate::wait_stored(&s.iid, &s.key, "received", 1);
    }
    (a, b)
}

/// I3: the ZIP holds one `.eml` per message and `messages.json`; a message past `raw_days` is
/// rebuilt; links fail when tampered, used, stale or after the export is deleted.
#[test]
fn i3_counterparty() {
    let _reset = fakes::Reset;
    let cp = "dana@i3.example";
    let (a, b) = counterparty_mail("i3 export", cp);
    let r = crate::send::send(
        &a,
        json!({ "to": [cp], "bcc": ["boss@i3.example"], "subject": "i3 reply", "text": "Sure" }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    let r = create(
        &platform(),
        json!({ "tenant_id": a.tid, "scope": "counterparty", "counterparty_address": cp }),
    );
    assert_eq!(r.status, 202, "{}", r.body);
    assert_eq!(r.body["status"], "queued");
    let exp = r.body["id"].as_str().unwrap().to_owned();
    let done = finish(&exp);
    assert_eq!(done["status"], "completed", "{done}");
    let entries = download(&exp, &platform());
    let doc = messages_json(&entries);
    assert_eq!(doc["scope"], "counterparty");
    assert_eq!(doc["counterparty_hash"].as_str().unwrap().len(), 64);
    let msgs = doc["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3, "{doc}");
    for m in msgs {
        let path = m["eml_path"].as_str().unwrap();
        assert!(path.starts_with(&format!("eml/{}/", m["identity_id"].as_str().unwrap())));
        let (_, eml) = entries.iter().find(|(n, _)| n == path).expect("its .eml");
        let text = String::from_utf8_lossy(eml);
        assert!(!text.to_ascii_lowercase().contains("\r\nbcc:"), "{text}");
        assert_eq!(m["eml_source"], "original");
    }
    let ids: Vec<&str> = doc["identities"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(ids.contains(&a.iid.as_str()) && ids.contains(&b.iid.as_str()));
    // A link works once; a fresh GET mints a new one; a tampered one never works.
    let url = get(&format!("/v1/exports/{exp}"), &platform()).body["download_url"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(fetch_bytes(&path_of(&url)).0, 200);
    assert_eq!(fetch_bytes(&path_of(&url)).0, 404);
    let mut bad = path_of(&url);
    bad.push('A');
    assert_eq!(fetch_bytes(&bad).0, 404);
    // A link older than its hour fails.
    let url = get(&format!("/v1/exports/{exp}"), &platform()).body["download_url"]
        .as_str()
        .unwrap()
        .to_owned();
    fakes::advance(61 * 60_000);
    assert_eq!(fetch_bytes(&path_of(&url)).0, 404);
    // Deleting the export revokes every link at once.
    let url = get(&format!("/v1/exports/{exp}"), &platform()).body["download_url"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = crate::delete(&format!("/v1/exports/{exp}"), &platform());
    assert_eq!(r.status, 204, "{}", r.body);
    assert_eq!(fetch_bytes(&path_of(&url)).0, 404);
    assert_eq!(
        get(&format!("/v1/exports/{exp}"), &platform()).body["status"],
        "expired"
    );
    // After raw_days the message is rebuilt from its stored fields.
    fakes::advance(91 * 86_400_000);
    let (_, st) = run_job("retention", Some(&a.tid), json!({}));
    assert_eq!(st, "completed");
    let r = create(
        &platform(),
        json!({ "tenant_id": a.tid, "scope": "identity", "identity_id": b.iid }),
    );
    let exp2 = r.body["id"].as_str().unwrap().to_owned();
    assert_eq!(finish(&exp2)["status"], "completed");
    let doc = messages_json(&download(&exp2, &platform()));
    assert_eq!(doc["messages"][0]["eml_source"], "reconstructed", "{doc}");
}

/// I14: an export needs `erasure:manage`, `messages:read` and `attachments:read` and is refused to
/// identity keys; quarantined mail is withheld without `quarantine:review`.
#[test]
fn i14_export_permissions() {
    let s = open_sender("i14 export", "live");
    let only_erasure = mint(
        &platform(),
        json!({ "name": "e", "level": "tenant", "tenant_id": s.tid, "permissions": ["erasure:manage"] }),
    )
    .1;
    let body = json!({ "tenant_id": s.tid, "scope": "identity", "identity_id": s.iid });
    let r = create(&only_erasure, body.clone());
    assert_eq!(
        (r.status, r.code()),
        (403, "permission_denied"),
        "{}",
        r.body
    );
    let required = r.details()["required"].to_string();
    assert!(
        required.contains("messages:read") && required.contains("attachments:read"),
        "{required}"
    );
    let idk = mint(
        &platform(),
        json!({ "name": "i", "level": "identity", "tenant_id": s.tid, "identity_id": s.iid,
                "permissions": ["erasure:manage", "messages:read", "attachments:read"] }),
    )
    .1;
    assert_eq!(create(&idk, body.clone()).status, 403);
    // A quarantined message: a risky attachment.
    let raw = format!(
        "From: mal@i14.example\r\nTo: {0}\r\nSubject: i14 risky\r\nMessage-ID: <{1}@it.example>\r\nDate: Mon, 26 Oct 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nSee\r\n--b\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"run.exe\"\r\nContent-Transfer-Encoding: base64\r\n\r\nTVqQAAMAAAAEAAAA//8AALgAAAAAAAAAQAAAAAAAAAAA\r\n--b--\r\n",
        s.addr,
        crate::slug("i14 risky")
    );
    let (o, _) = inbound("mal@i14.example", &s.addr, &raw);
    assert_eq!(o, "accepted");
    crate::wait_stored(&s.iid, &s.key, "quarantined", 1);
    let reader = |review: bool| {
        let mut perms = vec!["erasure:manage", "messages:read", "attachments:read"];
        if review {
            perms.push("quarantine:review");
        }
        mint(
            &platform(),
            json!({ "name": "r", "level": "tenant", "tenant_id": s.tid, "permissions": perms }),
        )
        .1
    };
    for review in [false, true] {
        let key = reader(review);
        let r = create(&key, body.clone());
        assert_eq!(r.status, 202, "{}", r.body);
        let exp = r.body["id"].as_str().unwrap().to_owned();
        assert_eq!(finish(&exp)["status"], "completed");
        let entries = download(&exp, &key);
        let doc = messages_json(&entries);
        let m = doc["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["status"] == "quarantined")
            .cloned()
            .expect("the quarantined message is listed");
        if review {
            assert!(m["eml_path"].is_string(), "{m}");
            assert!(m.get("content_withheld").is_none());
        } else {
            assert_eq!(m["content_withheld"], "quarantine_review_required");
            assert!(m["eml_path"].is_null());
            assert!(
                !entries
                    .iter()
                    .any(|(n, _)| n.ends_with(&format!("{}.eml", m["id"].as_str().unwrap())))
            );
        }
    }
    // Used links are forgotten by the global retention job once past their expiry.
    let used = d1("SELECT COUNT(*) AS n FROM link_uses")[0]["n"]
        .as_i64()
        .unwrap();
    assert!(used >= 2);
    let _reset = fakes::Reset;
    fakes::advance(2 * 3_600_000);
    run_job("retention", None, json!({ "global": true }));
    let left = d1(&format!(
        "SELECT COUNT(*) AS n FROM link_uses WHERE expires_at < {}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
            + 3_600_000
    ))[0]["n"]
        .as_i64()
        .unwrap();
    assert_eq!(left, 0);
}
