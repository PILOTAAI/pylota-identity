//! Request bodies of the `platform` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Default)]
pub enum JobCreateRequestKind {
    #[serde(rename = "reparse")]
    #[default]
    Reparse,
    #[serde(rename = "reembed")]
    Reembed,
    #[serde(rename = "reindex")]
    Reindex,
    #[serde(rename = "restore_reconcile")]
    RestoreReconcile,
    #[serde(rename = "event_log_rebuild")]
    EventLogRebuild,
}

fn schema_job_create_request_kind_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`restore_reconcile` needs `after` and `identity_ids`. `event_log_rebuild` takes `tenant_id` only.", "enum": ["reparse", "reembed", "reindex", "restore_reconcile", "event_log_rebuild"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct JobCreateRequest {
    /// `restore_reconcile` needs `after` and `identity_ids`. `event_log_rebuild` takes `tenant_id` only.
    #[schema(schema_with = schema_job_create_request_kind_property)]
    pub kind: JobCreateRequestKind,
    pub tenant_id: TenantId,
    /// Only these identities. `null` or omitted means every identity of the tenant.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<IdentityId>>)]
    pub identity_ids: Option<Option<Vec<IdentityId>>>,
    /// Only messages at or after this time.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub after: Option<Option<String>>,
    /// Only messages before this time.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub before: Option<Option<String>>,
}

/// Exactly one of `at` and `bookmark`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum MailboxRestoreRequest {
    Variant1 {
        /// The point in time, within the last 30 days.
        at: String,
    },
    Variant2 {
        /// An undo bookmark returned by an earlier restore.
        bookmark: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema, Default)]
pub struct WaitlistInviteRequest {
    /// How many entries to invite, oldest confirmed first.
    pub count: i64,
    /// Invite only entries whose plan of interest is this plan. `null` or omitted means any plan.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub plan: Option<Option<String>>,
}
