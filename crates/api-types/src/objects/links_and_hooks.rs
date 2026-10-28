//! Objects first used by the `links_and_hooks` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum SnsMessageSignatureVersion {
    #[serde(rename = "2")]
    V2,
}

/// An Amazon SNS HTTP(S) message, as SNS sends it. Only `SubscriptionConfirmation` and `Notification` messages for the endpoint's own topic are acted on: `PM_SES_SNS_TOPIC_ARN` for `/hooks/ses`, `PM_SES_INBOUND_TOPIC_ARN` for `/hooks/ses/inbound`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnsMessage {
    /// Every other entry.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
    #[serde(rename = "Type")]
    pub r#type: String,
    #[serde(rename = "MessageId")]
    pub message_id: String,
    #[serde(rename = "TopicArn")]
    pub topic_arn: String,
    #[serde(rename = "Subject")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// For a notification, the SES event (`/hooks/ses`) or the SES receipt notification (`/hooks/ses/inbound`) as a JSON string.
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "Timestamp")]
    pub timestamp: String,
    /// Must be `2` (SHA256withRSA). Version `1` (SHA1withRSA) is refused with `403 invalid_signature`.
    #[serde(rename = "SignatureVersion")]
    pub signature_version: SnsMessageSignatureVersion,
    #[serde(rename = "Signature")]
    pub signature: String,
    /// Must be `https` on the host `sns.{PM_SES_REGION}.amazonaws.com`.
    #[serde(rename = "SigningCertURL")]
    pub signing_cert_url: String,
    /// `SubscriptionConfirmation` only.
    #[serde(rename = "SubscribeURL")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscribe_url: Option<String>,
    /// `SubscriptionConfirmation` only.
    #[serde(rename = "Token")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(rename = "UnsubscribeURL")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsubscribe_url: Option<String>,
}

impl utoipa::PartialSchema for SnsMessage {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"additionalProperties": true, "description": "An Amazon SNS HTTP(S) message, as SNS sends it. Only `SubscriptionConfirmation` and `Notification` messages for the endpoint's own topic are acted on: `PM_SES_SNS_TOPIC_ARN` for `/hooks/ses`, `PM_SES_INBOUND_TOPIC_ARN` for `/hooks/ses/inbound`.", "properties": {"Message": {"description": "For a notification, the SES event (`/hooks/ses`) or the SES receipt notification (`/hooks/ses/inbound`) as a JSON string.", "type": "string"}, "MessageId": {"type": "string"}, "Signature": {"type": "string"}, "SignatureVersion": {"description": "Must be `2` (SHA256withRSA). Version `1` (SHA1withRSA) is refused with `403 invalid_signature`.", "enum": ["2"], "type": "string"}, "SigningCertURL": {"description": "Must be `https` on the host `sns.{PM_SES_REGION}.amazonaws.com`.", "format": "uri", "type": "string"}, "Subject": {"type": "string"}, "SubscribeURL": {"description": "`SubscriptionConfirmation` only.", "format": "uri", "type": "string"}, "Timestamp": {"format": "date-time", "type": "string"}, "Token": {"description": "`SubscriptionConfirmation` only.", "type": "string"}, "TopicArn": {"type": "string"}, "Type": {"examples": ["Notification", "SubscriptionConfirmation"], "type": "string"}, "UnsubscribeURL": {"format": "uri", "type": "string"}}, "required": ["Type", "MessageId", "TopicArn", "Message", "Timestamp", "SignatureVersion", "Signature", "SigningCertURL"], "type": "object"}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SnsMessage {}
