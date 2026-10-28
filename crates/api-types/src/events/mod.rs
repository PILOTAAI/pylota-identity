//! Webhook and event-feed event payloads ([events]).
//!
//! [events]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/events.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Every event type. New types can be added within `api_version`; handle unknown ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum EventType {
    #[serde(rename = "message.received")]
    MessageReceived,
    #[serde(rename = "message.quarantined")]
    MessageQuarantined,
    #[serde(rename = "message.released")]
    MessageReleased,
    #[serde(rename = "message.triaged")]
    MessageTriaged,
    #[serde(rename = "message.screened")]
    MessageScreened,
    #[serde(rename = "message.sent")]
    MessageSent,
    #[serde(rename = "message.delivered")]
    MessageDelivered,
    #[serde(rename = "message.deferred")]
    MessageDeferred,
    #[serde(rename = "message.bounced")]
    MessageBounced,
    #[serde(rename = "message.complained")]
    MessageComplained,
    #[serde(rename = "message.rejected")]
    MessageRejected,
    #[serde(rename = "message.failed")]
    MessageFailed,
    #[serde(rename = "message.uncertain")]
    MessageUncertain,
    #[serde(rename = "message.reconciled")]
    MessageReconciled,
    #[serde(rename = "message.suppressed")]
    MessageSuppressed,
    #[serde(rename = "message.canceled")]
    MessageCanceled,
    #[serde(rename = "message.labels_changed")]
    MessageLabelsChanged,
    #[serde(rename = "verification.received")]
    VerificationReceived,
    #[serde(rename = "calendar.received")]
    CalendarReceived,
    #[serde(rename = "draft.approval_requested")]
    DraftApprovalRequested,
    #[serde(rename = "draft.approved")]
    DraftApproved,
    #[serde(rename = "draft.rejected")]
    DraftRejected,
    #[serde(rename = "draft.sent")]
    DraftSent,
    #[serde(rename = "draft.failed")]
    DraftFailed,
    #[serde(rename = "identity.created")]
    IdentityCreated,
    #[serde(rename = "identity.updated")]
    IdentityUpdated,
    #[serde(rename = "identity.paused")]
    IdentityPaused,
    #[serde(rename = "identity.resumed")]
    IdentityResumed,
    #[serde(rename = "identity.deleted")]
    IdentityDeleted,
    #[serde(rename = "identity.address_added")]
    IdentityAddressAdded,
    #[serde(rename = "identity.address_activated")]
    IdentityAddressActivated,
    #[serde(rename = "identity.address_promoted")]
    IdentityAddressPromoted,
    #[serde(rename = "identity.address_retired")]
    IdentityAddressRetired,
    #[serde(rename = "identity.key_created")]
    IdentityKeyCreated,
    #[serde(rename = "identity.key_rotated")]
    IdentityKeyRotated,
    #[serde(rename = "identity.key_revoked")]
    IdentityKeyRevoked,
    #[serde(rename = "domain.created")]
    DomainCreated,
    #[serde(rename = "domain.verified")]
    DomainVerified,
    #[serde(rename = "domain.degraded")]
    DomainDegraded,
    #[serde(rename = "domain.failing")]
    DomainFailing,
    #[serde(rename = "domain.suspended")]
    DomainSuspended,
    #[serde(rename = "domain.recovered")]
    DomainRecovered,
    #[serde(rename = "domain.reminder")]
    DomainReminder,
    #[serde(rename = "domain.removed")]
    DomainRemoved,
    #[serde(rename = "erasure.completed")]
    ErasureCompleted,
    #[serde(rename = "erasure.failed")]
    ErasureFailed,
    #[serde(rename = "export.completed")]
    ExportCompleted,
    #[serde(rename = "suppression.created")]
    SuppressionCreated,
    #[serde(rename = "quota.warning")]
    QuotaWarning,
    #[serde(rename = "webhook.disabled")]
    WebhookDisabled,
    #[serde(rename = "webhook.test")]
    WebhookTest,
    #[serde(rename = "member.invited")]
    MemberInvited,
    #[serde(rename = "member.joined")]
    MemberJoined,
    #[serde(rename = "member.role_changed")]
    MemberRoleChanged,
    #[serde(rename = "member.removed")]
    MemberRemoved,
    #[serde(rename = "billing.plan_changed")]
    BillingPlanChanged,
    #[serde(rename = "billing.payment_failed")]
    BillingPaymentFailed,
    #[serde(rename = "billing.limit_reached")]
    BillingLimitReached,
    #[serde(rename = "tenant.policy_updated")]
    TenantPolicyUpdated,
    #[serde(rename = "signup.verified")]
    SignupVerified,
    #[serde(rename = "account.requested")]
    AccountRequested,
    #[serde(rename = "account.approved")]
    AccountApproved,
    #[serde(rename = "account.rejected")]
    AccountRejected,
    #[serde(rename = "account.closed")]
    AccountClosed,
    #[serde(rename = "sms.received")]
    SmsReceived,
    #[serde(rename = "sms.quarantined")]
    SmsQuarantined,
    #[serde(rename = "sms.released")]
    SmsReleased,
    #[serde(rename = "sms.screened")]
    SmsScreened,
    #[serde(rename = "sms.verification_received")]
    SmsVerificationReceived,
    #[serde(rename = "sms.sent")]
    SmsSent,
    #[serde(rename = "sms.delivered")]
    SmsDelivered,
    #[serde(rename = "sms.failed")]
    SmsFailed,
    #[serde(rename = "sms.delivery_unconfirmed")]
    SmsDeliveryUnconfirmed,
    #[serde(rename = "sms.uncertain")]
    SmsUncertain,
    #[serde(rename = "sms.reconciled")]
    SmsReconciled,
    #[serde(rename = "call.started")]
    CallStarted,
    #[serde(rename = "call.ended")]
    CallEnded,
    #[serde(rename = "phone_request.requested")]
    PhoneRequestRequested,
    #[serde(rename = "phone_request.approved")]
    PhoneRequestApproved,
    #[serde(rename = "phone_request.rejected")]
    PhoneRequestRejected,
    #[serde(rename = "phone_request.canceled")]
    PhoneRequestCanceled,
    #[serde(rename = "phone_request.completed")]
    PhoneRequestCompleted,
    #[serde(rename = "phone_request.failed")]
    PhoneRequestFailed,
    #[serde(rename = "phone_request.uncertain")]
    PhoneRequestUncertain,
    #[serde(rename = "phone_number.status_changed")]
    PhoneNumberStatusChanged,
    #[serde(rename = "phone_number.capability_changed")]
    PhoneNumberCapabilityChanged,
    #[serde(rename = "phone_number.expiring")]
    PhoneNumberExpiring,
    #[serde(rename = "phone_number.action_needed")]
    PhoneNumberActionNeeded,
    #[serde(rename = "phone_number.port_out_requested")]
    PhoneNumberPortOutRequested,
    #[serde(rename = "phone_number.port_out_decided")]
    PhoneNumberPortOutDecided,
    #[serde(rename = "phone_registration.status_changed")]
    PhoneRegistrationStatusChanged,
    #[serde(rename = "phone_compliance.status_changed")]
    PhoneComplianceStatusChanged,
    #[serde(rename = "phone_consent.changed")]
    PhoneConsentChanged,
    #[serde(rename = "phone_spend.warning")]
    PhoneSpendWarning,
    #[serde(rename = "phone_setup.status_changed")]
    PhoneSetupStatusChanged,
    #[serde(rename = "phone_budget.changed")]
    PhoneBudgetChanged,
    #[serde(rename = "vault.item.created")]
    VaultItemCreated,
    #[serde(rename = "vault.item.updated")]
    VaultItemUpdated,
    #[serde(rename = "vault.item.rotated")]
    VaultItemRotated,
    #[serde(rename = "vault.item.deleted")]
    VaultItemDeleted,
    #[serde(rename = "vault.item.revealed")]
    VaultItemRevealed,
    #[serde(rename = "vault.item.health_changed")]
    VaultItemHealthChanged,
    #[serde(rename = "vault.grant.requested")]
    VaultGrantRequested,
    #[serde(rename = "vault.grant.approved")]
    VaultGrantApproved,
    #[serde(rename = "vault.grant.denied")]
    VaultGrantDenied,
    #[serde(rename = "vault.grant.revoked")]
    VaultGrantRevoked,
    #[serde(rename = "vault.grant.expired")]
    VaultGrantExpired,
    #[serde(rename = "vault.use.requested")]
    VaultUseRequested,
    #[serde(rename = "vault.use.approval_requested")]
    VaultUseApprovalRequested,
    #[serde(rename = "vault.use.approved")]
    VaultUseApproved,
    #[serde(rename = "vault.use.denied")]
    VaultUseDenied,
    #[serde(rename = "vault.use.expired")]
    VaultUseExpired,
    #[serde(rename = "vault.use.canceled")]
    VaultUseCanceled,
    #[serde(rename = "vault.use.closed")]
    VaultUseClosed,
    #[serde(rename = "vault.ticket.issued")]
    VaultTicketIssued,
    #[serde(rename = "vault.ticket.redeemed")]
    VaultTicketRedeemed,
    #[serde(rename = "vault.ticket.refused")]
    VaultTicketRefused,
    #[serde(rename = "vault.ticket.reported")]
    VaultTicketReported,
    #[serde(rename = "vault.consumer.changed")]
    VaultConsumerChanged,
    #[serde(rename = "vault.key.changed")]
    VaultKeyChanged,
}

/// Any webhook event. `type` selects the payload schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum Event {
    MessageReceivedEvent(MessageReceivedEvent),
    MessageQuarantinedEvent(MessageQuarantinedEvent),
    MessageReleasedEvent(MessageReleasedEvent),
    MessageTriagedEvent(MessageTriagedEvent),
    MessageScreenedEvent(MessageScreenedEvent),
    MessageSentEvent(MessageSentEvent),
    MessageDeliveredEvent(MessageDeliveredEvent),
    MessageDeferredEvent(MessageDeferredEvent),
    MessageBouncedEvent(MessageBouncedEvent),
    MessageComplainedEvent(MessageComplainedEvent),
    MessageRejectedEvent(MessageRejectedEvent),
    MessageFailedEvent(MessageFailedEvent),
    MessageUncertainEvent(MessageUncertainEvent),
    MessageReconciledEvent(MessageReconciledEvent),
    MessageSuppressedEvent(MessageSuppressedEvent),
    MessageCanceledEvent(MessageCanceledEvent),
    MessageLabelsChangedEvent(MessageLabelsChangedEvent),
    VerificationReceivedEvent(VerificationReceivedEvent),
    CalendarReceivedEvent(CalendarReceivedEvent),
    DraftApprovalRequestedEvent(DraftApprovalRequestedEvent),
    DraftApprovedEvent(DraftApprovedEvent),
    DraftRejectedEvent(DraftRejectedEvent),
    DraftSentEvent(DraftSentEvent),
    DraftFailedEvent(DraftFailedEvent),
    IdentityCreatedEvent(IdentityCreatedEvent),
    IdentityUpdatedEvent(IdentityUpdatedEvent),
    IdentityPausedEvent(IdentityPausedEvent),
    IdentityResumedEvent(IdentityResumedEvent),
    IdentityDeletedEvent(IdentityDeletedEvent),
    IdentityAddressAddedEvent(IdentityAddressAddedEvent),
    IdentityAddressActivatedEvent(IdentityAddressActivatedEvent),
    IdentityAddressPromotedEvent(IdentityAddressPromotedEvent),
    IdentityAddressRetiredEvent(IdentityAddressRetiredEvent),
    IdentityKeyCreatedEvent(IdentityKeyCreatedEvent),
    IdentityKeyRotatedEvent(IdentityKeyRotatedEvent),
    IdentityKeyRevokedEvent(IdentityKeyRevokedEvent),
    DomainCreatedEvent(DomainCreatedEvent),
    DomainVerifiedEvent(DomainVerifiedEvent),
    DomainDegradedEvent(DomainDegradedEvent),
    DomainFailingEvent(DomainFailingEvent),
    DomainSuspendedEvent(DomainSuspendedEvent),
    DomainRecoveredEvent(DomainRecoveredEvent),
    DomainReminderEvent(DomainReminderEvent),
    DomainRemovedEvent(DomainRemovedEvent),
    ErasureCompletedEvent(ErasureCompletedEvent),
    ErasureFailedEvent(ErasureFailedEvent),
    ExportCompletedEvent(ExportCompletedEvent),
    SuppressionCreatedEvent(SuppressionCreatedEvent),
    QuotaWarningEvent(QuotaWarningEvent),
    WebhookDisabledEvent(WebhookDisabledEvent),
    WebhookTestEvent(WebhookTestEvent),
    MemberInvitedEvent(MemberInvitedEvent),
    MemberJoinedEvent(MemberJoinedEvent),
    MemberRoleChangedEvent(MemberRoleChangedEvent),
    MemberRemovedEvent(MemberRemovedEvent),
    BillingPlanChangedEvent(BillingPlanChangedEvent),
    BillingPaymentFailedEvent(BillingPaymentFailedEvent),
    BillingLimitReachedEvent(BillingLimitReachedEvent),
    TenantPolicyUpdatedEvent(TenantPolicyUpdatedEvent),
    SignupVerifiedEvent(SignupVerifiedEvent),
    AccountRequestedEvent(AccountRequestedEvent),
    AccountApprovedEvent(AccountApprovedEvent),
    AccountRejectedEvent(AccountRejectedEvent),
    AccountClosedEvent(AccountClosedEvent),
    SmsReceivedEvent(SmsReceivedEvent),
    SmsQuarantinedEvent(SmsQuarantinedEvent),
    SmsReleasedEvent(SmsReleasedEvent),
    SmsScreenedEvent(SmsScreenedEvent),
    SmsVerificationReceivedEvent(SmsVerificationReceivedEvent),
    SmsSentEvent(SmsSentEvent),
    SmsDeliveredEvent(SmsDeliveredEvent),
    SmsFailedEvent(SmsFailedEvent),
    SmsDeliveryUnconfirmedEvent(SmsDeliveryUnconfirmedEvent),
    SmsUncertainEvent(SmsUncertainEvent),
    SmsReconciledEvent(SmsReconciledEvent),
    CallStartedEvent(CallStartedEvent),
    CallEndedEvent(CallEndedEvent),
    PhoneRequestRequestedEvent(PhoneRequestRequestedEvent),
    PhoneRequestApprovedEvent(PhoneRequestApprovedEvent),
    PhoneRequestRejectedEvent(PhoneRequestRejectedEvent),
    PhoneRequestCanceledEvent(PhoneRequestCanceledEvent),
    PhoneRequestCompletedEvent(PhoneRequestCompletedEvent),
    PhoneRequestFailedEvent(PhoneRequestFailedEvent),
    PhoneRequestUncertainEvent(PhoneRequestUncertainEvent),
    PhoneNumberStatusChangedEvent(PhoneNumberStatusChangedEvent),
    PhoneNumberCapabilityChangedEvent(PhoneNumberCapabilityChangedEvent),
    PhoneNumberExpiringEvent(PhoneNumberExpiringEvent),
    PhoneNumberActionNeededEvent(PhoneNumberActionNeededEvent),
    PhoneNumberPortOutRequestedEvent(PhoneNumberPortOutRequestedEvent),
    PhoneNumberPortOutDecidedEvent(PhoneNumberPortOutDecidedEvent),
    PhoneRegistrationStatusChangedEvent(PhoneRegistrationStatusChangedEvent),
    PhoneComplianceStatusChangedEvent(PhoneComplianceStatusChangedEvent),
    PhoneConsentChangedEvent(PhoneConsentChangedEvent),
    PhoneSpendWarningEvent(PhoneSpendWarningEvent),
    PhoneSetupStatusChangedEvent(PhoneSetupStatusChangedEvent),
    PhoneBudgetChangedEvent(PhoneBudgetChangedEvent),
    VaultItemCreatedEvent(VaultItemCreatedEvent),
    VaultItemUpdatedEvent(VaultItemUpdatedEvent),
    VaultItemRotatedEvent(VaultItemRotatedEvent),
    VaultItemDeletedEvent(VaultItemDeletedEvent),
    VaultItemRevealedEvent(VaultItemRevealedEvent),
    VaultItemHealthChangedEvent(VaultItemHealthChangedEvent),
    VaultGrantRequestedEvent(VaultGrantRequestedEvent),
    VaultGrantApprovedEvent(VaultGrantApprovedEvent),
    VaultGrantDeniedEvent(VaultGrantDeniedEvent),
    VaultGrantRevokedEvent(VaultGrantRevokedEvent),
    VaultGrantExpiredEvent(VaultGrantExpiredEvent),
    VaultUseRequestedEvent(VaultUseRequestedEvent),
    VaultUseApprovalRequestedEvent(VaultUseApprovalRequestedEvent),
    VaultUseApprovedEvent(VaultUseApprovedEvent),
    VaultUseDeniedEvent(VaultUseDeniedEvent),
    VaultUseExpiredEvent(VaultUseExpiredEvent),
    VaultUseCanceledEvent(VaultUseCanceledEvent),
    VaultUseClosedEvent(VaultUseClosedEvent),
    VaultTicketIssuedEvent(VaultTicketIssuedEvent),
    VaultTicketRedeemedEvent(VaultTicketRedeemedEvent),
    VaultTicketRefusedEvent(VaultTicketRefusedEvent),
    VaultTicketReportedEvent(VaultTicketReportedEvent),
    VaultConsumerChangedEvent(VaultConsumerChangedEvent),
    VaultKeyChangedEvent(VaultKeyChangedEvent),
}

/// `message.received`: An inbound message is stored and visible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageReceivedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageReceivedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageReceivedData"}, "type": {"enum": ["message.received"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.received`: An inbound message is stored and visible."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageReceivedEvent {}

/// `message.quarantined`: An inbound message is stored but quarantined.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageQuarantinedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageQuarantinedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageQuarantinedData"}, "type": {"enum": ["message.quarantined"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.quarantined`: An inbound message is stored but quarantined."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageQuarantinedEvent {}

/// `message.released`: A quarantined message was released.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageReleasedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageReleasedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageReleasedData"}, "type": {"enum": ["message.released"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.released`: A quarantined message was released."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageReleasedEvent {}

/// `message.triaged`: Triage finished (or failed).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageTriagedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageTriagedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageTriagedData"}, "type": {"enum": ["message.triaged"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.triaged`: Triage finished (or failed)."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageTriagedEvent {}

/// `message.screened`: A screening verdict was reached, or changed by a late signal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageScreenedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageScreenedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageScreenedData"}, "type": {"enum": ["message.screened"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.screened`: A screening verdict was reached, or changed by a late signal."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageScreenedEvent {}

/// `message.sent`: The transport accepted an outbound message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageSentEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageSentEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageSentData"}, "type": {"enum": ["message.sent"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.sent`: The transport accepted an outbound message."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageSentEvent {}

/// `message.delivered`: A recipient's server accepted the message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageDeliveredEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageDeliveredEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageDeliveredData"}, "type": {"enum": ["message.delivered"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.delivered`: A recipient's server accepted the message."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageDeliveredEvent {}

/// `message.deferred`: A temporary failure; the provider is retrying.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageDeferredEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageDeferredEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageDeferredData"}, "type": {"enum": ["message.deferred"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.deferred`: A temporary failure; the provider is retrying."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageDeferredEvent {}

/// `message.bounced`: A permanent failure, or retries exhausted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageBouncedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageBouncedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageBouncedData"}, "type": {"enum": ["message.bounced"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.bounced`: A permanent failure, or retries exhausted."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageBouncedEvent {}

/// `message.complained`: A recipient reported spam.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageComplainedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageComplainedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageComplainedData"}, "type": {"enum": ["message.complained"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.complained`: A recipient reported spam."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageComplainedEvent {}

/// `message.rejected`: The transport refused the message before sending.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageRejectedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageRejectedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageRejectedData"}, "type": {"enum": ["message.rejected"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.rejected`: The transport refused the message before sending."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageRejectedEvent {}

/// `message.failed`: The message could not be sent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageFailedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageFailedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageFailedData"}, "type": {"enum": ["message.failed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.failed`: The message could not be sent."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageFailedEvent {}

/// `message.uncertain`: The outcome is unknown; the message is never resent automatically.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageUncertainEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageUncertainEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageUncertainData"}, "type": {"enum": ["message.uncertain"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.uncertain`: The outcome is unknown; the message is never resent automatically."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageUncertainEvent {}

/// `message.reconciled`: An uncertain send was matched to a provider event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageReconciledEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageReconciledEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageReconciledData"}, "type": {"enum": ["message.reconciled"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.reconciled`: An uncertain send was matched to a provider event."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageReconciledEvent {}

/// `message.suppressed`: Every recipient is suppressed; nothing was sent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageSuppressedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageSuppressedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageSuppressedData"}, "type": {"enum": ["message.suppressed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.suppressed`: Every recipient is suppressed; nothing was sent."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageSuppressedEvent {}

/// `message.canceled`: The message was cancelled while queued.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageCanceledEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageCanceledEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageCanceledData"}, "type": {"enum": ["message.canceled"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.canceled`: The message was cancelled while queued."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageCanceledEvent {}

/// `message.labels_changed`: (opt-in) labels of visible messages changed through the API, MCP or the console.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageLabelsChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MessageLabelsChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MessageLabelsChangedData"}, "type": {"enum": ["message.labels_changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`message.labels_changed`: (opt-in) labels of visible messages changed through the API, MCP or the console."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MessageLabelsChangedEvent {}

/// `verification.received`: A verification code or link was found in authenticated mail. The value itself is only available through `wait`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerificationReceivedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VerificationReceivedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VerificationReceivedData"}, "type": {"enum": ["verification.received"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`verification.received`: A verification code or link was found in authenticated mail. The value itself is only available through `wait`."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VerificationReceivedEvent {}

/// `calendar.received`: A message with a calendar object became visible as received. Read the object with `getMessage`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalendarReceivedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for CalendarReceivedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/CalendarReceivedData"}, "type": {"enum": ["calendar.received"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`calendar.received`: A message with a calendar object became visible as received. Read the object with `getMessage`."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for CalendarReceivedEvent {}

/// `draft.approval_requested`: A draft waits for approval, or was edited while waiting, or a send found approval needed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftApprovalRequestedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DraftApprovalRequestedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DraftApprovalRequestedData"}, "type": {"enum": ["draft.approval_requested"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`draft.approval_requested`: A draft waits for approval, or was edited while waiting, or a send found approval needed."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DraftApprovalRequestedEvent {}

/// `draft.approved`: An approver approved a revision of a draft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftApprovedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DraftApprovedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DraftApprovedData"}, "type": {"enum": ["draft.approved"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`draft.approved`: An approver approved a revision of a draft."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DraftApprovedEvent {}

/// `draft.rejected`: An approver rejected a draft, or its approval request expired.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftRejectedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DraftRejectedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DraftRejectedData"}, "type": {"enum": ["draft.rejected"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`draft.rejected`: An approver rejected a draft, or its approval request expired."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DraftRejectedEvent {}

/// `draft.sent`: A draft became a message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftSentEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DraftSentEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DraftSentData"}, "type": {"enum": ["draft.sent"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`draft.sent`: A draft became a message."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DraftSentEvent {}

/// `draft.failed`: A scheduled or approved send of a draft ended without a message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftFailedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DraftFailedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DraftFailedData"}, "type": {"enum": ["draft.failed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`draft.failed`: A scheduled or approved send of a draft ended without a message."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DraftFailedEvent {}

/// `identity.created`: An identity was created.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityCreatedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for IdentityCreatedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/IdentityCreatedData"}, "type": {"enum": ["identity.created"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`identity.created`: An identity was created."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for IdentityCreatedEvent {}

/// `identity.updated`: An identity was updated.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityUpdatedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for IdentityUpdatedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/IdentityUpdatedData"}, "type": {"enum": ["identity.updated"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`identity.updated`: An identity was updated."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for IdentityUpdatedEvent {}

/// `identity.paused`: An identity was paused (manually, for abuse, or because its tenant was suspended).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityPausedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for IdentityPausedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/IdentityPausedData"}, "type": {"enum": ["identity.paused"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`identity.paused`: An identity was paused (manually, for abuse, or because its tenant was suspended)."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for IdentityPausedEvent {}

/// `identity.resumed`: A paused identity was resumed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityResumedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for IdentityResumedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/IdentityResumedData"}, "type": {"enum": ["identity.resumed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`identity.resumed`: A paused identity was resumed."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for IdentityResumedEvent {}

/// `identity.deleted`: An identity was deleted: emitted once, when the identity-scope erasure that deletes it completes and the identity becomes `deleted` (after held threads are released, if any). It comes from the erasure job with `identity_id` set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityDeletedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for IdentityDeletedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/IdentityDeletedData"}, "type": {"enum": ["identity.deleted"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`identity.deleted`: An identity was deleted: emitted once, when the identity-scope erasure that deletes it completes and the identity becomes `deleted` (after held threads are released, if any). It comes from the erasure job with `identity_id` set."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for IdentityDeletedEvent {}

/// `identity.address_added`: An address was added to an identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityAddressAddedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for IdentityAddressAddedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/IdentityAddressData"}, "type": {"enum": ["identity.address_added"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`identity.address_added`: An address was added to an identity."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for IdentityAddressAddedEvent {}

/// `identity.address_activated`: A pending address became active once its domain was healthy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityAddressActivatedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for IdentityAddressActivatedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/IdentityAddressData"}, "type": {"enum": ["identity.address_activated"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`identity.address_activated`: A pending address became active once its domain was healthy."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for IdentityAddressActivatedEvent {}

/// `identity.address_promoted`: An address became the identity's primary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityAddressPromotedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for IdentityAddressPromotedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/IdentityAddressPromotedData"}, "type": {"enum": ["identity.address_promoted"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`identity.address_promoted`: An address became the identity's primary."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for IdentityAddressPromotedEvent {}

/// `identity.address_retired`: An address became retired.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityAddressRetiredEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for IdentityAddressRetiredEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/IdentityAddressData"}, "type": {"enum": ["identity.address_retired"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`identity.address_retired`: An address became retired."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for IdentityAddressRetiredEvent {}

/// `identity.key_created`: A signing key was created for an identity that had no active key, lazily by a signing request or by `POST …/keys`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityKeyCreatedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for IdentityKeyCreatedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/IdentityKeyEventData"}, "type": {"enum": ["identity.key_created"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`identity.key_created`: A signing key was created for an identity that had no active key, lazily by a signing request or by `POST …/keys`."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for IdentityKeyCreatedEvent {}

/// `identity.key_rotated`: An identity signing key was rotated: `kid` is the new active key, `previous_kid` the key now `retiring`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityKeyRotatedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for IdentityKeyRotatedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/IdentityKeyRotatedData"}, "type": {"enum": ["identity.key_rotated"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`identity.key_rotated`: An identity signing key was rotated: `kid` is the new active key, `previous_kid` the key now `retiring`."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for IdentityKeyRotatedEvent {}

/// `identity.key_revoked`: An identity signing key was revoked and is `retired`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdentityKeyRevokedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for IdentityKeyRevokedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/IdentityKeyEventData"}, "type": {"enum": ["identity.key_revoked"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`identity.key_revoked`: An identity signing key was revoked and is `retired`."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for IdentityKeyRevokedEvent {}

/// `domain.created`: A domain was added.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomainCreatedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DomainCreatedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DomainEventData"}, "type": {"enum": ["domain.created"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`domain.created`: A domain was added."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DomainCreatedEvent {}

/// `domain.verified`: A domain became healthy for the first time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomainVerifiedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DomainVerifiedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DomainEventData"}, "type": {"enum": ["domain.verified"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`domain.verified`: A domain became healthy for the first time."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DomainVerifiedEvent {}

/// `domain.degraded`: A domain became degraded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomainDegradedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DomainDegradedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DomainDegradedData"}, "type": {"enum": ["domain.degraded"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`domain.degraded`: A domain became degraded."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DomainDegradedEvent {}

/// `domain.failing`: A domain started failing; sending falls back to platform addresses unless disabled by policy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomainFailingEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DomainFailingEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DomainFailingData"}, "type": {"enum": ["domain.failing"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`domain.failing`: A domain started failing; sending falls back to platform addresses unless disabled by policy."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DomainFailingEvent {}

/// `domain.suspended`: A domain was suspended; ownership must be re-proved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomainSuspendedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DomainSuspendedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DomainSuspendedData"}, "type": {"enum": ["domain.suspended"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`domain.suspended`: A domain was suspended; ownership must be re-proved."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DomainSuspendedEvent {}

/// `domain.recovered`: A domain returned to healthy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomainRecoveredEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DomainRecoveredEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DomainRecoveredData"}, "type": {"enum": ["domain.recovered"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`domain.recovered`: A domain returned to healthy."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DomainRecoveredEvent {}

/// `domain.reminder`: A domain is still unhealthy (sent at 24 h, 72 h and 7 days in the state).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomainReminderEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DomainReminderEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DomainReminderData"}, "type": {"enum": ["domain.reminder"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`domain.reminder`: A domain is still unhealthy (sent at 24 h, 72 h and 7 days in the state)."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DomainReminderEvent {}

/// `domain.removed`: A domain was removed (`reason`: `requested`, `zone_expired`, `evicted` or `unverified_expired`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomainRemovedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for DomainRemovedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/DomainRemovedData"}, "type": {"enum": ["domain.removed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`domain.removed`: A domain was removed (`reason`: `requested`, `zone_expired`, `evicted` or `unverified_expired`)."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for DomainRemovedEvent {}

/// `erasure.completed`: An erasure request completed, with its receipt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErasureCompletedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for ErasureCompletedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/ErasureCompletedData"}, "type": {"enum": ["erasure.completed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`erasure.completed`: An erasure request completed, with its receipt."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for ErasureCompletedEvent {}

/// `erasure.failed`: An erasure step failed after the job runner's own retries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErasureFailedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for ErasureFailedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/ErasureFailedData"}, "type": {"enum": ["erasure.failed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`erasure.failed`: An erasure step failed after the job runner's own retries."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for ErasureFailedEvent {}

/// `export.completed`: A subject-access export is ready. Fetch the download link from the API.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportCompletedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for ExportCompletedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/ExportCompletedData"}, "type": {"enum": ["export.completed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`export.completed`: A subject-access export is ready. Fetch the download link from the API."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for ExportCompletedEvent {}

/// `suppression.created`: An address was suppressed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SuppressionCreatedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SuppressionCreatedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SuppressionCreatedData"}, "type": {"enum": ["suppression.created"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`suppression.created`: An address was suppressed."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SuppressionCreatedEvent {}

/// `quota.warning`: A quota reached 80% or 100% of its limit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuotaWarningEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for QuotaWarningEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/QuotaWarningData"}, "type": {"enum": ["quota.warning"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`quota.warning`: A quota reached 80% or 100% of its limit."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for QuotaWarningEvent {}

/// `webhook.disabled`: A webhook endpoint was disabled. Sent to platform endpoints and, when the disabled endpoint belongs to a partner (a partner endpoint, or an endpoint of a tenant a partner's key created), to that partner's other endpoints; never to tenant endpoints or to the endpoint itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebhookDisabledEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for WebhookDisabledEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/WebhookDisabledData"}, "type": {"enum": ["webhook.disabled"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`webhook.disabled`: A webhook endpoint was disabled. Sent to platform endpoints and, when the disabled endpoint belongs to a partner (a partner endpoint, or an endpoint of a tenant a partner's key created), to that partner's other endpoints; never to tenant endpoints or to the endpoint itself."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for WebhookDisabledEvent {}

/// `webhook.test`: A test event sent by `POST /v1/webhooks/{webhook_id}/test`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebhookTestEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for WebhookTestEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/WebhookTestData"}, "type": {"enum": ["webhook.test"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`webhook.test`: A test event sent by `POST /v1/webhooks/{webhook_id}/test`."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for WebhookTestEvent {}

/// `member.invited`: An invitation to the workspace was created or re-sent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemberInvitedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MemberInvitedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MemberInvitedData"}, "type": {"enum": ["member.invited"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`member.invited`: An invitation to the workspace was created or re-sent."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MemberInvitedEvent {}

/// `member.joined`: An invitation was accepted; the user is now a member.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemberJoinedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MemberJoinedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MemberJoinedData"}, "type": {"enum": ["member.joined"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`member.joined`: An invitation was accepted; the user is now a member."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MemberJoinedEvent {}

/// `member.role_changed`: A member's role changed (an ownership transfer sends two).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemberRoleChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MemberRoleChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MemberRoleChangedData"}, "type": {"enum": ["member.role_changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`member.role_changed`: A member's role changed (an ownership transfer sends two)."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MemberRoleChangedEvent {}

/// `member.removed`: A member was removed, or left.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemberRemovedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for MemberRemovedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/MemberRemovedData"}, "type": {"enum": ["member.removed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`member.removed`: A member was removed, or left."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for MemberRemovedEvent {}

/// `billing.plan_changed`: The workspace's plan changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BillingPlanChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for BillingPlanChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/BillingPlanChangedData"}, "type": {"enum": ["billing.plan_changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`billing.plan_changed`: The workspace's plan changed."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for BillingPlanChangedEvent {}

/// `billing.payment_failed`: A payment failed; the workspace keeps its plan until `grace_until`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BillingPaymentFailedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for BillingPaymentFailedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/BillingPaymentFailedData"}, "type": {"enum": ["billing.payment_failed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`billing.payment_failed`: A payment failed; the workspace keeps its plan until `grace_until`."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for BillingPaymentFailedEvent {}

/// `billing.limit_reached`: A plan allowance is spent. Sent once per feature per period, when the first `402 billing_limit` is returned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BillingLimitReachedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for BillingLimitReachedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/BillingLimitReachedData"}, "type": {"enum": ["billing.limit_reached"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`billing.limit_reached`: A plan allowance is spent. Sent once per feature per period, when the first `402 billing_limit` is returned."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for BillingLimitReachedEvent {}

/// `tenant.policy_updated`: A tenant's policy was changed (any route, any writer, except a tenant's creation).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TenantPolicyUpdatedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for TenantPolicyUpdatedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/TenantPolicyUpdatedData"}, "type": {"enum": ["tenant.policy_updated"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`tenant.policy_updated`: A tenant's policy was changed (any route, any writer, except a tenant's creation)."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for TenantPolicyUpdatedEvent {}

/// `signup.verified`: A provisional agent sign-up was verified by its human.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignupVerifiedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SignupVerifiedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SignupVerifiedData"}, "type": {"enum": ["signup.verified"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`signup.verified`: A provisional agent sign-up was verified by its human."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SignupVerifiedEvent {}

/// `account.requested`: An agent recorded that it wants an account at a third-party service.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountRequestedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for AccountRequestedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/AccountEventData"}, "type": {"enum": ["account.requested"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`account.requested`: An agent recorded that it wants an account at a third-party service."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for AccountRequestedEvent {}

/// `account.approved`: An operator approved a service-ledger entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountApprovedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for AccountApprovedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/AccountEventData"}, "type": {"enum": ["account.approved"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`account.approved`: An operator approved a service-ledger entry."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for AccountApprovedEvent {}

/// `account.rejected`: A service-ledger entry was rejected by an operator, or expired after 7 days undecided.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountRejectedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for AccountRejectedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/AccountRejectedData"}, "type": {"enum": ["account.rejected"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`account.rejected`: A service-ledger entry was rejected by an operator, or expired after 7 days undecided."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for AccountRejectedEvent {}

/// `account.closed`: A service-ledger entry was closed, or deleted while pending or approved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountClosedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for AccountClosedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/AccountEventData"}, "type": {"enum": ["account.closed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`account.closed`: A service-ledger entry was closed, or deleted while pending or approved."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for AccountClosedEvent {}

/// `sms.received`: An inbound SMS is stored and visible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmsReceivedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SmsReceivedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SmsReceivedData"}, "type": {"enum": ["sms.received"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`sms.received`: An inbound SMS is stored and visible."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SmsReceivedEvent {}

/// `sms.quarantined`: An inbound SMS is stored but held. `sms.text` is absent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmsQuarantinedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SmsQuarantinedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SmsQuarantinedData"}, "type": {"enum": ["sms.quarantined"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`sms.quarantined`: An inbound SMS is stored but held. `sms.text` is absent."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SmsQuarantinedEvent {}

/// `sms.released`: A held SMS was released.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmsReleasedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SmsReleasedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SmsReleasedData"}, "type": {"enum": ["sms.released"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`sms.released`: A held SMS was released."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SmsReleasedEvent {}

/// `sms.screened`: A screening verdict on an SMS.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmsScreenedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SmsScreenedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SmsScreenedData"}, "type": {"enum": ["sms.screened"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`sms.screened`: A screening verdict on an SMS."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SmsScreenedEvent {}

/// `sms.verification_received`: A verification code was found in a visible SMS. The value is only available through `GET …/sms/wait`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmsVerificationReceivedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SmsVerificationReceivedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SmsVerificationReceivedData"}, "type": {"enum": ["sms.verification_received"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`sms.verification_received`: A verification code was found in a visible SMS. The value is only available through `GET …/sms/wait`."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SmsVerificationReceivedEvent {}

/// `sms.sent`: The provider accepted the SMS.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmsSentEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SmsSentEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SmsSentData"}, "type": {"enum": ["sms.sent"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`sms.sent`: The provider accepted the SMS."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SmsSentEvent {}

/// `sms.delivered`: The handset or carrier confirmed delivery.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmsDeliveredEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SmsDeliveredEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SmsDeliveredData"}, "type": {"enum": ["sms.delivered"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`sms.delivered`: The handset or carrier confirmed delivery."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SmsDeliveredEvent {}

/// `sms.failed`: The SMS was refused or not delivered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmsFailedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SmsFailedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SmsFailedData"}, "type": {"enum": ["sms.failed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`sms.failed`: The SMS was refused or not delivered."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SmsFailedEvent {}

/// `sms.delivery_unconfirmed`: The carrier gave no final answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmsDeliveryUnconfirmedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SmsDeliveryUnconfirmedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SmsDeliveryUnconfirmedData"}, "type": {"enum": ["sms.delivery_unconfirmed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`sms.delivery_unconfirmed`: The carrier gave no final answer."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SmsDeliveryUnconfirmedEvent {}

/// `sms.uncertain`: The outcome is unknown; it is never resent automatically.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmsUncertainEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SmsUncertainEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SmsUncertainData"}, "type": {"enum": ["sms.uncertain"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`sms.uncertain`: The outcome is unknown; it is never resent automatically."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SmsUncertainEvent {}

/// `sms.reconciled`: An uncertain SMS was matched to a provider record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmsReconciledEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for SmsReconciledEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/SmsReconciledData"}, "type": {"enum": ["sms.reconciled"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`sms.reconciled`: An uncertain SMS was matched to a provider record."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for SmsReconciledEvent {}

/// `call.started`: A call on one of the identity's numbers began.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CallStartedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for CallStartedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/CallEventData"}, "type": {"enum": ["call.started"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`call.started`: A call on one of the identity's numbers began."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for CallStartedEvent {}

/// `call.ended`: A call ended, or a call record filled it in later.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CallEndedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for CallEndedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/CallEventData"}, "type": {"enum": ["call.ended"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`call.ended`: A call ended, or a call record filled it in later."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for CallEndedEvent {}

/// `phone_request.requested`: A phone request was created, or returned to `pending_approval` because its price rose.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneRequestRequestedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneRequestRequestedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneRequestRequestedData"}, "type": {"enum": ["phone_request.requested"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_request.requested`: A phone request was created, or returned to `pending_approval` because its price rose."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneRequestRequestedEvent {}

/// `phone_request.approved`: A person, an allowed key or the tenant's standing budget approved a phone request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneRequestApprovedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneRequestApprovedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneRequestApprovedData"}, "type": {"enum": ["phone_request.approved"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_request.approved`: A person, an allowed key or the tenant's standing budget approved a phone request."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneRequestApprovedEvent {}

/// `phone_request.rejected`: A phone request was rejected, or expired after 7 days.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneRequestRejectedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneRequestRejectedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneRequestRejectedData"}, "type": {"enum": ["phone_request.rejected"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_request.rejected`: A phone request was rejected, or expired after 7 days."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneRequestRejectedEvent {}

/// `phone_request.canceled`: A phone request was canceled by the requester, or by erasure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneRequestCanceledEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneRequestCanceledEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneRequestCanceledData"}, "type": {"enum": ["phone_request.canceled"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_request.canceled`: A phone request was canceled by the requester, or by erasure."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneRequestCanceledEvent {}

/// `phone_request.completed`: The provider work of a phone request ended successfully.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneRequestCompletedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneRequestCompletedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneRequestCompletedData"}, "type": {"enum": ["phone_request.completed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_request.completed`: The provider work of a phone request ended successfully."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneRequestCompletedEvent {}

/// `phone_request.failed`: The provider work of a phone request ended without success.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneRequestFailedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneRequestFailedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneRequestFailedData"}, "type": {"enum": ["phone_request.failed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_request.failed`: The provider work of a phone request ended without success."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneRequestFailedEvent {}

/// `phone_request.uncertain`: A billable provider call's outcome is unknown; the service is reconciling.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneRequestUncertainEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneRequestUncertainEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneRequestUncertainData"}, "type": {"enum": ["phone_request.uncertain"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_request.uncertain`: A billable provider call's outcome is unknown; the service is reconciling."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneRequestUncertainEvent {}

/// `phone_number.status_changed`: A number changed status.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneNumberStatusChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneNumberStatusChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneNumberStatusChangedData"}, "type": {"enum": ["phone_number.status_changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_number.status_changed`: A number changed status."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneNumberStatusChangedEvent {}

/// `phone_number.capability_changed`: One of a number's capability facets changed state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneNumberCapabilityChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneNumberCapabilityChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneNumberCapabilityChangedData"}, "type": {"enum": ["phone_number.capability_changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_number.capability_changed`: One of a number's capability facets changed state."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneNumberCapabilityChangedEvent {}

/// `phone_number.expiring`: A number bought with `expires_at` will be released within 24 hours.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneNumberExpiringEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneNumberExpiringEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneNumberExpiringData"}, "type": {"enum": ["phone_number.expiring"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_number.expiring`: A number bought with `expires_at` will be released within 24 hours."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneNumberExpiringEvent {}

/// `phone_number.action_needed`: The provider needs information before a deadline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneNumberActionNeededEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneNumberActionNeededEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneNumberActionNeededData"}, "type": {"enum": ["phone_number.action_needed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_number.action_needed`: The provider needs information before a deadline."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneNumberActionNeededEvent {}

/// `phone_number.port_out_requested`: Another carrier asked for the number.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneNumberPortOutRequestedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneNumberPortOutRequestedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneNumberPortOutRequestedData"}, "type": {"enum": ["phone_number.port_out_requested"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_number.port_out_requested`: Another carrier asked for the number."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneNumberPortOutRequestedEvent {}

/// `phone_number.port_out_decided`: A person authorised or rejected a port-out, or the provider authorised it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneNumberPortOutDecidedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneNumberPortOutDecidedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneNumberPortOutDecidedData"}, "type": {"enum": ["phone_number.port_out_decided"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_number.port_out_decided`: A person authorised or rejected a port-out, or the provider authorised it."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneNumberPortOutDecidedEvent {}

/// `phone_registration.status_changed`: A US 10DLC brand, campaign or number assignment, or a toll-free verification, changed status.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneRegistrationStatusChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneRegistrationStatusChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneRegistrationStatusChangedData"}, "type": {"enum": ["phone_registration.status_changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_registration.status_changed`: A US 10DLC brand, campaign or number assignment, or a toll-free verification, changed status."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneRegistrationStatusChangedEvent {}

/// `phone_compliance.status_changed`: The compliance profile's status for a country, number type and action changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneComplianceStatusChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneComplianceStatusChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneComplianceStatusChangedData"}, "type": {"enum": ["phone_compliance.status_changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_compliance.status_changed`: The compliance profile's status for a country, number type and action changed."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneComplianceStatusChangedEvent {}

/// `phone_consent.changed`: A counterparty opted out or in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneConsentChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneConsentChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneConsentChangedData"}, "type": {"enum": ["phone_consent.changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_consent.changed`: A counterparty opted out or in."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneConsentChangedEvent {}

/// `phone_spend.warning`: Phone spend reached 80% or 100% of a monthly cap.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneSpendWarningEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneSpendWarningEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneSpendWarningData"}, "type": {"enum": ["phone_spend.warning"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_spend.warning`: Phone spend reached 80% or 100% of a monthly cap."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneSpendWarningEvent {}

/// `phone_setup.status_changed`: The tenant's onboarding status for a country changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneSetupStatusChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneSetupStatusChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneSetupStatusChangedData"}, "type": {"enum": ["phone_setup.status_changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_setup.status_changed`: The tenant's onboarding status for a country changed."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneSetupStatusChangedEvent {}

/// `phone_budget.changed`: The tenant's standing purchase budget was set, loosened, tightened or turned off.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhoneBudgetChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for PhoneBudgetChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/PhoneBudgetChangedData"}, "type": {"enum": ["phone_budget.changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`phone_budget.changed`: The tenant's standing purchase budget was set, loosened, tightened or turned off."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for PhoneBudgetChangedEvent {}

/// `vault.item.created`: A vault item was created or imported.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultItemCreatedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultItemCreatedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultItemEventData"}, "type": {"enum": ["vault.item.created"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.item.created`: A vault item was created or imported."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultItemCreatedEvent {}

/// `vault.item.updated`: A vault item's metadata, origins, approval level or state changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultItemUpdatedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultItemUpdatedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultItemEventData"}, "type": {"enum": ["vault.item.updated"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.item.updated`: A vault item's metadata, origins, approval level or state changed."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultItemUpdatedEvent {}

/// `vault.item.rotated`: A new version of a vault item became current.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultItemRotatedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultItemRotatedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultItemEventData"}, "type": {"enum": ["vault.item.rotated"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.item.rotated`: A new version of a vault item became current."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultItemRotatedEvent {}

/// `vault.item.deleted`: A vault item was crypto-shredded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultItemDeletedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultItemDeletedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultItemDeletedData"}, "type": {"enum": ["vault.item.deleted"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.item.deleted`: A vault item was crypto-shredded."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultItemDeletedEvent {}

/// `vault.item.revealed`: A person revealed a value in the console, or was shown a new managed value once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultItemRevealedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultItemRevealedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultItemRevealedData"}, "type": {"enum": ["vault.item.revealed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.item.revealed`: A person revealed a value in the console, or was shown a new managed value once."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultItemRevealedEvent {}

/// `vault.item.health_changed`: A vault item's health changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultItemHealthChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultItemHealthChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultItemHealthChangedData"}, "type": {"enum": ["vault.item.health_changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.item.health_changed`: A vault item's health changed."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultItemHealthChangedEvent {}

/// `vault.grant.requested`: An agent asked for a vault grant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultGrantRequestedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultGrantRequestedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultGrantEventData"}, "type": {"enum": ["vault.grant.requested"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.grant.requested`: An agent asked for a vault grant."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultGrantRequestedEvent {}

/// `vault.grant.approved`: A vault grant was approved, or created approved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultGrantApprovedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultGrantApprovedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultGrantEventData"}, "type": {"enum": ["vault.grant.approved"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.grant.approved`: A vault grant was approved, or created approved."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultGrantApprovedEvent {}

/// `vault.grant.denied`: A vault grant was denied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultGrantDeniedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultGrantDeniedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultGrantEventData"}, "type": {"enum": ["vault.grant.denied"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.grant.denied`: A vault grant was denied."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultGrantDeniedEvent {}

/// `vault.grant.revoked`: A vault grant was revoked, or given up by its agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultGrantRevokedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultGrantRevokedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultGrantEventData"}, "type": {"enum": ["vault.grant.revoked"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.grant.revoked`: A vault grant was revoked, or given up by its agent."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultGrantRevokedEvent {}

/// `vault.grant.expired`: A vault grant's pending week or active period ended.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultGrantExpiredEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultGrantExpiredEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultGrantEventData"}, "type": {"enum": ["vault.grant.expired"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.grant.expired`: A vault grant's pending week or active period ended."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultGrantExpiredEvent {}

/// `vault.use.requested`: The orchestrator created a vault use (also for refusals at creation).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultUseRequestedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultUseRequestedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultUseEventData"}, "type": {"enum": ["vault.use.requested"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.use.requested`: The orchestrator created a vault use (also for refusals at creation)."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultUseRequestedEvent {}

/// `vault.use.approval_requested`: A person must decide on a vault use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultUseApprovalRequestedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultUseApprovalRequestedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultUseEventData"}, "type": {"enum": ["vault.use.approval_requested"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.use.approval_requested`: A person must decide on a vault use."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultUseApprovalRequestedEvent {}

/// `vault.use.approved`: A vault use was approved; mint a ticket now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultUseApprovedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultUseApprovedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultUseEventData"}, "type": {"enum": ["vault.use.approved"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.use.approved`: A vault use was approved; mint a ticket now."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultUseApprovedEvent {}

/// `vault.use.denied`: A vault use was denied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultUseDeniedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultUseDeniedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultUseEventData"}, "type": {"enum": ["vault.use.denied"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.use.denied`: A vault use was denied."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultUseDeniedEvent {}

/// `vault.use.expired`: A vault use's approval deadline or first-redemption window passed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultUseExpiredEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultUseExpiredEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultUseEventData"}, "type": {"enum": ["vault.use.expired"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.use.expired`: A vault use's approval deadline or first-redemption window passed."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultUseExpiredEvent {}

/// `vault.use.canceled`: The orchestrator canceled a vault use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultUseCanceledEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultUseCanceledEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultUseEventData"}, "type": {"enum": ["vault.use.canceled"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.use.canceled`: The orchestrator canceled a vault use."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultUseCanceledEvent {}

/// `vault.use.closed`: A vault use closed, normally or early.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultUseClosedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultUseClosedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultUseEventData"}, "type": {"enum": ["vault.use.closed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.use.closed`: A vault use closed, normally or early."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultUseClosedEvent {}

/// `vault.ticket.issued`: A vault ticket was minted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultTicketIssuedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultTicketIssuedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultTicketIssuedData"}, "type": {"enum": ["vault.ticket.issued"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.ticket.issued`: A vault ticket was minted."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultTicketIssuedEvent {}

/// `vault.ticket.redeemed`: A consumer redeemed a phase.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultTicketRedeemedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultTicketRedeemedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultTicketRedeemedData"}, "type": {"enum": ["vault.ticket.redeemed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.ticket.redeemed`: A consumer redeemed a phase."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultTicketRedeemedEvent {}

/// `vault.ticket.refused`: A redemption was refused.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultTicketRefusedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultTicketRefusedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultTicketRefusedData"}, "type": {"enum": ["vault.ticket.refused"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.ticket.refused`: A redemption was refused."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultTicketRefusedEvent {}

/// `vault.ticket.reported`: A consumer reported an outcome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultTicketReportedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultTicketReportedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultTicketReportedData"}, "type": {"enum": ["vault.ticket.reported"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.ticket.reported`: A consumer reported an outcome."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultTicketReportedEvent {}

/// `vault.consumer.changed`: A vault consumer was registered, activated, suspended, resumed, re-keyed or revoked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultConsumerChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultConsumerChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultConsumerChangedData"}, "type": {"enum": ["vault.consumer.changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.consumer.changed`: A vault consumer was registered, activated, suspended, resumed, re-keyed or revoked."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultConsumerChangedEvent {}

/// `vault.key.changed`: The tenant's vault key rotated, custody changed, or `revoke-all` ran.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VaultKeyChangedEvent {
    #[serde(flatten)]
    pub base: EventBase,
}

impl utoipa::PartialSchema for VaultKeyChangedEvent {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        serde_json::from_str(r###"{"allOf": [{"$ref": "#/components/schemas/EventBase"}, {"properties": {"data": {"$ref": "#/components/schemas/VaultKeyChangedData"}, "type": {"enum": ["vault.key.changed"], "type": "string"}}, "required": ["type", "data"], "type": "object"}], "description": "`vault.key.changed`: The tenant's vault key rotated, custody changed, or `revoke-all` ran."}"###).expect("the contract's schema")
    }
}

impl utoipa::ToSchema for VaultKeyChangedEvent {}
