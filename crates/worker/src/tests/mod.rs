//! Worker-logic tests on `platform::fakes`: the app over a migrated fake D1, with `TenantQuota`,
//! `IdentityMailbox` and `JobRunner` calls routed to their logic over one fake SQLite per object.

mod flows;
mod identities;
mod inbound;
mod search;
mod triage;
mod webhooks;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pylota_identity_core::keys as ck;
use pylota_identity_platform::app::{HttpRequestIn, HttpResponseOut};
use pylota_identity_platform::buffer::JsBuffer;
use pylota_identity_platform::d1::Stmt;
use pylota_identity_platform::durable::DoClass;
use pylota_identity_platform::fakes::{FakePlatform, FakeSql, FakeState, block_on};
use pylota_identity_platform::{ControlDb as _, Platform as _};
use serde_json::{Value, json};

use crate::app::App;
use crate::mailbox::Mailbox;
use crate::quota::Quota;

pub struct Harness {
    pub app: App<FakePlatform>,
    pub objects: Rc<RefCell<HashMap<String, FakeSql>>>,
    pub mailboxes: Rc<RefCell<HashMap<String, FakeState>>>,
}

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
}

impl Harness {
    pub fn new() -> Self {
        Self::with_vars(&[])
    }

    pub fn with_vars(vars: &[(&str, &str)]) -> Self {
        let p = FakePlatform::with_vars(vars);
        p.db.exec_script(include_str!("../../../../migrations/d1/0001_init.sql"))
            .unwrap();
        let app = App::new(p);
        let objects: Rc<RefCell<HashMap<String, FakeSql>>> = Rc::default();
        let objs = objects.clone();
        let weak = Rc::downgrade(&app.p);
        app.p.objects.on(DoClass::Quota, move |id, env| {
            let sql = objs.borrow_mut().entry(id).or_default().clone();
            let p = weak.upgrade().expect("the platform outlives the call");
            async move {
                Ok(Quota {
                    p: p.as_ref(),
                    sql: &sql,
                }
                .handle(&env))
            }
        });
        let mailboxes: Rc<RefCell<HashMap<String, FakeState>>> = Rc::default();
        let boxes = mailboxes.clone();
        let weak = Rc::downgrade(&app.p);
        app.p.objects.on(DoClass::Mailbox, move |id, env| {
            let state = boxes.borrow_mut().entry(id.clone()).or_default().clone();
            let p = weak.upgrade().expect("the platform outlives the call");
            async move {
                Ok(Mailbox {
                    p: p.as_ref(),
                    sql: &state.sql,
                    state: &state,
                    object_id: &id,
                }
                .handle(&env)
                .await)
            }
        });
        let runners: Rc<RefCell<HashMap<String, FakeState>>> = Rc::default();
        let weak = Rc::downgrade(&app.p);
        app.p.objects.on(DoClass::Job, move |id, env| {
            let state = runners.borrow_mut().entry(id).or_default().clone();
            let p = weak.upgrade().expect("the platform outlives the call");
            async move {
                Ok(crate::jobs::Runner {
                    p: p.as_ref(),
                    sql: &state.sql,
                    state: &state,
                }
                .handle(&env)
                .await)
            }
        });
        Self {
            app,
            objects,
            mailboxes,
        }
    }

    /// Runs one mailbox's alarm now.
    pub fn mailbox_alarm(&self, object_id: &str) {
        let state = self
            .mailboxes
            .borrow()
            .get(object_id)
            .cloned()
            .expect("a mailbox");
        block_on(
            Mailbox {
                p: self.app.p.as_ref(),
                sql: &state.sql,
                state: &state,
                object_id,
            }
            .alarm(),
        );
    }

    /// Rows from one mailbox's SQLite.
    pub fn mailbox_sql(&self, object_id: &str, sql: &str) -> Vec<Value> {
        let state = self
            .mailboxes
            .borrow()
            .get(object_id)
            .cloned()
            .expect("a mailbox");
        pylota_identity_platform::durable::Sql::query(&state.sql, sql, &[]).unwrap()
    }

    pub fn call(
        &self,
        method: &str,
        path: &str,
        key: Option<&str>,
        body: Option<Value>,
        extra: &[(&str, &str)],
    ) -> Resp {
        let mut headers = vec![("host".to_owned(), "api.pylotamail.test".to_owned())];
        if let Some(k) = key {
            headers.push(("authorization".into(), format!("Bearer {k}")));
        }
        for (n, v) in extra {
            headers.push(((*n).to_owned(), (*v).to_owned()));
        }
        let req = HttpRequestIn {
            method: method.to_owned(),
            url: format!("https://api.pylotamail.test{path}"),
            headers,
            body: body.map(|b| JsBuffer::from_bytes(b.to_string().as_bytes())),
            cf_connecting_ip: Some("192.0.2.1".into()),
        };
        let out = block_on(self.app.fetch(req));
        self.app.p.background.run_pending();
        let (status, headers, body) = match out {
            HttpResponseOut::Http {
                status,
                headers,
                body,
            } => (status, headers, body),
            // A stream is complete once the background work has run.
            HttpResponseOut::Stream {
                status,
                headers,
                body,
            } => (status, headers, body.text().into_bytes()),
            HttpResponseOut::Upgrade(_) => panic!("an upgrade"),
        };
        let body = serde_json::from_slice(&body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
        Resp {
            status,
            headers,
            body,
        }
    }

    /// Inserts a key row directly (as `idvy setup` does for the first platform key) and returns its token.
    pub fn seed_key(
        &self,
        level: &str,
        permissions: &[&str],
        tenant: Option<&str>,
        identity: Option<&str>,
        partner: Option<&str>,
        role: Option<&str>,
    ) -> String {
        let p = &self.app.p;
        let mode = match tenant {
            Some(t) => {
                let rows: Vec<Value> = block_on(p.db().query(Stmt::new(
                    "SELECT mode FROM tenants WHERE id = ?1",
                    vec![t.into()],
                )))
                .unwrap();
                if rows[0]["mode"] == "test" {
                    ck::Mode::Test
                } else {
                    ck::Mode::Live
                }
            }
            None => ck::Mode::Live,
        };
        let token = ck::generate(
            mode,
            pylota_identity_platform::Rng::bytes(&p.rng),
            pylota_identity_platform::Rng::bytes(&p.rng),
        );
        let lookup = ck::parse(&token).unwrap().lookup.to_owned();
        let id = pylota_identity_platform::Ids::new_id(
            &p.ids,
            pylota_identity_platform::ids::IdPrefix::ApiKey,
        );
        let hash = ck::hash(&p.secrets().key_pepper, &token);
        block_on(p.db().execute(Stmt::new(
            "INSERT INTO api_keys (id, lookup, hash, name, level, partner_id, tenant_id, identity_id, mode, permissions_json, created_by_role, created_by_level, created_at) VALUES (?1, ?2, ?3, 'seed', ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'setup', 1)",
            vec![
                id.into(), lookup.into(), hash.into(), level.into(), partner.map(str::to_owned).into(), tenant.map(str::to_owned).into(),
                identity.map(str::to_owned).into(), mode.as_str().into(), json!(permissions).to_string().into(), role.map(str::to_owned).into(),
            ],
        )))
        .unwrap();
        token
    }

    /// A platform key with every platform-level permission.
    pub fn platform_key(&self) -> String {
        self.seed_key(
            "platform",
            &[
                "tenants:manage",
                "partners:manage",
                "keys:manage",
                "audit:read",
                "platform:ops",
                "policy:write",
                "identities:read",
                "identities:write",
                "erasure:manage",
                "domains:read",
            ],
            None,
            None,
            None,
            None,
        )
    }

    pub fn sql(
        &self,
        sql: &'static str,
        params: Vec<pylota_identity_platform::d1::SqlValue>,
    ) -> Vec<Value> {
        block_on(self.app.p.db().query(Stmt::new(sql, params))).unwrap()
    }
}
