//! Objects first used by the `members` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Member {
    pub user_id: UserId,
    pub email: EmailAddress,
    #[schema(required = true)]
    pub name: Option<String>,
    pub role: MemberRole,
    /// The person's last console sign-in, from `users.last_login_at`; `null` before the first.
    #[schema(required = true)]
    pub last_login_at: Option<String>,
    pub created_at: String,
}

/// A pending invitation. It uses a seat until it is accepted, revoked or expires.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Invitation {
    pub id: InvitationId,
    pub email: EmailAddress,
    pub role: InvitationRole,
    /// The console user who sent it, or `null` when an API key created it.
    #[schema(required = true)]
    pub invited_by: Option<UserId>,
    pub expires_at: String,
}

/// Members plus pending invitations count as used seats.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MemberListSeats {
    /// `null` when unlimited.
    #[schema(required = true)]
    pub granted: Option<i64>,
    pub used: i64,
}

fn schema_member_list_seats_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Members plus pending invitations count as used seats.", "properties": {"granted": {"description": "`null` when unlimited.", "minimum": 0, "type": ["integer", "null"]}, "used": {"minimum": 0, "type": "integer"}}, "required": ["granted", "used"], "type": "object"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MemberList {
    pub data: Vec<Member>,
    /// Pending invitations.
    pub invitations: Vec<Invitation>,
    /// Members plus pending invitations count as used seats.
    #[schema(schema_with = schema_member_list_seats_property)]
    pub seats: MemberListSeats,
}
