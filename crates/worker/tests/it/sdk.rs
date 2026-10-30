//! The Rust SDK against the workerd harness (build plan M16, FR-SDK-1): every operation the Worker
//! serves is called through its SDK method, and every answer decodes into its `api-types` object.
//! `every_served_operation` checks that the calls below cover the route table.

use std::collections::BTreeSet;

use pylota_identity::types;
use pylota_identity::{Client, Error, IdempotencyKey};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{fakes, hook, inbound, platform};

/// The operations the Worker serves that the SDK leaves out (`x-sdk: false`).
const NOT_CALLED: &[&str] = &["getSecurityTxt"];

/// A request body from JSON.
fn body<T: DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{v}: {e}"))
}

/// A member of an answer, as JSON.
fn field<T: serde::Serialize>(t: &T, name: &str) -> String {
    serde_json::to_value(t).unwrap()[name]
        .as_str()
        .unwrap_or_else(|| panic!("no {name}"))
        .to_owned()
}

fn client(key: &str) -> Client {
    let pem = std::fs::read(std::env::var("PM_ITEST_CA").unwrap()).unwrap();
    let url = std::env::var("PM_ITEST_URL").unwrap();
    let port: u16 = url.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap();
    let http = reqwest::Client::builder()
        .add_root_certificate(reqwest::Certificate::from_pem(&pem).unwrap())
        .resolve(
            "localhost",
            std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        )
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    Client::builder()
        .base_url(url)
        .api_key(key)
        .http_client(http)
        .build()
        .unwrap()
}

/// Records each operation called.
#[derive(Default)]
struct Seen(BTreeSet<&'static str>);

impl Seen {
    /// A call that must succeed.
    fn ok<T>(&mut self, op: &'static str, r: Result<T, Error>) -> T {
        self.0.insert(op);
        r.unwrap_or_else(|e| panic!("{op}: {e:?}"))
    }

    /// A call whose answer depends on state the test does not set up: success, or one of these
    /// error codes, decoded from the envelope.
    fn any<T>(&mut self, op: &'static str, r: Result<T, Error>, codes: &[&str]) -> Option<T> {
        self.0.insert(op);
        match r {
            Ok(t) => Some(t),
            Err(e) => {
                let code = e.api().map(|a| a.code.clone()).unwrap_or_default();
                assert!(codes.contains(&code.as_str()), "{op}: {e:?}");
                None
            }
        }
    }
}

fn key(k: &str) -> IdempotencyKey {
    IdempotencyKey::new(&format!("{k}-{}", crate::slug("sdk"))).unwrap()
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
}

/// A blocking harness helper inside the async test (it runs its own client).
fn blocking<T>(f: impl FnOnce() -> T) -> T {
    tokio::task::block_in_place(f)
}

/// Every served operation through the SDK; the calls decode into their types.
#[test]
fn every_served_operation() {
    let _reset = fakes::Reset;
    let mut seen = Seen::default();
    let p = client(&platform());
    rt().block_on(async {
        // Meta.
        seen.ok("getHealth", p.get_health().await);
        let doc = seen.ok("getOpenApiDocument", p.get_open_api_document().await);
        assert_eq!(doc["openapi"], "3.1.0");
        seen.ok("getMe", p.get_me().await);

        // Partners.
        let ptn = seen.ok("createPartner", p.create_partner(&body(json!({ "name": crate::slug("sdk partner") })), None).await);
        let pid = field(&ptn, "id");
        seen.ok("getPartner", p.get_partner(&pid).await);
        seen.ok("listPartners", p.list_partners(&Default::default()).await);
        seen.ok("updatePartner", p.update_partner(&pid, &body(json!({ "name": "renamed" }))).await);
        seen.ok("deletePartner", p.delete_partner(&pid).await);

        // A tenant with a healthy domain, an identity on it and a tenant key.
        let (s, did, zone) = blocking(|| crate::addresses::on_domain("sdk table"));
        let tid = s.tid.clone();
        let t = client(&s.key);
        seen.ok("getTenant", p.get_tenant(&tid).await);
        seen.ok("listTenants", p.list_tenants(&Default::default()).await);
        seen.ok("updateTenant", p.update_tenant(&tid, &body(json!({ "name": "SDK table" }))).await);
        seen.ok("getTenantPolicy", p.get_tenant_policy(&tid).await);
        seen.ok("updateTenantPolicy", p.update_tenant_policy(&tid, &body(json!({ "search": { "agentic_enabled": true } }))).await);
        let made = seen.ok("createTenant", p.create_tenant(&body(json!({ "slug": crate::slug("sdk second"), "name": "Second", "mode": "live" })), Some(&key("tenant"))).await);
        let tid2 = field(&made, "id");

        // Keys.
        let all = json!(["identities:read", "identities:write", "messages:read", "messages:write", "messages:send",
            "search:read", "search:agentic", "attachments:read", "quarantine:review", "domains:read", "domains:write",
            "webhooks:manage", "suppressions:manage", "erasure:manage", "keys:manage", "policy:write", "audit:read"]);
        let k = seen.ok("createKey", p.create_key(&body(json!({ "name": "sdk", "level": "tenant", "tenant_id": tid, "permissions": all })), None).await);
        let kid = field(&k, "id");
        seen.ok("getKey", p.get_key(&kid).await);
        let q = pylota_identity::ListKeysQuery::default();
        seen.ok("listKeys", p.list_keys(&q).await);
        let rotated = seen.ok("rotateKey", p.rotate_key(&kid, &body(json!({})), None).await);
        let tkey = field(&rotated, "secret");
        let tk = client(&tkey);
        let spare = p.create_key(&body(json!({ "name": "spare", "level": "tenant", "tenant_id": tid2, "permissions": ["identities:read"] })), None).await.unwrap();
        seen.ok("revokeKey", p.revoke_key(&field(&spare, "id")).await);

        // Identities.
        let idn = seen.ok("createIdentity", p.create_identity(&tid, &body(json!({ "username": crate::slug("sdkid")[..10].to_owned(), "display_name": "SDK" })), None).await);
        let iid2 = field(&idn, "id");
        seen.ok("getIdentity", t.get_identity(&s.iid).await);
        seen.ok("listIdentities", p.list_identities(&Default::default()).await);
        seen.ok("listTenantIdentities", t.list_tenant_identities(&tid, &Default::default()).await);
        let q = pylota_identity::LookupIdentityQuery { address: Some(types::EmailAddress(s.addr.clone())) };
        seen.ok("lookupIdentity", t.lookup_identity(&q).await);
        seen.ok("updateIdentity", t.update_identity(&s.iid, &body(json!({ "display_name": "Acme Desk" }))).await);

        // Addresses on the domain.
        let a = seen.ok("createAddress", s_create_address(&t, &s.iid, &did).await);
        let aid = field(&a, "id");
        seen.ok("listAddresses", t.list_addresses(&s.iid, &Default::default()).await);
        let b = t.create_address(&s.iid, &body(json!({ "local_part": "desk2", "domain_id": did })), None).await.unwrap();
        seen.any("promoteAddress", t.promote_address(&s.iid, &field(&b, "id"), &body(json!({})), None).await, &["domain_not_ready", "address_in_use"]);
        seen.any("retireAddress", t.retire_address(&s.iid, &aid, &body(json!({ "after_days": 1 })), None).await, &["address_is_primary", "address_in_use"]);
        seen.any("deleteAddress", t.delete_address(&s.iid, &field(&b, "id")).await, &["address_in_use", "conflict"]);

        // Mail.
        let from = "zia@sdk.example";
        let raw = format!(
            "From: {from}\r\nTo: {0}\r\nSubject: SDK booking BK-77\r\nMessage-ID: <{1}@it.example>\r\nDate: Mon, 26 Oct 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nThe booking is attached.\r\n--b\r\nContent-Type: text/plain; name=\"note.txt\"\r\nContent-Disposition: attachment; filename=\"note.txt\"\r\n\r\nBooking BK-77 for Friday.\r\n--b--\r\n",
            s.addr,
            crate::slug("sdk mail")
        );
        let (o, _) = blocking(|| inbound(from, &s.addr, &raw));
        assert_eq!(o, "accepted");
        let stored = blocking(|| crate::wait_stored(&s.iid, &s.key, "received", 1));
        let mid = stored[0]["id"].as_str().unwrap().to_owned();
        let thr = stored[0]["thread_id"].as_str().unwrap().to_owned();
        seen.ok("listMessages", t.list_messages(&s.iid, &Default::default()).await);
        let m = seen.ok("getMessage", t.get_message(&s.iid, &mid, &Default::default()).await);
        let att = serde_json::to_value(&m).unwrap()["attachments"][0]["id"].as_str().unwrap().to_owned();
        let r = seen.ok("getMessageRaw", t.get_message_raw(&s.iid, &mid).await);
        assert!(String::from_utf8_lossy(&r.body).contains("SDK booking"));
        seen.ok("getAttachment", t.get_attachment(&s.iid, &mid, &att).await);
        seen.ok("getAttachmentText", t.get_attachment_text(&s.iid, &mid, &att, &Default::default()).await);
        seen.ok("updateMessage", t.update_message(&s.iid, &mid, &body(json!({ "labels_add": ["sdk"] }))).await);
        seen.ok("listThreads", t.list_threads(&s.iid, &Default::default()).await);
        seen.ok("getThread", t.get_thread(&s.iid, &thr, &Default::default()).await);
        seen.ok("updateThread", t.update_thread(&s.iid, &thr, &body(json!({ "read": true }))).await);
        seen.any("rerunTriage", tk.rerun_triage(&s.iid, &mid, None).await, &["permission_denied", "conflict"]);
        let hits = seen.ok("searchIdentity", tk.search_identity(&s.iid, &json!({ "q": "booking", "mode": "keyword" })).await);
        assert!(!hits["hits"].as_array().unwrap().is_empty());
        seen.ok("searchTenant", tk.search_tenant(&tid, &json!({ "q": "booking", "mode": "keyword" })).await);
        seen.ok("findRelatedMessages", tk.find_related_messages(&s.iid, &mid, &Default::default()).await);
        let q = pylota_identity::SearchContactsQuery { q: Some("zia".into()), ..Default::default() };
        seen.ok("searchContacts", tk.search_contacts(&s.iid, &q).await);
        let q = pylota_identity::WaitForMessageQuery { timeout: Some(1), since: Some("2099-01-01T00:00:00Z".into()), ..Default::default() };
        let w = seen.ok("waitForMessage", tk.wait_for_message(&s.iid, &q).await);
        assert!(w.timed_out);

        // Sending.
        let req: types::SendRequest = body(json!({ "to": ["pia@sdk.example"], "subject": "SDK send", "text": "Hi" }));
        let dry = t.send_message_dry_run(&s.iid, &req).await.unwrap();
        assert!(serde_json::to_value(&dry).unwrap()["would_send"].is_boolean());
        let send_key = key("send");
        let sent = seen.ok("sendMessage", t.send_message(&s.iid, &req, &send_key).await);
        assert!(!sent.deduplicated);
        let again = t.send_message(&s.iid, &req, &send_key).await.unwrap();
        assert_eq!((again.message.id.0.clone(), again.deduplicated), (sent.message.id.0.clone(), true));
        seen.ok("replyToMessage", t.reply_to_message(&s.iid, &mid, &body(json!({ "text": "Thanks" })), &key("reply")).await);
        seen.ok("replyAllToMessage", t.reply_all_to_message(&s.iid, &mid, &body(json!({ "text": "Thanks all" })), &key("replyall")).await);
        seen.ok("forwardMessage", t.forward_message(&s.iid, &mid, &body(json!({ "to": ["rex@sdk.example"], "text": "FYI" })), &key("fwd")).await);
        seen.any("cancelMessage", t.cancel_message(&s.iid, &sent.message.id.0, None).await, &["not_cancelable"]);
        seen.any("resolveMessage", t.resolve_message(&s.iid, &sent.message.id.0, &body(json!({ "outcome": "sent" })), None).await, &["not_uncertain"]);

        // Quarantine.
        seen.ok("listQuarantine", t.list_quarantine(&s.iid, &Default::default()).await);
        seen.any("releaseMessage", t.release_message(&s.iid, &mid, &body(json!({ "reason": "expected" })), None).await, &["not_quarantined", "permission_denied"]);

        // Holds and erasure.
        seen.ok("setThreadHold", tk.set_thread_hold(&s.iid, &thr, &body(json!({ "reason": "audit" })), None).await);
        seen.ok("removeThreadHold", tk.remove_thread_hold(&s.iid, &thr).await);
        let era = seen.ok("createErasureRequest", tk.create_erasure_request(&body(json!({ "tenant_id": tid, "scope": "message", "identity_id": s.iid, "message_id": sent.message.id.0, "reason": "asked" })), None).await);
        let era_id = field(&era, "id");
        seen.ok("getErasureRequest", tk.get_erasure_request(&era_id).await);
        let q = pylota_identity::ListErasureRequestsQuery { tenant_id: Some(types::TenantId(tid.clone())), ..Default::default() };
        seen.ok("listErasureRequests", tk.list_erasure_requests(&q).await);
        seen.any("reapplyErasureRequest", p.reapply_erasure_request(&era_id, None).await, &["erasure_not_completed"]);
        seen.ok("deleteMessage", tk.delete_message(&s.iid, &mid).await);
        seen.ok("deleteIdentity", p.delete_identity(&iid2).await);

        // Exports and signed links.
        let ex = seen.ok("createExport", p.create_export(&body(json!({ "tenant_id": tid, "scope": "identity", "identity_id": s.iid })), None).await);
        let exp = field(&ex, "id");
        let got = seen.ok("getExport", p.get_export(&exp).await);
        if let Some(url) = serde_json::to_value(&got).unwrap()["download_url"].as_str() {
            let token = url.rsplit_once("/v1/links/").unwrap().1;
            seen.ok("getSignedLink", p.get_signed_link(token).await);
        } else {
            seen.any("getSignedLink", p.get_signed_link("not-a-token").await, &["attachment_not_found"]);
        }
        seen.ok("deleteExport", p.delete_export(&exp).await);

        // Jobs, signing keys, audit.
        let job = seen.ok("createJob", p.create_job(&body(json!({ "kind": "reindex", "tenant_id": tid })), None).await);
        seen.ok("getJob", p.get_job(&field(&job, "id")).await);
        seen.ok("getPlatformStatus", p.get_platform_status().await);
        seen.ok("listDlqItems", p.list_dlq_items(&Default::default()).await);
        seen.any(
            "restoreMailbox",
            p.restore_mailbox(&s.iid, &body(json!({ "bookmark": "b" }))).await,
            &["not_frozen"],
        );
        seen.ok(
            "setSwitch",
            p.set_switch(&body(json!({ "name": "agent_signup", "state": "on", "reason": "sdk coverage" })), None)
                .await,
        );
        seen.any(
            "redriveDlqItem",
            p.redrive_dlq_item("dlq_01ARYZ6S41TSV4RRFFQ69G5FAV", None).await,
            &["dlq_item_not_found"],
        );
        seen.ok("rotateSigningKey", p.rotate_signing_key("cursor", &Default::default(), None).await);
        let q = pylota_identity::ListAuditEventsQuery { tenant_id: Some(types::TenantId(tid.clone())), ..Default::default() };
        seen.ok("listAuditEvents", p.list_audit_events(&q).await);

        // Webhooks.
        let (url, _) = crate::webhooks::receiver("sdk hook");
        let w = seen.ok("createTenantWebhook", tk.create_tenant_webhook(&tid, &body(json!({ "url": url, "events": ["identity.created"] })), None).await);
        let wid = field(&w, "id");
        seen.ok("listTenantWebhooks", tk.list_tenant_webhooks(&tid, &Default::default()).await);
        seen.ok("getWebhook", tk.get_webhook(&wid).await);
        seen.ok("updateWebhook", tk.update_webhook(&wid, &body(json!({ "description": "sdk" }))).await);
        seen.ok("testWebhook", tk.test_webhook(&wid, None).await);
        seen.ok("listWebhookDeliveries", tk.list_webhook_deliveries(&wid, &Default::default()).await);
        seen.ok("replayWebhookEvents", tk.replay_webhook_events(&wid, &body(json!({ "since": "2026-01-01T00:00:00Z" })), None).await);
        seen.ok("rotateWebhookSecret", tk.rotate_webhook_secret(&wid, &body(json!({})), None).await);
        seen.ok("deleteWebhook", tk.delete_webhook(&wid).await);
        let (purl, _) = crate::webhooks::receiver("sdk platform hook");
        let pw = seen.ok("createPlatformWebhook", p.create_platform_webhook(&body(json!({ "url": purl, "events": ["domain.removed"] })), None).await);
        seen.ok("listWebhooks", p.list_webhooks(&Default::default()).await);
        p.delete_webhook(&field(&pw, "id")).await.unwrap();

        // Suppressions and lists.
        seen.ok("createSuppression", tk.create_suppression(&tid, &body(json!({ "address": "stop@sdk.example", "note": "asked" })), None).await);
        seen.ok("listSuppressions", tk.list_suppressions(&tid, &Default::default()).await);
        seen.any("deleteSuppression", tk.delete_suppression(&tid, "stop@sdk.example", &body(json!({ "reason": "asked again" }))).await, &["suppression_not_removable"]);
        let entry = "@blocked.sdk.example";
        seen.ok("putListEntry", tk.put_list_entry(&tid, "send", "block", entry, &body(json!({}))).await);
        seen.ok("getListEntry", tk.get_list_entry(&tid, "send", "block", entry).await);
        seen.ok("listListEntries", tk.list_list_entries(&tid, "send", "block", &Default::default()).await);
        seen.ok("deleteListEntry", tk.delete_list_entry(&tid, "send", "block", entry).await);
        seen.ok("putIdentityListEntry", tk.put_identity_list_entry(&s.iid, "send", "block", entry, &body(json!({}))).await);
        seen.ok("getIdentityListEntry", tk.get_identity_list_entry(&s.iid, "send", "block", entry).await);
        seen.ok("listIdentityListEntries", tk.list_identity_list_entries(&s.iid, "send", "block", &Default::default()).await);
        seen.ok("deleteIdentityListEntry", tk.delete_identity_list_entry(&s.iid, "send", "block", entry).await);

        // Domains.
        seen.ok("getDomain", tk.get_domain(&did).await);
        seen.ok("listTenantDomains", tk.list_tenant_domains(&tid, &Default::default()).await);
        seen.ok("getDomainRecords", tk.get_domain_records(&did, &Default::default()).await);
        seen.ok("getDomainHealth", tk.get_domain_health(&did).await);
        seen.any("verifyDomain", tk.verify_domain(&did, None).await, &["rate_limited"]);
        seen.any("reproveDomain", tk.reprove_domain(&did, None).await, &["domain_not_suspended", "conflict", "invalid_state"]);
        seen.any("probeDomain", tk.probe_domain(&did, None).await, &["transport_unavailable", "unprocessable", "domain_not_ready"]);
        seen.any("updateDomain", p.update_domain(&did, &body(json!({ "sending_paused": false }))).await, &["invalid_request", "transport_unavailable"]);
        let sub = format!("news.{zone}");
        crate::cloud::record(&zone, "TXT", &format!("_dmarc.{sub}"), "v=DMARC1; p=quarantine", None);
        let d2 = seen.ok("createDomain", tk.create_domain(&tid, &body(json!({ "name": sub, "method": "cloudflare_zone" })), None).await);
        seen.ok("deleteDomain", tk.delete_domain(&field(&d2, "id")).await);
        let _ = t;
    });
    // Every served operation was called (or is left out of the SDK).
    let routes = hook("GET", "/__test/routes", None);
    let served: BTreeSet<String> = routes.body["routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["operation_id"].as_str().unwrap().to_owned())
        .filter(|o| !NOT_CALLED.contains(&o.as_str()))
        .collect();
    let called: BTreeSet<String> = seen.0.iter().map(|s| (*s).to_owned()).collect();
    let missing: Vec<_> = served.difference(&called).collect();
    assert!(
        missing.is_empty(),
        "served operations the SDK test does not call: {missing:?}"
    );
}

async fn s_create_address(t: &Client, iid: &str, did: &str) -> Result<types::Address, Error> {
    t.create_address(
        iid,
        &body(json!({ "local_part": "desk", "domain_id": did })),
        None,
    )
    .await
}
