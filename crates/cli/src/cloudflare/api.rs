//! The Cloudflare calls of `setup`, `doctor` and `domains subscribe`, each a lookup or a create, so a
//! re-run finds what an earlier run made ([CLI and setup §6.3]). Endpoints and fields as cited there.
//!
//! [CLI and setup §6.3]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/cli.md#63-steps

use serde_json::{Value, json};

use super::{Client, d1_body, unexpected};
use crate::CliError;

/// `Ok(None)` for a `404`.
fn absent(r: Result<Value, CliError>) -> Result<Option<Value>, CliError> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(CliError::Cloudflare {
            status: Some(404), ..
        }) => Ok(None),
        Err(e) => Err(e),
    }
}

impl Client<'_> {
    /// The zone named exactly `name` in the account.
    pub async fn zone(&self, name: &str) -> Result<Option<Value>, CliError> {
        let zones = self
            .list(
                "zones",
                "/zones",
                &[
                    ("name", name.to_owned()),
                    ("account.id", self.account.clone()),
                ],
            )
            .await?;
        Ok(zones.into_iter().find(|z| z["name"] == name))
    }

    /// The zone `domain` belongs to: itself or its longest parent that is a zone in the account.
    pub async fn zone_of(&self, domain: &str) -> Result<Option<Value>, CliError> {
        let mut d = domain;
        loop {
            if let Some(z) = self.zone(d).await? {
                return Ok(Some(z));
            }
            match d.split_once('.') {
                Some((_, rest)) if rest.contains('.') => d = rest,
                _ => return Ok(None),
            }
        }
    }

    pub async fn dns_records(
        &self,
        zone: &str,
        rtype: &str,
        name: &str,
    ) -> Result<Vec<Value>, CliError> {
        self.list(
            "dns records",
            &format!("/zones/{zone}/dns_records"),
            &[("type", rtype.to_owned()), ("name.exact", name.to_owned())],
        )
        .await
    }

    pub async fn d1_find(&self, name: &str) -> Result<Option<Value>, CliError> {
        let dbs = self
            .list(
                "D1 database",
                &self.acct("/d1/database"),
                &[("name", name.to_owned())],
            )
            .await?;
        Ok(dbs.into_iter().find(|d| d["name"] == name))
    }

    pub async fn d1_create(&self, name: &str, jurisdiction: &str) -> Result<Value, CliError> {
        let mut body = json!({ "name": name });
        if jurisdiction != "default" {
            body["jurisdiction"] = json!(jurisdiction);
        }
        self.post("D1 database", &self.acct("/d1/database"), &body)
            .await
    }

    /// One statement through the D1 query API: its rows.
    pub async fn d1_query(
        &self,
        db: &str,
        sql: &str,
        params: &[Value],
    ) -> Result<Vec<Value>, CliError> {
        let r = self
            .post(
                "D1 query",
                &self.acct(&format!("/d1/database/{db}/query")),
                &d1_body(sql, params),
            )
            .await?;
        let first = r
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null);
        if first["success"] == false {
            return Err(unexpected(
                "D1 query",
                first["error"].as_str().unwrap_or("the statement failed"),
            ));
        }
        Ok(first["results"].as_array().cloned().unwrap_or_default())
    }

    /// A bucket in a jurisdiction (`None` when it does not exist there).
    pub async fn r2_bucket(
        &self,
        name: &str,
        jurisdiction: &str,
    ) -> Result<Option<Value>, CliError> {
        let path = self.acct(&format!("/r2/buckets/{name}"));
        absent(
            self.call(
                "R2 bucket",
                reqwest::Method::GET,
                &path,
                &[],
                None,
                &[("cf-r2-jurisdiction", jurisdiction)],
            )
            .await
            .map(|r| r.0),
        )
    }

    pub async fn r2_create(&self, name: &str, jurisdiction: &str) -> Result<Value, CliError> {
        let path = self.acct("/r2/buckets");
        Ok(self
            .call(
                "R2 bucket",
                reqwest::Method::POST,
                &path,
                &[],
                Some(&json!({ "name": name })),
                &[("cf-r2-jurisdiction", jurisdiction)],
            )
            .await?
            .0)
    }

    pub async fn r2_lifecycle(
        &self,
        name: &str,
        jurisdiction: &str,
    ) -> Result<Vec<Value>, CliError> {
        let path = self.acct(&format!("/r2/buckets/{name}/lifecycle"));
        let r = self
            .call(
                "R2 lifecycle",
                reqwest::Method::GET,
                &path,
                &[],
                None,
                &[("cf-r2-jurisdiction", jurisdiction)],
            )
            .await?
            .0;
        Ok(r["rules"].as_array().cloned().unwrap_or_default())
    }

    pub async fn r2_put_lifecycle(
        &self,
        name: &str,
        jurisdiction: &str,
        rules: &[Value],
    ) -> Result<(), CliError> {
        let path = self.acct(&format!("/r2/buckets/{name}/lifecycle"));
        self.call(
            "R2 lifecycle",
            reqwest::Method::PUT,
            &path,
            &[],
            Some(&json!({ "rules": rules })),
            &[("cf-r2-jurisdiction", jurisdiction)],
        )
        .await?;
        Ok(())
    }

    pub async fn queues(&self) -> Result<Vec<Value>, CliError> {
        self.list("queues", &self.acct("/queues"), &[]).await
    }

    pub async fn queue_create(&self, name: &str) -> Result<Value, CliError> {
        self.post(
            "queues",
            &self.acct("/queues"),
            &json!({ "queue_name": name }),
        )
        .await
    }

    pub async fn vectorize_index(&self, name: &str) -> Result<Option<Value>, CliError> {
        absent(
            self.get(
                "Vectorize index",
                &self.acct(&format!("/vectorize/v2/indexes/{name}")),
                &[],
            )
            .await,
        )
    }

    pub async fn vectorize_create(
        &self,
        name: &str,
        description: &str,
        dimensions: u32,
    ) -> Result<Value, CliError> {
        self.post(
            "Vectorize index",
            &self.acct("/vectorize/v2/indexes"),
            &json!({ "name": name, "description": description,
                     "config": { "dimensions": dimensions, "metric": "cosine" } }),
        )
        .await
    }

    pub async fn vectorize_metadata(&self, name: &str) -> Result<Vec<Value>, CliError> {
        let r = self
            .get(
                "Vectorize metadata",
                &self.acct(&format!("/vectorize/v2/indexes/{name}/metadata_index/list")),
                &[],
            )
            .await?;
        Ok(r["metadataIndexes"].as_array().cloned().unwrap_or_default())
    }

    pub async fn vectorize_metadata_create(
        &self,
        name: &str,
        property: &str,
        kind: &str,
    ) -> Result<(), CliError> {
        self.post(
            "Vectorize metadata",
            &self.acct(&format!(
                "/vectorize/v2/indexes/{name}/metadata_index/create"
            )),
            &json!({ "propertyName": property, "indexType": kind }),
        )
        .await?;
        Ok(())
    }

    pub async fn routing(&self, zone: &str) -> Result<Value, CliError> {
        self.get(
            "Email Routing",
            &format!("/zones/{zone}/email/routing"),
            &[],
        )
        .await
    }

    pub async fn routing_dns(&self, zone: &str) -> Result<Vec<Value>, CliError> {
        let r = self
            .get(
                "Email Routing DNS",
                &format!("/zones/{zone}/email/routing/dns"),
                &[],
            )
            .await?;
        Ok(match r {
            Value::Array(a) => a,
            other => other["record"].as_array().cloned().unwrap_or_default(),
        })
    }

    pub async fn catch_all(&self, zone: &str) -> Result<Value, CliError> {
        self.get(
            "catch-all",
            &format!("/zones/{zone}/email/routing/rules/catch_all"),
            &[],
        )
        .await
    }

    pub async fn sending_subdomains(&self, zone: &str) -> Result<Vec<Value>, CliError> {
        self.list(
            "Email Sending",
            &format!("/zones/{zone}/email/sending/subdomains"),
            &[],
        )
        .await
    }

    pub async fn sending_dns(&self, zone: &str, tag: &str) -> Result<Vec<Value>, CliError> {
        let r = self
            .get(
                "Email Sending DNS",
                &format!("/zones/{zone}/email/sending/subdomains/{tag}/dns"),
                &[],
            )
            .await?;
        Ok(r.as_array().cloned().unwrap_or_default())
    }

    pub async fn event_subscriptions(&self) -> Result<Vec<Value>, CliError> {
        self.list(
            "event subscriptions",
            &self.acct("/event_subscriptions/subscriptions"),
            &[],
        )
        .await
    }

    /// The names of a Worker's secrets (their values are write-only).
    pub async fn secret_names(&self, script: &str) -> Result<Option<Vec<String>>, CliError> {
        let r = absent(
            self.get(
                "Worker secrets",
                &self.acct(&format!("/workers/scripts/{script}/secrets")),
                &[],
            )
            .await,
        )?;
        Ok(r.map(|v| {
            v.as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| s["name"].as_str().map(str::to_owned))
                .collect()
        }))
    }

    pub async fn scripts(&self) -> Result<Vec<Value>, CliError> {
        let r = self
            .get("Workers scripts", &self.acct("/workers/scripts"), &[])
            .await?;
        Ok(r.as_array().cloned().unwrap_or_default())
    }

    pub async fn script_settings(&self, script: &str) -> Result<Value, CliError> {
        self.get(
            "Workers settings",
            &self.acct(&format!("/workers/scripts/{script}/settings")),
            &[],
        )
        .await
    }
}
