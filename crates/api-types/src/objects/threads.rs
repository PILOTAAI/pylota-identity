//! Objects first used by the `threads` operations of the [REST API].
//!
//! [REST API]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/api.md

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::*;

/// Attachment ID (`att_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AttachmentId(pub String);

/// An address with its display name. Display names are untrusted content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MailboxAddress {
    pub address: EmailAddress,
    /// The display name, or an empty string when there is none.
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum Direction {
    #[serde(rename = "inbound")]
    Inbound,
    #[serde(rename = "outbound")]
    Outbound,
}

/// A legal hold. Retention and erasure skip the thread while it is set (FR-PRV-4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ThreadHold {
    pub reason: String,
    /// When the hold lapses. `null` holds until removed.
    #[schema(required = true)]
    pub until: Option<String>,
    /// The key that set the hold.
    #[schema(required = true)]
    pub set_by: Option<KeyId>,
    pub set_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ThreadSummary {
    pub id: ThreadId,
    /// The normalised subject of the first message (untrusted content).
    pub subject: String,
    /// Everyone on the thread, capped at 50.
    pub participants: Vec<MailboxAddress>,
    pub message_count: i64,
    pub unread_count: i64,
    pub first_at: String,
    pub last_at: String,
    /// When the latest inbound message arrived. Use it to detect new mail before approving a draft ([C5]).
    #[schema(required = true)]
    pub last_inbound_at: Option<String>,
    pub last_direction: Direction,
    /// Up to 240 characters of the latest message's `extracted_text` (untrusted content).
    pub snippet: String,
    /// Labels on any message in the thread.
    pub labels: Vec<Label>,
    /// The triage category of the latest triaged inbound message.
    #[schema(required = true)]
    pub category: Option<String>,
    #[schema(required = true)]
    pub needs_reply: Option<f64>,
    #[schema(required = true)]
    pub urgency: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub archived: Option<bool>,
    #[schema(required = true)]
    pub hold: Option<ThreadHold>,
}

/// A thread summary with a page of its messages, oldest first within the page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Thread {
    #[serde(flatten)]
    pub base: ThreadSummary,
    pub messages: Vec<Message>,
    /// Cursor for the next page of messages, or `null` on the last page.
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

/// Inbound: `normal`, `automated`, `dsn`, `list`, `calendar` or `mdn`. Outbound: `transactional`, `marketing` or `auto_reply`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageKind {
    #[serde(rename = "normal")]
    Normal,
    #[serde(rename = "automated")]
    Automated,
    #[serde(rename = "dsn")]
    Dsn,
    #[serde(rename = "list")]
    List,
    #[serde(rename = "calendar")]
    Calendar,
    #[serde(rename = "mdn")]
    Mdn,
    #[serde(rename = "transactional")]
    Transactional,
    #[serde(rename = "marketing")]
    Marketing,
    #[serde(rename = "auto_reply")]
    AutoReply,
}

/// `sent_via_fallback` (sent from the platform address because the domain was failing), `parse_degraded`, `encrypted`, `message_id_conflict`, `reprocessed`, `reconciled`, `bcc` (the identity was BCC'd), `loopback` (delivered inside the deployment for a test tenant, [L3]) and `body_truncated` (a stored body was cut at its storage cap; the full message is in the raw MIME), `sender_suppressed` (the sender is on the tenant's suppression list; the message is stored as usual, [D7]), `body_redacted` (links and codes in a system-identity message were replaced by `[link]` and `[code]`, [A15]) and `dsn_untrusted` (a delivery report that could not be tied to a send of ours by a trusted path, so it changed no delivery, [D11]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageFlag {
    #[serde(rename = "sent_via_fallback")]
    SentViaFallback,
    #[serde(rename = "parse_degraded")]
    ParseDegraded,
    #[serde(rename = "encrypted")]
    Encrypted,
    #[serde(rename = "message_id_conflict")]
    MessageIdConflict,
    #[serde(rename = "reprocessed")]
    Reprocessed,
    #[serde(rename = "reconciled")]
    Reconciled,
    #[serde(rename = "bcc")]
    Bcc,
    #[serde(rename = "loopback")]
    Loopback,
    #[serde(rename = "body_truncated")]
    BodyTruncated,
    #[serde(rename = "sender_suppressed")]
    SenderSuppressed,
    #[serde(rename = "body_redacted")]
    BodyRedacted,
    #[serde(rename = "dsn_untrusted")]
    DsnUntrusted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum QuarantineReason {
    #[serde(rename = "auth_failed")]
    AuthFailed,
    #[serde(rename = "auth_unverified")]
    AuthUnverified,
    #[serde(rename = "spam")]
    Spam,
    #[serde(rename = "risky_attachment")]
    RiskyAttachment,
    #[serde(rename = "blocked_sender")]
    BlockedSender,
    #[serde(rename = "otp_unsolicited")]
    OtpUnsolicited,
    #[serde(rename = "account_unapproved")]
    AccountUnapproved,
    #[serde(rename = "steering_suspected")]
    SteeringSuspected,
    #[serde(rename = "screening_unavailable")]
    ScreeningUnavailable,
}

/// One header field as received, in order (untrusted content).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageHeader {
    pub name: String,
    pub value: String,
}

/// A message. Every text field (subject, display names, filenames, bodies) is untrusted content: show it to a model inside a clearly delimited block, never as instructions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Message {
    pub id: MessageId,
    pub thread_id: ThreadId,
    pub identity_id: IdentityId,
    pub direction: Direction,
    pub status: MessageStatus,
    /// The first parseable `From` mailbox, or `null` when there is none ([B13]).
    #[schema(required = true)]
    pub from: Option<MailboxAddress>,
    pub to: Vec<MailboxAddress>,
    pub cc: Vec<MailboxAddress>,
    /// Outbound only. Always empty for inbound messages.
    pub bcc: Vec<MailboxAddress>,
    pub reply_to: Vec<MailboxAddress>,
    /// Inbound only. The envelope recipient this copy was delivered to.
    #[schema(required = true)]
    pub delivered_to: Option<EmailAddress>,
    /// Inbound only. `true` on exactly one copy when one message reached several identities of the tenant; act only on that copy ([A9]).
    #[schema(required = true)]
    pub is_primary_recipient: Option<bool>,
    #[schema(required = true)]
    pub subject: Option<String>,
    /// The `Date` header, or the submit time for outbound messages.
    #[schema(required = true)]
    pub sent_at: Option<String>,
    /// When the service stored the message (its own clock).
    pub received_at: String,
    /// The new content, with quoted history, signatures and hidden text removed.
    #[schema(required = true)]
    pub extracted_text: Option<String>,
    /// The full plain text (derived from HTML when the message has none). Included with `include=quoted`, otherwise `null`.
    #[schema(required = true)]
    pub text: Option<String>,
    /// Sanitised HTML, never rendered by the service. Included with `include=html`, otherwise `null`.
    #[schema(required = true)]
    pub html: Option<String>,
    /// The original header fields. Present only with `include=headers`; `null` there when the raw MIME they are read from is gone (past `retention.raw_days`, or an outbound message whose sent copy was deleted).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<Vec<MessageHeader>>)]
    pub headers: Option<Option<Vec<MessageHeader>>>,
    pub attachments: Vec<Attachment>,
    pub labels: Vec<Label>,
    pub kind: MessageKind,
    /// Authentication and trust metadata of an inbound message. `null` for outbound messages.
    #[schema(required = true)]
    pub trust: Option<Trust>,
    /// Triage of an inbound message. `null` for outbound messages, and for a quarantined message until it is released (triage then runs).
    #[schema(required = true)]
    pub triage: Option<Triage>,
    /// The screening record, or `null` when the message was not screened. `status: "screening"` means the message is held until its verdict, at the latest at `screening.deadline_at`.
    #[schema(required = true)]
    pub screening: Option<Screening>,
    /// The message's calendar object, or `null`. Every string in it is untrusted content.
    #[schema(required = true)]
    pub calendar: Option<MessageCalendar>,
    /// Set when `status` is `quarantined`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<QuarantineReason>)]
    pub quarantine_reason: Option<Option<QuarantineReason>>,
    /// Exact references extracted at ingest (FR-SRCH-4).
    pub refs: Vec<Ref>,
    /// The `Message-ID` header, normalised, without angle brackets.
    #[schema(required = true)]
    pub rfc_message_id: Option<String>,
    /// The `In-Reply-To` header, normalised, without angle brackets.
    #[schema(required = true)]
    pub in_reply_to: Option<String>,
    /// The draft an outbound message was sent from, or `null`.
    #[schema(required = true)]
    pub draft_id: Option<DraftId>,
    /// Outbound only. The transport's message ID, set once the message is `submitted`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub provider_message_id: Option<Option<String>>,
    /// Outbound only. Per-recipient delivery status; `null` for inbound messages.
    #[schema(required = true)]
    pub deliveries: Option<Vec<Delivery>>,
    pub flags: Vec<MessageFlag>,
    pub metadata: Metadata,
}

/// Why an attachment is unsafe ([B10]). A message with a risky attachment is quarantined.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AttachmentRisk {
    #[serde(rename = "executable")]
    Executable,
    #[serde(rename = "macro")]
    Macro,
    #[serde(rename = "encrypted_archive")]
    EncryptedArchive,
    #[serde(rename = "archive_bomb")]
    ArchiveBomb,
    #[serde(rename = "type_mismatch")]
    TypeMismatch,
    #[serde(rename = "encrypted_document")]
    EncryptedDocument,
}

/// `unavailable`: extraction failed or the type is unsupported. `skipped`: by policy or risk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AttachmentTextStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "unavailable")]
    Unavailable,
    #[serde(rename = "skipped")]
    Skipped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AttachmentDisposition {
    #[serde(rename = "attachment")]
    Attachment,
    #[serde(rename = "inline")]
    Inline,
}

fn schema_attachment_disposition_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["attachment", "inline", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Attachment {
    pub id: AttachmentId,
    /// Sanitised (no path, at most 255 bytes). Untrusted content.
    #[schema(required = true)]
    pub filename: Option<String>,
    pub content_type: String,
    /// Bytes.
    pub size: i64,
    #[schema(required = true, schema_with = schema_attachment_disposition_property)]
    pub disposition: Option<AttachmentDisposition>,
    pub text_status: AttachmentTextStatus,
    /// Pages of extracted text, when known.
    #[schema(required = true)]
    pub pages: Option<i64>,
    #[schema(required = true)]
    pub risk: Option<AttachmentRisk>,
    /// The upload the attachment was copied from; `null` for inline, inbound and forwarded attachments. It stays after the upload is gone.
    #[schema(required = true)]
    pub upload_id: Option<UploadId>,
}

/// `unverified`: the sender's DMARC policy is `quarantine` or `reject`, no aligned DKIM signature passed, and SPF alignment could not be read because no trusted `Authentication-Results` header was available (`PM_TRUSTED_AUTHSERV_ID` not yet set). It quarantines with `auth_unverified`. `arc_rescued`: DMARC failed, and the newest ARC seal is from a sealer in `inbound.trusted_arc_sealers` that signed `From`, recorded `dmarc=pass` for the `From` domain, sealed within 72 hours and saw the message addressed to this identity. It is not quarantined for authentication and never releases a verification code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AuthVerdict {
    #[serde(rename = "pass")]
    Pass,
    #[serde(rename = "arc_rescued")]
    ArcRescued,
    #[serde(rename = "fail")]
    Fail,
    #[serde(rename = "softfail")]
    Softfail,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "unaligned")]
    Unaligned,
    #[serde(rename = "unverified")]
    Unverified,
}

/// An RFC 8601 authentication result for one method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum AuthResult {
    #[serde(rename = "pass")]
    Pass,
    #[serde(rename = "fail")]
    Fail,
    #[serde(rename = "softfail")]
    Softfail,
    #[serde(rename = "neutral")]
    Neutral,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "temperror")]
    Temperror,
    #[serde(rename = "permerror")]
    Permerror,
    #[serde(rename = "policy")]
    Policy,
}

/// `thread_join_unverified`: the message joined a thread without a valid thread token, either by a token that failed verification or by `In-Reply-To`/`References` from a sender who is not a participant of the thread ([C9]); a `wait` with `kind=reply` does not match it. `shared_domain_sender`: the sender is an address on the shared platform domain that belongs to another workspace ([D12]). `steering_suspected`: screening judged that the message tries to steer an agent. `attachments_unscreened`: screening could not read the text of at least one attachment (an image while `inbound.extract_image_text` is off, a file whose conversion failed, text still pending at the verdict); treat those attachments' bytes as unscreened content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TrustFlag {
    #[serde(rename = "hidden_text")]
    HiddenText,
    #[serde(rename = "display_name_spoof")]
    DisplayNameSpoof,
    #[serde(rename = "lookalike_domain")]
    LookalikeDomain,
    #[serde(rename = "reply_to_mismatch")]
    ReplyToMismatch,
    #[serde(rename = "thread_join_unverified")]
    ThreadJoinUnverified,
    #[serde(rename = "shared_domain_sender")]
    SharedDomainSender,
    #[serde(rename = "steering_suspected")]
    SteeringSuspected,
    #[serde(rename = "attachments_unscreened")]
    AttachmentsUnscreened,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TrustDmarcPolicy {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "quarantine")]
    Quarantine,
    #[serde(rename = "reject")]
    Reject,
}

fn schema_trust_dmarc_policy_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["none", "quarantine", "reject", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TrustDmarcAlignedBy {
    #[serde(rename = "dkim")]
    Dkim,
    #[serde(rename = "spf")]
    Spf,
}

fn schema_trust_dmarc_aligned_by_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["dkim", "spf", null], "type": ["string", "null"]}"###)
        .expect("the contract's schema")
}

/// Authentication and trust metadata (FR-IN-4), with the domain each method checked and whether it aligned (FR-IN-14).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Trust {
    pub verdict: AuthVerdict,
    pub spf: AuthResult,
    pub dkim: AuthResult,
    pub dmarc: AuthResult,
    pub arc: AuthResult,
    /// The domain SPF checked (`smtp.mailfrom` of the trusted header, or SES's `mail.source`); `null` when SPF was not checked.
    #[schema(required = true)]
    pub spf_domain: Option<String>,
    pub spf_aligned: bool,
    /// One entry per DKIM signature evaluated, in header order.
    pub dkim_domains: Vec<DkimDomainResult>,
    /// The `From` domain DMARC was evaluated for.
    #[schema(required = true)]
    pub dmarc_domain: Option<String>,
    #[schema(required = true, schema_with = schema_trust_dmarc_policy_property)]
    pub dmarc_policy: Option<TrustDmarcPolicy>,
    #[schema(required = true, schema_with = schema_trust_dmarc_aligned_by_property)]
    pub dmarc_aligned_by: Option<TrustDmarcAlignedBy>,
    /// The `d=` of the newest `ARC-Seal` when the ARC chain validated, trusted or not.
    #[schema(required = true)]
    pub arc_sealer: Option<String>,
    /// The sender is a known contact of this identity.
    pub known_sender: bool,
    pub quarantined: bool,
    #[schema(required = true)]
    pub spam_score: Option<f64>,
    /// Auto-reply, mailing list, bounce or read receipt (FR-IN-6). Never auto-reply to it.
    pub automated: bool,
    pub flags: Vec<TrustFlag>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DkimDomainResult {
    /// `d=`, an A-label in lower case.
    pub domain: String,
    pub selector: String,
    pub result: AuthResult,
    pub aligned: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TriageStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "skipped")]
    Skipped,
    #[serde(rename = "failed")]
    Failed,
}

/// The built-in triage categories, used when the tenant has no custom list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum BuiltinTriageCategory {
    #[serde(rename = "customer_request")]
    CustomerRequest,
    #[serde(rename = "vendor")]
    Vendor,
    #[serde(rename = "billing")]
    Billing,
    #[serde(rename = "legal_compliance")]
    LegalCompliance,
    #[serde(rename = "verification")]
    Verification,
    #[serde(rename = "notification")]
    Notification,
    #[serde(rename = "newsletter")]
    Newsletter,
    #[serde(rename = "marketing")]
    Marketing,
    #[serde(rename = "auto_reply")]
    AutoReply,
    #[serde(rename = "personal")]
    Personal,
    #[serde(rename = "spam")]
    Spam,
    #[serde(rename = "other")]
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum RiskFlag {
    #[serde(rename = "payment_change_request")]
    PaymentChangeRequest,
    #[serde(rename = "credential_request")]
    CredentialRequest,
    #[serde(rename = "prompt_injection_suspected")]
    PromptInjectionSuspected,
    #[serde(rename = "phishing_suspected")]
    PhishingSuspected,
    #[serde(rename = "impersonation_suspected")]
    ImpersonationSuspected,
    #[serde(rename = "urgent_pressure")]
    UrgentPressure,
    #[serde(rename = "unknown_sender")]
    UnknownSender,
    #[serde(rename = "auth_failed")]
    AuthFailed,
    #[serde(rename = "attachment_risky")]
    AttachmentRisky,
    #[serde(rename = "hidden_text")]
    HiddenText,
}

fn schema_triage_category_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"anyOf": [{"$ref": "#/components/schemas/BuiltinTriageCategory"}, {"type": "string"}, {"type": "null"}], "description": "A built-in category, or one of the tenant's custom categories."}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TriageReason {
    #[serde(rename = "allowance")]
    Allowance,
    #[serde(rename = "policy_disabled")]
    PolicyDisabled,
    #[serde(rename = "not_eligible")]
    NotEligible,
    #[serde(rename = "ai_unavailable")]
    AiUnavailable,
    #[serde(rename = "invalid_output")]
    InvalidOutput,
    #[serde(rename = "model_unavailable")]
    ModelUnavailable,
    #[serde(rename = "input_unavailable")]
    InputUnavailable,
}

fn schema_triage_reason_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Present only when `status` is `skipped` (`allowance`: the workspace's triage allowance is spent, edge case W7; `policy_disabled`; `not_eligible`: hidden, throttled, DSN or MDN, or a message of the system identity; `ai_unavailable`: the tenant's share of Workers AI stayed busy for about two hours, [F16]) or `failed` (`invalid_output`, `model_unavailable`, `input_unavailable`).", "enum": ["allowance", "policy_disabled", "not_eligible", "ai_unavailable", "invalid_output", "model_unavailable", "input_unavailable"], "type": "string"}"###).expect("the contract's schema")
}

/// Advisory triage (FR-TRI-1). It never sends, deletes or releases anything.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Triage {
    pub status: TriageStatus,
    /// A built-in category, or one of the tenant's custom categories.
    #[schema(schema_with = schema_triage_category_property, required = true)]
    pub category: Option<String>,
    #[schema(required = true)]
    pub needs_reply: Option<f64>,
    #[schema(required = true)]
    pub urgency: Option<i64>,
    #[schema(required = true)]
    pub summary: Option<String>,
    /// BCP 47 language tag.
    #[schema(required = true)]
    pub language: Option<String>,
    pub risk_flags: Vec<RiskFlag>,
    /// The model that produced the triage, `"rules"` when rules alone decided (`skip_model`), or `null` while it is pending or when it was skipped or failed.
    #[schema(required = true)]
    pub model: Option<String>,
    /// The triage logic version (`TRIAGE_VERSION`) that produced the record: the prompt, schema, built-in rules and merge logic of the release that ran it. A re-run on the same release keeps it; a re-run after an upgrade records the current one.
    pub version: i64,
    /// How many times triage has completed for this message: `0` for a record written at ingest (`skipped`), `1` after the first run, one more after each re-run, release or re-parse.
    pub run: i64,
    /// Present only when `status` is `skipped` (`allowance`: the workspace's triage allowance is spent, edge case W7; `policy_disabled`; `not_eligible`: hidden, throttled, DSN or MDN, or a message of the system identity; `ai_unavailable`: the tenant's share of Workers AI stayed busy for about two hours, [F16]) or `failed` (`invalid_output`, `model_unavailable`, `input_unavailable`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_triage_reason_property)]
    pub reason: Option<TriageReason>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DeliveryStatus {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "suppressed")]
    Suppressed,
    #[serde(rename = "submitted")]
    Submitted,
    #[serde(rename = "delivered")]
    Delivered,
    #[serde(rename = "deferred")]
    Deferred,
    #[serde(rename = "bounced")]
    Bounced,
    #[serde(rename = "complained")]
    Complained,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "uncertain")]
    Uncertain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DeliveryField {
    #[serde(rename = "to")]
    To,
    #[serde(rename = "cc")]
    Cc,
    #[serde(rename = "bcc")]
    Bcc,
}

fn schema_delivery_field_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["to", "cc", "bcc"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DeliveryBounceType {
    #[serde(rename = "hard")]
    Hard,
    #[serde(rename = "soft")]
    Soft,
}

fn schema_delivery_bounce_type_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["hard", "soft", null], "type": ["string", "null"]}"###)
        .expect("the contract's schema")
}

/// The delivery status of one recipient of an outbound message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Delivery {
    pub address: EmailAddress,
    #[schema(schema_with = schema_delivery_field_property)]
    pub field: DeliveryField,
    pub status: DeliveryStatus,
    #[schema(required = true)]
    pub smtp_code: Option<String>,
    /// The RFC 3463 enhanced status code from the provider or relay, when it gave one.
    #[schema(required = true)]
    pub enhanced_code: Option<String>,
    #[schema(required = true, schema_with = schema_delivery_bounce_type_property)]
    pub bounce_type: Option<DeliveryBounceType>,
    pub updated_at: String,
}

fn schema_ref_kind_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"anyOf": [{"enum": ["uk_plate", "pcn", "invoice", "order", "amount", "phone", "email", "domain", "date"], "type": "string"}, {"pattern": "^custom:.+$", "type": "string"}], "description": "A built-in kind, or `custom:<name>` for a tenant pattern (a booking reference, for example, is `custom:booking`)."}"###).expect("the contract's schema")
}

/// An exact reference extracted from the subject, body or attachment text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Ref {
    /// A built-in kind, or `custom:<name>` for a tenant pattern (a booking reference, for example, is `custom:booking`).
    #[schema(schema_with = schema_ref_kind_property)]
    pub kind: String,
    /// The normalised value, for example `AB12CDE`, `+447700900123` or `GBP:412.80`.
    pub value: String,
}

/// `pending` (no verdict yet), `clear`, `suspect` (the message tries to steer an agent) or `unavailable` (no verdict by the deadline).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ScreeningStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "clear")]
    Clear,
    #[serde(rename = "suspect")]
    Suspect,
    #[serde(rename = "unavailable")]
    Unavailable,
}

/// A service code, never model text. With status `unavailable` in hold mode, `model_cap` and `invalid_output` always hold the message as `screening_unavailable`; `deadline_exceeded`, `model_unavailable` and `ai_rate_limited` follow the tenant's `screening.on_unavailable`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ScreeningReason {
    #[serde(rename = "rules")]
    Rules,
    #[serde(rename = "model")]
    Model,
    #[serde(rename = "late_signal")]
    LateSignal,
    #[serde(rename = "model_clear")]
    ModelClear,
    #[serde(rename = "nothing_to_screen")]
    NothingToScreen,
    #[serde(rename = "deadline_exceeded")]
    DeadlineExceeded,
    #[serde(rename = "model_unavailable")]
    ModelUnavailable,
    #[serde(rename = "invalid_output")]
    InvalidOutput,
    #[serde(rename = "ai_rate_limited")]
    AiRateLimited,
    #[serde(rename = "model_cap")]
    ModelCap,
    #[serde(rename = "test_forced")]
    TestForced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ScreeningSignalSource {
    #[serde(rename = "rules")]
    Rules,
    #[serde(rename = "model")]
    Model,
}

fn schema_screening_signal_source_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["rules", "model"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ScreeningSignalKind {
    #[serde(rename = "instruction_override")]
    InstructionOverride,
    #[serde(rename = "role_marker")]
    RoleMarker,
    #[serde(rename = "role_claim")]
    RoleClaim,
    #[serde(rename = "tool_mention")]
    ToolMention,
    #[serde(rename = "action_request")]
    ActionRequest,
    #[serde(rename = "exfiltration")]
    Exfiltration,
    #[serde(rename = "fence_spoof")]
    FenceSpoof,
    #[serde(rename = "encoded_payload")]
    EncodedPayload,
    #[serde(rename = "concealment")]
    Concealment,
}

fn schema_screening_signal_kind_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"enum": ["instruction_override", "role_marker", "role_claim", "tool_mention", "action_request", "exfiltration", "fence_spoof", "encoded_payload", "concealment"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ScreeningSignalField {
    #[serde(rename = "subject")]
    Subject,
    #[serde(rename = "display_name")]
    DisplayName,
    #[serde(rename = "body")]
    Body,
    #[serde(rename = "html")]
    Html,
    #[serde(rename = "hidden")]
    Hidden,
    #[serde(rename = "filename")]
    Filename,
    #[serde(rename = "attachment")]
    Attachment,
}

fn schema_screening_signal_field_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["subject", "display_name", "body", "html", "hidden", "filename", "attachment", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ScreeningSignalSeverity {
    #[serde(rename = "high")]
    High,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "low")]
    Low,
}

fn schema_screening_signal_severity_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["high", "medium", "low", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScreeningSignal {
    #[schema(schema_with = schema_screening_signal_source_property)]
    pub source: ScreeningSignalSource,
    #[schema(schema_with = schema_screening_signal_kind_property)]
    pub kind: ScreeningSignalKind,
    #[schema(required = true, schema_with = schema_screening_signal_field_property)]
    pub field: Option<ScreeningSignalField>,
    #[schema(required = true, schema_with = schema_screening_signal_severity_property)]
    pub severity: Option<ScreeningSignalSeverity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum ScreeningMode {
    #[serde(rename = "monitor")]
    Monitor,
    #[serde(rename = "hold")]
    Hold,
}

fn schema_screening_mode_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["monitor", "hold"], "type": "string"}"###)
        .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScreeningReleasedBy {
    #[schema(required = true)]
    pub key_id: Option<String>,
    #[schema(required = true)]
    pub user_id: Option<String>,
    pub at: String,
}

fn schema_screening_released_by_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"oneOf": [{"properties": {"at": {"format": "date-time", "type": "string"}, "key_id": {"type": ["string", "null"]}, "user_id": {"type": ["string", "null"]}}, "required": ["key_id", "user_id", "at"], "type": "object"}, {"type": "null"}]}"###).expect("the contract's schema")
}

/// A message's screening record (docs › Screening hold § 3). Holds no text from the mail or the classifier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Screening {
    pub status: ScreeningStatus,
    #[schema(schema_with = schema_screening_mode_property)]
    pub mode: ScreeningMode,
    #[schema(required = true)]
    pub reason: Option<ScreeningReason>,
    pub signals: Vec<ScreeningSignal>,
    /// The model ID, `rules`, or `null`.
    #[schema(required = true)]
    pub model: Option<String>,
    pub started_at: String,
    pub deadline_at: String,
    #[schema(required = true)]
    pub completed_at: Option<String>,
    pub late: bool,
    #[schema(required = true, schema_with = schema_screening_released_by_property)]
    pub released_by: Option<ScreeningReleasedBy>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum CalendarTimeTzResolution {
    #[serde(rename = "utc")]
    Utc,
    #[serde(rename = "iana")]
    Iana,
    #[serde(rename = "windows")]
    Windows,
    #[serde(rename = "prefixed")]
    Prefixed,
    #[serde(rename = "vtimezone")]
    Vtimezone,
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "floating")]
    Floating,
    #[serde(rename = "date")]
    Date,
}

fn schema_calendar_time_tz_resolution_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["utc", "iana", "windows", "prefixed", "vtimezone", "unknown", "floating", "date"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum CalendarTimeSource {
    #[serde(rename = "dtend")]
    Dtend,
    #[serde(rename = "duration")]
    Duration,
    #[serde(rename = "default")]
    Default,
}

fn schema_calendar_time_source_property() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>
{
    serde_json::from_str(r###"{"description": "Ends only.", "enum": ["dtend", "duration", "default"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CalendarTime {
    #[schema(required = true)]
    pub utc: Option<String>,
    /// Local wall time, `YYYY-MM-DDTHH:MM:SS`.
    #[schema(required = true)]
    pub local: Option<String>,
    #[schema(required = true)]
    pub date: Option<String>,
    #[schema(required = true)]
    pub tzid: Option<String>,
    /// IANA zone name.
    #[schema(required = true)]
    pub tz: Option<String>,
    #[schema(schema_with = schema_calendar_time_tz_resolution_property)]
    pub tz_resolution: CalendarTimeTzResolution,
    /// Ends only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_calendar_time_source_property)]
    pub source: Option<CalendarTimeSource>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CalendarParty {
    #[schema(required = true)]
    pub address: Option<String>,
    #[schema(required = true)]
    pub name: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub sent_by: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub uri_scheme: Option<Option<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum CalendarAttendeeRole {
    #[serde(rename = "chair")]
    Chair,
    #[serde(rename = "req_participant")]
    ReqParticipant,
    #[serde(rename = "opt_participant")]
    OptParticipant,
    #[serde(rename = "non_participant")]
    NonParticipant,
    #[serde(rename = "other")]
    Other,
}

fn schema_calendar_attendee_role_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["chair", "req_participant", "opt_participant", "non_participant", "other"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum CalendarAttendeePartstat {
    #[serde(rename = "needs_action")]
    NeedsAction,
    #[serde(rename = "accepted")]
    Accepted,
    #[serde(rename = "declined")]
    Declined,
    #[serde(rename = "tentative")]
    Tentative,
    #[serde(rename = "delegated")]
    Delegated,
    #[serde(rename = "other")]
    Other,
}

fn schema_calendar_attendee_partstat_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["needs_action", "accepted", "declined", "tentative", "delegated", "other"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum CalendarAttendeeCutype {
    #[serde(rename = "individual")]
    Individual,
    #[serde(rename = "group")]
    Group,
    #[serde(rename = "resource")]
    Resource,
    #[serde(rename = "room")]
    Room,
    #[serde(rename = "unknown")]
    Unknown,
}

fn schema_calendar_attendee_cutype_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["individual", "group", "resource", "room", "unknown"], "type": "string"}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CalendarAttendee {
    #[serde(flatten)]
    pub base: CalendarParty,
    #[schema(schema_with = schema_calendar_attendee_role_property)]
    pub role: CalendarAttendeeRole,
    #[schema(schema_with = schema_calendar_attendee_partstat_property)]
    pub partstat: CalendarAttendeePartstat,
    pub rsvp: bool,
    #[schema(schema_with = schema_calendar_attendee_cutype_property)]
    pub cutype: CalendarAttendeeCutype,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub delegated_from: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub delegated_to: Option<Vec<String>>,
    pub is_self: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageCalendarObjectStatus {
    #[serde(rename = "parsed")]
    Parsed,
    #[serde(rename = "invalid")]
    Invalid,
    #[serde(rename = "too_large")]
    TooLarge,
    #[serde(rename = "too_complex")]
    TooComplex,
    #[serde(rename = "unsupported")]
    Unsupported,
}

fn schema_message_calendar_object_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["parsed", "invalid", "too_large", "too_complex", "unsupported"], "type": "string"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageCalendarMethod {
    #[serde(rename = "publish")]
    Publish,
    #[serde(rename = "request")]
    Request,
    #[serde(rename = "reply")]
    Reply,
    #[serde(rename = "add")]
    Add,
    #[serde(rename = "cancel")]
    Cancel,
    #[serde(rename = "refresh")]
    Refresh,
    #[serde(rename = "counter")]
    Counter,
    #[serde(rename = "declinecounter")]
    Declinecounter,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "unknown")]
    Unknown,
}

fn schema_message_calendar_method_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["publish", "request", "reply", "add", "cancel", "refresh", "counter", "declinecounter", "none", "unknown", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageCalendarVerifiedBy {
    #[serde(rename = "organizer")]
    Organizer,
    #[serde(rename = "attendee")]
    Attendee,
}

fn schema_message_calendar_verified_by_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(
        r###"{"enum": ["organizer", "attendee", null], "type": ["string", "null"]}"###,
    )
    .expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageCalendarReason {
    #[serde(rename = "object_invalid")]
    ObjectInvalid,
    #[serde(rename = "object_too_large")]
    ObjectTooLarge,
    #[serde(rename = "object_too_complex")]
    ObjectTooComplex,
    #[serde(rename = "no_vevent")]
    NoVevent,
    #[serde(rename = "method_unknown")]
    MethodUnknown,
    #[serde(rename = "method_mismatch")]
    MethodMismatch,
    #[serde(rename = "organizer_missing")]
    OrganizerMissing,
    #[serde(rename = "organizer_not_mailto")]
    OrganizerNotMailto,
    #[serde(rename = "organizer_inconsistent")]
    OrganizerInconsistent,
    #[serde(rename = "uid_mismatch")]
    UidMismatch,
    #[serde(rename = "attendee_count")]
    AttendeeCount,
    #[serde(rename = "sender_not_authenticated")]
    SenderNotAuthenticated,
    #[serde(rename = "sender_is_delegator")]
    SenderIsDelegator,
    #[serde(rename = "sent_by_unverifiable")]
    SentByUnverifiable,
    #[serde(rename = "sender_not_organizer")]
    SenderNotOrganizer,
    #[serde(rename = "sender_not_attendee")]
    SenderNotAttendee,
}

fn schema_message_calendar_reason_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "`sent_by_unverifiable`: the sender is only a `SENT-BY` of the organizer or attendee, which never verifies.", "enum": ["object_invalid", "object_too_large", "object_too_complex", "no_vevent", "method_unknown", "method_mismatch", "organizer_missing", "organizer_not_mailto", "organizer_inconsistent", "uid_mismatch", "attendee_count", "sender_not_authenticated", "sender_is_delegator", "sent_by_unverifiable", "sender_not_organizer", "sender_not_attendee", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageCalendarStatus {
    #[serde(rename = "tentative")]
    Tentative,
    #[serde(rename = "confirmed")]
    Confirmed,
    #[serde(rename = "cancelled")]
    Cancelled,
}

fn schema_message_calendar_status_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["tentative", "confirmed", "cancelled", null], "type": ["string", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageCalendarConferenceItem {
    pub uri: String,
    #[schema(required = true)]
    pub label: Option<String>,
}

fn schema_message_calendar_conference_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "The primary's `CONFERENCE` properties (RFC 7986). Untrusted; never fetched.", "items": {"properties": {"label": {"maxLength": 256, "type": ["string", "null"]}, "uri": {"maxLength": 2048, "type": "string"}}, "required": ["uri", "label"], "type": "object"}, "maxItems": 5, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum MessageCalendarEndsAtBasis {
    #[serde(rename = "utc")]
    Utc,
    #[serde(rename = "tenant_timezone")]
    TenantTimezone,
}

fn schema_message_calendar_ends_at_basis_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"enum": ["utc", "tenant_timezone"], "type": "string"}"###)
        .expect("the contract's schema")
}

/// Validated, never expanded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageCalendarRrule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = String)]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub valid: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub freq: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub interval: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub count: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub until: Option<Option<String>>,
}

fn schema_message_calendar_rrule_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Validated, never expanded.", "properties": {"count": {"type": ["integer", "null"]}, "freq": {"type": ["string", "null"]}, "interval": {"type": ["integer", "null"]}, "text": {"maxLength": 512, "type": "string"}, "until": {"type": ["string", "null"]}, "valid": {"type": "boolean"}}, "type": ["object", "null"]}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageCalendarOverridesItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = CalendarTime)]
    pub recurrence_id: Option<CalendarTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub this_and_future: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub sequence: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub status: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = CalendarTime)]
    pub start: Option<CalendarTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = CalendarTime)]
    pub end: Option<CalendarTime>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub summary: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub location: Option<Option<String>>,
    #[serde(rename = "self")]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<CalendarAttendee>)]
    pub self_: Option<Option<CalendarAttendee>>,
}

fn schema_message_calendar_overrides_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"items": {"properties": {"end": {"$ref": "#/components/schemas/CalendarTime"}, "location": {"maxLength": 256, "type": ["string", "null"]}, "recurrence_id": {"$ref": "#/components/schemas/CalendarTime"}, "self": {"oneOf": [{"$ref": "#/components/schemas/CalendarAttendee"}, {"type": "null"}]}, "sequence": {"type": "integer"}, "start": {"$ref": "#/components/schemas/CalendarTime"}, "status": {"type": ["string", "null"]}, "summary": {"maxLength": 256, "type": ["string", "null"]}, "this_and_future": {"type": "boolean"}}, "type": "object"}, "maxItems": 100, "type": "array"}"###).expect("the contract's schema")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageCalendarRemoved {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub alarms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub attachments: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub images: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub x_properties: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub conference: Option<i64>,
}

fn schema_message_calendar_removed_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"properties": {"alarms": {"type": "integer"}, "attachments": {"type": "integer"}, "conference": {"type": "integer"}, "images": {"type": "integer"}, "x_properties": {"type": "integer"}}, "type": "object"}"###).expect("the contract's schema")
}

/// Outbound messages only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageCalendarOutbound {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub attach_copy: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub added_vtimezones: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub removed: Option<Vec<String>>,
}

fn schema_message_calendar_outbound_property()
-> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    serde_json::from_str(r###"{"description": "Outbound messages only.", "properties": {"added_vtimezones": {"items": {"type": "string"}, "maxItems": 10, "type": "array"}, "attach_copy": {"type": "boolean"}, "removed": {"items": {"type": "string"}, "maxItems": 20, "type": "array"}}, "type": "object"}"###).expect("the contract's schema")
}

/// The message's calendar object (docs › Calendar invitations § 2.9). Every string is untrusted content. Alarms, attachments, images and extension properties are never included. New `warnings` values and new `reason` values may be added within /v1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageCalendar {
    #[schema(schema_with = schema_message_calendar_object_status_property)]
    pub object_status: MessageCalendarObjectStatus,
    #[schema(required = true, schema_with = schema_message_calendar_method_property)]
    pub method: Option<MessageCalendarMethod>,
    /// `null` on outbound messages.
    #[schema(required = true)]
    pub verified: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_message_calendar_verified_by_property)]
    pub verified_by: Option<Option<MessageCalendarVerifiedBy>>,
    /// `sent_by_unverifiable`: the sender is only a `SENT-BY` of the organizer or attendee, which never verifies.
    #[schema(required = true, schema_with = schema_message_calendar_reason_property)]
    pub reason: Option<MessageCalendarReason>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub authenticated_sender: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub uid: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<i64>)]
    pub sequence: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub dtstamp: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<CalendarTime>)]
    pub recurrence_id: Option<Option<CalendarTime>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_message_calendar_status_property)]
    pub status: Option<Option<MessageCalendarStatus>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub summary: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub summary_truncated: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub description: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub description_truncated: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub location: Option<Option<String>>,
    /// The primary's `CONFERENCE` properties (RFC 7986). Untrusted; never fetched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_message_calendar_conference_property)]
    pub conference: Option<Vec<MessageCalendarConferenceItem>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub comment: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<CalendarTime>)]
    pub start: Option<Option<CalendarTime>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<CalendarTime>)]
    pub end: Option<Option<CalendarTime>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub all_day: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub floating: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<String>)]
    pub ends_at: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_message_calendar_ends_at_basis_property)]
    pub ends_at_basis: Option<MessageCalendarEndsAtBasis>,
    /// Validated, never expanded.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(schema_with = schema_message_calendar_rrule_property)]
    pub rrule: Option<Option<MessageCalendarRrule>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub exdates_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub rdates_count: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    #[schema(value_type = Option<CalendarParty>)]
    pub organizer: Option<Option<CalendarParty>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<CalendarAttendee>)]
    pub attendees: Option<Vec<CalendarAttendee>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub attendees_total: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub attendees_truncated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_message_calendar_overrides_property)]
    pub overrides: Option<Vec<MessageCalendarOverridesItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub overrides_total: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub overrides_truncated: Option<bool>,
    /// `true` only for a verified `reply`, `refresh` or `counter` answering an invitation this identity sent with the same `UID` and `ORGANIZER`, from an attendee that invitation names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = bool)]
    pub organized_here: Option<bool>,
    /// The attachment holding the raw `.ics`.
    #[schema(required = true)]
    pub attachment_id: Option<AttachmentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub other_objects: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub ignored_components: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_message_calendar_removed_property)]
    pub removed: Option<MessageCalendarRemoved>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Vec<String>)]
    pub warnings: Option<Vec<String>>,
    /// Outbound messages only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(schema_with = schema_message_calendar_outbound_property)]
    pub outbound: Option<MessageCalendarOutbound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = i64)]
    pub parser_version: Option<i64>,
}

/// Draft ID (`drf_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DraftId(pub String);

/// Upload ID (`upl_` + ULID).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct UploadId(pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ThreadPage {
    pub data: Vec<ThreadSummary>,
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}
