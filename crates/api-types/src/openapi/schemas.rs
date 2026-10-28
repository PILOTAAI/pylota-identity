//! The component schemas of `openapi.json`, in the order of `openapi.yaml`.

use utoipa::openapi::{RefOr, schema::Schema};

use crate::*;

/// Every component schema, by name.
pub fn components() -> Vec<(String, RefOr<Schema>)> {
    vec![
        (
            "TenantId".to_owned(),
            <TenantId as utoipa::PartialSchema>::schema(),
        ),
        (
            "PartnerId".to_owned(),
            <PartnerId as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityId".to_owned(),
            <IdentityId as utoipa::PartialSchema>::schema(),
        ),
        (
            "AddressId".to_owned(),
            <AddressId as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainId".to_owned(),
            <DomainId as utoipa::PartialSchema>::schema(),
        ),
        (
            "ThreadId".to_owned(),
            <ThreadId as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageId".to_owned(),
            <MessageId as utoipa::PartialSchema>::schema(),
        ),
        (
            "AttachmentId".to_owned(),
            <AttachmentId as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookId".to_owned(),
            <WebhookId as utoipa::PartialSchema>::schema(),
        ),
        (
            "DeliveryId".to_owned(),
            <DeliveryId as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventId".to_owned(),
            <EventId as utoipa::PartialSchema>::schema(),
        ),
        (
            "KeyId".to_owned(),
            <KeyId as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureRequestId".to_owned(),
            <ErasureRequestId as utoipa::PartialSchema>::schema(),
        ),
        (
            "ExportId".to_owned(),
            <ExportId as utoipa::PartialSchema>::schema(),
        ),
        (
            "AuditEventId".to_owned(),
            <AuditEventId as utoipa::PartialSchema>::schema(),
        ),
        (
            "UserId".to_owned(),
            <UserId as utoipa::PartialSchema>::schema(),
        ),
        (
            "InvitationId".to_owned(),
            <InvitationId as utoipa::PartialSchema>::schema(),
        ),
        (
            "DlqItemId".to_owned(),
            <DlqItemId as utoipa::PartialSchema>::schema(),
        ),
        (
            "JobId".to_owned(),
            <JobId as utoipa::PartialSchema>::schema(),
        ),
        (
            "RequestId".to_owned(),
            <RequestId as utoipa::PartialSchema>::schema(),
        ),
        (
            "EmailAddress".to_owned(),
            <EmailAddress as utoipa::PartialSchema>::schema(),
        ),
        (
            "MailboxAddress".to_owned(),
            <MailboxAddress as utoipa::PartialSchema>::schema(),
        ),
        (
            "Label".to_owned(),
            <Label as utoipa::PartialSchema>::schema(),
        ),
        (
            "Metadata".to_owned(),
            <Metadata as utoipa::PartialSchema>::schema(),
        ),
        (
            "Direction".to_owned(),
            <Direction as utoipa::PartialSchema>::schema(),
        ),
        (
            "Error".to_owned(),
            <Error as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErrorDetails".to_owned(),
            <ErrorDetails as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErrorCode".to_owned(),
            <ErrorCode as utoipa::PartialSchema>::schema(),
        ),
        (
            "TransportUnavailableReason".to_owned(),
            <TransportUnavailableReason as utoipa::PartialSchema>::schema(),
        ),
        (
            "Permission".to_owned(),
            <Permission as utoipa::PartialSchema>::schema(),
        ),
        (
            "KeyLevel".to_owned(),
            <KeyLevel as utoipa::PartialSchema>::schema(),
        ),
        ("Mode".to_owned(), <Mode as utoipa::PartialSchema>::schema()),
        ("Me".to_owned(), <Me as utoipa::PartialSchema>::schema()),
        (
            "Health".to_owned(),
            <Health as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantStatus".to_owned(),
            <TenantStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantSlug".to_owned(),
            <TenantSlug as utoipa::PartialSchema>::schema(),
        ),
        (
            "AddressSuffix".to_owned(),
            <AddressSuffix as utoipa::PartialSchema>::schema(),
        ),
        (
            "Tenant".to_owned(),
            <Tenant as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantCreateRequest".to_owned(),
            <TenantCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantUpdateRequest".to_owned(),
            <TenantUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantPolicy".to_owned(),
            <TenantPolicy as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantPolicyPatch".to_owned(),
            <TenantPolicyPatch as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantPolicyView".to_owned(),
            <TenantPolicyView as utoipa::PartialSchema>::schema(),
        ),
        (
            "PolicyFieldAccess".to_owned(),
            <PolicyFieldAccess as utoipa::PartialSchema>::schema(),
        ),
        (
            "TriageCategoryDefinition".to_owned(),
            <TriageCategoryDefinition as utoipa::PartialSchema>::schema(),
        ),
        (
            "TriageRule".to_owned(),
            <TriageRule as utoipa::PartialSchema>::schema(),
        ),
        (
            "TriageRuleMatch".to_owned(),
            <TriageRuleMatch as utoipa::PartialSchema>::schema(),
        ),
        (
            "TriageRuleSet".to_owned(),
            <TriageRuleSet as utoipa::PartialSchema>::schema(),
        ),
        (
            "CustomRefPattern".to_owned(),
            <CustomRefPattern as utoipa::PartialSchema>::schema(),
        ),
        (
            "ServiceAccountId".to_owned(),
            <ServiceAccountId as utoipa::PartialSchema>::schema(),
        ),
        (
            "ServiceAccountStatus".to_owned(),
            <ServiceAccountStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "ServiceAccount".to_owned(),
            <ServiceAccount as utoipa::PartialSchema>::schema(),
        ),
        (
            "ServiceAccountCreateRequest".to_owned(),
            <ServiceAccountCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "ServiceAccountNoteRequest".to_owned(),
            <ServiceAccountNoteRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "ServiceAccountPage".to_owned(),
            <ServiceAccountPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgentSignupId".to_owned(),
            <AgentSignupId as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgentSignupState".to_owned(),
            <AgentSignupState as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgentSignupCreateRequest".to_owned(),
            <AgentSignupCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgentSignupCreated".to_owned(),
            <AgentSignupCreated as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgentSignup".to_owned(),
            <AgentSignup as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgentSignupVerifyRequest".to_owned(),
            <AgentSignupVerifyRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgentSignupHumanRequest".to_owned(),
            <AgentSignupHumanRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgentSignupEnded".to_owned(),
            <AgentSignupEnded as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgentSignupListItem".to_owned(),
            <AgentSignupListItem as utoipa::PartialSchema>::schema(),
        ),
        (
            "PartnerStatus".to_owned(),
            <PartnerStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "PartnerBillingMode".to_owned(),
            <PartnerBillingMode as utoipa::PartialSchema>::schema(),
        ),
        (
            "Partner".to_owned(),
            <Partner as utoipa::PartialSchema>::schema(),
        ),
        (
            "PartnerCreateRequest".to_owned(),
            <PartnerCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PartnerUpdateRequest".to_owned(),
            <PartnerUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PartnerMaxTenants".to_owned(),
            <PartnerMaxTenants as utoipa::PartialSchema>::schema(),
        ),
        (
            "PartnerRampExempt".to_owned(),
            <PartnerRampExempt as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityStatus".to_owned(),
            <IdentityStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "PauseReason".to_owned(),
            <PauseReason as utoipa::PartialSchema>::schema(),
        ),
        (
            "Username".to_owned(),
            <Username as utoipa::PartialSchema>::schema(),
        ),
        (
            "UsernameInput".to_owned(),
            <UsernameInput as utoipa::PartialSchema>::schema(),
        ),
        (
            "DisplayName".to_owned(),
            <DisplayName as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityOwner".to_owned(),
            <IdentityOwner as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentitySignature".to_owned(),
            <IdentitySignature as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentitySendPolicy".to_owned(),
            <IdentitySendPolicy as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityRetention".to_owned(),
            <IdentityRetention as utoipa::PartialSchema>::schema(),
        ),
        (
            "Identity".to_owned(),
            <Identity as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityCreateRequest".to_owned(),
            <IdentityCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityUpdateRequest".to_owned(),
            <IdentityUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "AddressRole".to_owned(),
            <AddressRole as utoipa::PartialSchema>::schema(),
        ),
        (
            "AddressStatus".to_owned(),
            <AddressStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "AddressForwarding".to_owned(),
            <AddressForwarding as utoipa::PartialSchema>::schema(),
        ),
        (
            "Address".to_owned(),
            <Address as utoipa::PartialSchema>::schema(),
        ),
        (
            "AddressCreateRequest".to_owned(),
            <AddressCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PromoteAddressRequest".to_owned(),
            <PromoteAddressRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "RetireAddressRequest".to_owned(),
            <RetireAddressRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainMethod".to_owned(),
            <DomainMethod as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainKind".to_owned(),
            <DomainKind as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainInbound".to_owned(),
            <DomainInbound as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainState".to_owned(),
            <DomainState as utoipa::PartialSchema>::schema(),
        ),
        (
            "RoutingMode".to_owned(),
            <RoutingMode as utoipa::PartialSchema>::schema(),
        ),
        (
            "Transport".to_owned(),
            <Transport as utoipa::PartialSchema>::schema(),
        ),
        (
            "ReplyTokenMode".to_owned(),
            <ReplyTokenMode as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmtpSettings".to_owned(),
            <SmtpSettings as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainSmtp".to_owned(),
            <DomainSmtp as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainProbe".to_owned(),
            <DomainProbe as utoipa::PartialSchema>::schema(),
        ),
        (
            "Domain".to_owned(),
            <Domain as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainCreateRequest".to_owned(),
            <DomainCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainUpdateRequest".to_owned(),
            <DomainUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "ProbeAccepted".to_owned(),
            <ProbeAccepted as utoipa::PartialSchema>::schema(),
        ),
        (
            "DnsRecordStatus".to_owned(),
            <DnsRecordStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "DnsRecord".to_owned(),
            <DnsRecord as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainRecords".to_owned(),
            <DomainRecords as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainIssue".to_owned(),
            <DomainIssue as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainCheck".to_owned(),
            <DomainCheck as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainHealth".to_owned(),
            <DomainHealth as utoipa::PartialSchema>::schema(),
        ),
        (
            "ThreadHold".to_owned(),
            <ThreadHold as utoipa::PartialSchema>::schema(),
        ),
        (
            "ThreadHoldRequest".to_owned(),
            <ThreadHoldRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "ThreadSummary".to_owned(),
            <ThreadSummary as utoipa::PartialSchema>::schema(),
        ),
        (
            "Thread".to_owned(),
            <Thread as utoipa::PartialSchema>::schema(),
        ),
        (
            "ThreadUpdateRequest".to_owned(),
            <ThreadUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageStatus".to_owned(),
            <MessageStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "OutboundStatus".to_owned(),
            <OutboundStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageKind".to_owned(),
            <MessageKind as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageFlag".to_owned(),
            <MessageFlag as utoipa::PartialSchema>::schema(),
        ),
        (
            "QuarantineReason".to_owned(),
            <QuarantineReason as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageHeader".to_owned(),
            <MessageHeader as utoipa::PartialSchema>::schema(),
        ),
        (
            "Message".to_owned(),
            <Message as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageSummary".to_owned(),
            <MessageSummary as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageUpdateRequest".to_owned(),
            <MessageUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "BatchMessageIds".to_owned(),
            <BatchMessageIds as utoipa::PartialSchema>::schema(),
        ),
        (
            "BatchGetRequest".to_owned(),
            <BatchGetRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "BatchMessage".to_owned(),
            <BatchMessage as utoipa::PartialSchema>::schema(),
        ),
        (
            "BatchGetResponse".to_owned(),
            <BatchGetResponse as utoipa::PartialSchema>::schema(),
        ),
        (
            "BatchUpdateRequest".to_owned(),
            <BatchUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "BatchUpdateResponse".to_owned(),
            <BatchUpdateResponse as utoipa::PartialSchema>::schema(),
        ),
        (
            "AttachmentRisk".to_owned(),
            <AttachmentRisk as utoipa::PartialSchema>::schema(),
        ),
        (
            "AttachmentTextStatus".to_owned(),
            <AttachmentTextStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "Attachment".to_owned(),
            <Attachment as utoipa::PartialSchema>::schema(),
        ),
        (
            "AttachmentText".to_owned(),
            <AttachmentText as utoipa::PartialSchema>::schema(),
        ),
        (
            "AuthVerdict".to_owned(),
            <AuthVerdict as utoipa::PartialSchema>::schema(),
        ),
        (
            "AuthResult".to_owned(),
            <AuthResult as utoipa::PartialSchema>::schema(),
        ),
        (
            "TrustFlag".to_owned(),
            <TrustFlag as utoipa::PartialSchema>::schema(),
        ),
        (
            "Trust".to_owned(),
            <Trust as utoipa::PartialSchema>::schema(),
        ),
        (
            "DkimDomainResult".to_owned(),
            <DkimDomainResult as utoipa::PartialSchema>::schema(),
        ),
        (
            "TrustSummary".to_owned(),
            <TrustSummary as utoipa::PartialSchema>::schema(),
        ),
        (
            "TriageStatus".to_owned(),
            <TriageStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "BuiltinTriageCategory".to_owned(),
            <BuiltinTriageCategory as utoipa::PartialSchema>::schema(),
        ),
        (
            "RiskFlag".to_owned(),
            <RiskFlag as utoipa::PartialSchema>::schema(),
        ),
        (
            "Triage".to_owned(),
            <Triage as utoipa::PartialSchema>::schema(),
        ),
        (
            "DeliveryStatus".to_owned(),
            <DeliveryStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "Delivery".to_owned(),
            <Delivery as utoipa::PartialSchema>::schema(),
        ),
        ("Ref".to_owned(), <Ref as utoipa::PartialSchema>::schema()),
        (
            "ScreeningStatus".to_owned(),
            <ScreeningStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "ScreeningReason".to_owned(),
            <ScreeningReason as utoipa::PartialSchema>::schema(),
        ),
        (
            "ScreeningSignal".to_owned(),
            <ScreeningSignal as utoipa::PartialSchema>::schema(),
        ),
        (
            "Screening".to_owned(),
            <Screening as utoipa::PartialSchema>::schema(),
        ),
        (
            "ScreeningFeedbackVerdict".to_owned(),
            <ScreeningFeedbackVerdict as utoipa::PartialSchema>::schema(),
        ),
        (
            "ScreeningFeedbackRequest".to_owned(),
            <ScreeningFeedbackRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "ScreeningFeedback".to_owned(),
            <ScreeningFeedback as utoipa::PartialSchema>::schema(),
        ),
        (
            "ScreeningSummary".to_owned(),
            <ScreeningSummary as utoipa::PartialSchema>::schema(),
        ),
        (
            "Recipient".to_owned(),
            <Recipient as utoipa::PartialSchema>::schema(),
        ),
        (
            "AttachmentInput".to_owned(),
            <AttachmentInput as utoipa::PartialSchema>::schema(),
        ),
        (
            "Unsubscribe".to_owned(),
            <Unsubscribe as utoipa::PartialSchema>::schema(),
        ),
        (
            "Consent".to_owned(),
            <Consent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SendHeaders".to_owned(),
            <SendHeaders as utoipa::PartialSchema>::schema(),
        ),
        (
            "SendRequest".to_owned(),
            <SendRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "ReplyRequest".to_owned(),
            <ReplyRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "ForwardRequest".to_owned(),
            <ForwardRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "SendResponse".to_owned(),
            <SendResponse as utoipa::PartialSchema>::schema(),
        ),
        (
            "DryRunResult".to_owned(),
            <DryRunResult as utoipa::PartialSchema>::schema(),
        ),
        (
            "DryRunRecipient".to_owned(),
            <DryRunRecipient as utoipa::PartialSchema>::schema(),
        ),
        (
            "ResolveRequest".to_owned(),
            <ResolveRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "ReleaseRequest".to_owned(),
            <ReleaseRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "CalendarSendInput".to_owned(),
            <CalendarSendInput as utoipa::PartialSchema>::schema(),
        ),
        (
            "CalendarReplyRequest".to_owned(),
            <CalendarReplyRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "CalendarTime".to_owned(),
            <CalendarTime as utoipa::PartialSchema>::schema(),
        ),
        (
            "CalendarParty".to_owned(),
            <CalendarParty as utoipa::PartialSchema>::schema(),
        ),
        (
            "CalendarAttendee".to_owned(),
            <CalendarAttendee as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageCalendar".to_owned(),
            <MessageCalendar as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftId".to_owned(),
            <DraftId as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftStatus".to_owned(),
            <DraftStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftKind".to_owned(),
            <DraftKind as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftApprovalReason".to_owned(),
            <DraftApprovalReason as utoipa::PartialSchema>::schema(),
        ),
        (
            "RequireApproval".to_owned(),
            <RequireApproval as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftFailureReason".to_owned(),
            <DraftFailureReason as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftAttachmentInput".to_owned(),
            <DraftAttachmentInput as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftAttachment".to_owned(),
            <DraftAttachment as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftCreateRequest".to_owned(),
            <DraftCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftUpdateRequest".to_owned(),
            <DraftUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftSendRequest".to_owned(),
            <DraftSendRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftApproveRequest".to_owned(),
            <DraftApproveRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftRejectRequest".to_owned(),
            <DraftRejectRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "Draft".to_owned(),
            <Draft as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftDryRun".to_owned(),
            <DraftDryRun as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftIndexEntry".to_owned(),
            <DraftIndexEntry as utoipa::PartialSchema>::schema(),
        ),
        (
            "UploadId".to_owned(),
            <UploadId as utoipa::PartialSchema>::schema(),
        ),
        (
            "UploadStatus".to_owned(),
            <UploadStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "UploadCreateRequest".to_owned(),
            <UploadCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "Upload".to_owned(),
            <Upload as utoipa::PartialSchema>::schema(),
        ),
        (
            "AttachmentUploadInput".to_owned(),
            <AttachmentUploadInput as utoipa::PartialSchema>::schema(),
        ),
        (
            "SendAttachment".to_owned(),
            <SendAttachment as utoipa::PartialSchema>::schema(),
        ),
        (
            "SearchFilters".to_owned(),
            <SearchFilters as utoipa::PartialSchema>::schema(),
        ),
        (
            "SearchRequest".to_owned(),
            <SearchRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantSearchRequest".to_owned(),
            <TenantSearchRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityIdFilter".to_owned(),
            <IdentityIdFilter as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgenticBudget".to_owned(),
            <AgenticBudget as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgenticRequest".to_owned(),
            <AgenticRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantAgenticRequest".to_owned(),
            <TenantAgenticRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "SearchMode".to_owned(),
            <SearchMode as utoipa::PartialSchema>::schema(),
        ),
        (
            "SearchResponse".to_owned(),
            <SearchResponse as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantSearchResponse".to_owned(),
            <TenantSearchResponse as utoipa::PartialSchema>::schema(),
        ),
        (
            "AttachmentHit".to_owned(),
            <AttachmentHit as utoipa::PartialSchema>::schema(),
        ),
        (
            "SearchHit".to_owned(),
            <SearchHit as utoipa::PartialSchema>::schema(),
        ),
        (
            "ThreadHit".to_owned(),
            <ThreadHit as utoipa::PartialSchema>::schema(),
        ),
        (
            "FacetCounts".to_owned(),
            <FacetCounts as utoipa::PartialSchema>::schema(),
        ),
        (
            "Facets".to_owned(),
            <Facets as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgenticStatus".to_owned(),
            <AgenticStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgenticSentence".to_owned(),
            <AgenticSentence as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgenticAnswer".to_owned(),
            <AgenticAnswer as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgenticEvidence".to_owned(),
            <AgenticEvidence as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgenticTraceStep".to_owned(),
            <AgenticTraceStep as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgenticUsage".to_owned(),
            <AgenticUsage as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgenticResponse".to_owned(),
            <AgenticResponse as utoipa::PartialSchema>::schema(),
        ),
        (
            "SearchHitList".to_owned(),
            <SearchHitList as utoipa::PartialSchema>::schema(),
        ),
        (
            "Contact".to_owned(),
            <Contact as utoipa::PartialSchema>::schema(),
        ),
        (
            "Verification".to_owned(),
            <Verification as utoipa::PartialSchema>::schema(),
        ),
        (
            "WaitResponse".to_owned(),
            <WaitResponse as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookEventFilter".to_owned(),
            <WebhookEventFilter as utoipa::PartialSchema>::schema(),
        ),
        (
            "Webhook".to_owned(),
            <Webhook as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookCreated".to_owned(),
            <WebhookCreated as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookCreateRequest".to_owned(),
            <WebhookCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookUpdateRequest".to_owned(),
            <WebhookUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookHeaderMap".to_owned(),
            <WebhookHeaderMap as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookHeadersUpdateRequest".to_owned(),
            <WebhookHeadersUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "RotateRequest".to_owned(),
            <RotateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookDeliveryStatus".to_owned(),
            <WebhookDeliveryStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookDelivery".to_owned(),
            <WebhookDelivery as utoipa::PartialSchema>::schema(),
        ),
        (
            "ReplayRequest".to_owned(),
            <ReplayRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "ReplayByIds".to_owned(),
            <ReplayByIds as utoipa::PartialSchema>::schema(),
        ),
        (
            "ReplayByRange".to_owned(),
            <ReplayByRange as utoipa::PartialSchema>::schema(),
        ),
        (
            "ReplayResponse".to_owned(),
            <ReplayResponse as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventConsumerId".to_owned(),
            <EventConsumerId as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventCursor".to_owned(),
            <EventCursor as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventFeedItem".to_owned(),
            <EventFeedItem as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventFeedPage".to_owned(),
            <EventFeedPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventConsumer".to_owned(),
            <EventConsumer as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventConsumerCreateRequest".to_owned(),
            <EventConsumerCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventConsumerCommitRequest".to_owned(),
            <EventConsumerCommitRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventConsumerResetRequest".to_owned(),
            <EventConsumerResetRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "StreamToken".to_owned(),
            <StreamToken as utoipa::PartialSchema>::schema(),
        ),
        (
            "SuppressionReason".to_owned(),
            <SuppressionReason as utoipa::PartialSchema>::schema(),
        ),
        (
            "Suppression".to_owned(),
            <Suppression as utoipa::PartialSchema>::schema(),
        ),
        (
            "SuppressionCreateRequest".to_owned(),
            <SuppressionCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "SuppressionDeleteRequest".to_owned(),
            <SuppressionDeleteRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "ListDirection".to_owned(),
            <ListDirection as utoipa::PartialSchema>::schema(),
        ),
        (
            "ListKind".to_owned(),
            <ListKind as utoipa::PartialSchema>::schema(),
        ),
        (
            "ListEntryValue".to_owned(),
            <ListEntryValue as utoipa::PartialSchema>::schema(),
        ),
        (
            "ListEntry".to_owned(),
            <ListEntry as utoipa::PartialSchema>::schema(),
        ),
        (
            "ListEntryPutRequest".to_owned(),
            <ListEntryPutRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "McpToolName".to_owned(),
            <McpToolName as utoipa::PartialSchema>::schema(),
        ),
        (
            "McpToolList".to_owned(),
            <McpToolList as utoipa::PartialSchema>::schema(),
        ),
        (
            "ApiKey".to_owned(),
            <ApiKey as utoipa::PartialSchema>::schema(),
        ),
        (
            "ApiKeyCreated".to_owned(),
            <ApiKeyCreated as utoipa::PartialSchema>::schema(),
        ),
        (
            "ApiKeyCreateRequest".to_owned(),
            <ApiKeyCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureScope".to_owned(),
            <ErasureScope as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureStatus".to_owned(),
            <ErasureStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureRequestCreate".to_owned(),
            <ErasureRequestCreate as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureRequestCreateMessage".to_owned(),
            <ErasureRequestCreateMessage as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureRequestCreateThread".to_owned(),
            <ErasureRequestCreateThread as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureRequestCreateCounterparty".to_owned(),
            <ErasureRequestCreateCounterparty as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureRequestCreateIdentity".to_owned(),
            <ErasureRequestCreateIdentity as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureRequestCreateTenant".to_owned(),
            <ErasureRequestCreateTenant as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureHeldItem".to_owned(),
            <ErasureHeldItem as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureReceipt".to_owned(),
            <ErasureReceipt as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureRequest".to_owned(),
            <ErasureRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "ExportScope".to_owned(),
            <ExportScope as utoipa::PartialSchema>::schema(),
        ),
        (
            "ExportStatus".to_owned(),
            <ExportStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "ExportCreate".to_owned(),
            <ExportCreate as utoipa::PartialSchema>::schema(),
        ),
        (
            "ExportCreateCounterparty".to_owned(),
            <ExportCreateCounterparty as utoipa::PartialSchema>::schema(),
        ),
        (
            "ExportCreateIdentity".to_owned(),
            <ExportCreateIdentity as utoipa::PartialSchema>::schema(),
        ),
        (
            "Export".to_owned(),
            <Export as utoipa::PartialSchema>::schema(),
        ),
        (
            "UsageDay".to_owned(),
            <UsageDay as utoipa::PartialSchema>::schema(),
        ),
        (
            "DailyUsage".to_owned(),
            <DailyUsage as utoipa::PartialSchema>::schema(),
        ),
        (
            "BillingMode".to_owned(),
            <BillingMode as utoipa::PartialSchema>::schema(),
        ),
        (
            "UsageFeatureName".to_owned(),
            <UsageFeatureName as utoipa::PartialSchema>::schema(),
        ),
        (
            "UsageFeature".to_owned(),
            <UsageFeature as utoipa::PartialSchema>::schema(),
        ),
        (
            "PlanState".to_owned(),
            <PlanState as utoipa::PartialSchema>::schema(),
        ),
        ("Plan".to_owned(), <Plan as utoipa::PartialSchema>::schema()),
        (
            "Topups".to_owned(),
            <Topups as utoipa::PartialSchema>::schema(),
        ),
        (
            "UsageSummary".to_owned(),
            <UsageSummary as utoipa::PartialSchema>::schema(),
        ),
        (
            "PlanCatalog".to_owned(),
            <PlanCatalog as utoipa::PartialSchema>::schema(),
        ),
        (
            "BillingAccount".to_owned(),
            <BillingAccount as utoipa::PartialSchema>::schema(),
        ),
        (
            "BillingAccountUpdateRequest".to_owned(),
            <BillingAccountUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "AuditEvent".to_owned(),
            <AuditEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MetricEventType".to_owned(),
            <MetricEventType as utoipa::PartialSchema>::schema(),
        ),
        (
            "RateWindow".to_owned(),
            <RateWindow as utoipa::PartialSchema>::schema(),
        ),
        (
            "SendingStop".to_owned(),
            <SendingStop as utoipa::PartialSchema>::schema(),
        ),
        (
            "Rates".to_owned(),
            <Rates as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventCounts".to_owned(),
            <EventCounts as utoipa::PartialSchema>::schema(),
        ),
        (
            "MemberRole".to_owned(),
            <MemberRole as utoipa::PartialSchema>::schema(),
        ),
        (
            "InvitationRole".to_owned(),
            <InvitationRole as utoipa::PartialSchema>::schema(),
        ),
        (
            "Member".to_owned(),
            <Member as utoipa::PartialSchema>::schema(),
        ),
        (
            "Invitation".to_owned(),
            <Invitation as utoipa::PartialSchema>::schema(),
        ),
        (
            "InvitationCreateRequest".to_owned(),
            <InvitationCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "MemberList".to_owned(),
            <MemberList as utoipa::PartialSchema>::schema(),
        ),
        (
            "SigningKeyPurpose".to_owned(),
            <SigningKeyPurpose as utoipa::PartialSchema>::schema(),
        ),
        (
            "SigningKeyRotation".to_owned(),
            <SigningKeyRotation as utoipa::PartialSchema>::schema(),
        ),
        (
            "DlqQueue".to_owned(),
            <DlqQueue as utoipa::PartialSchema>::schema(),
        ),
        (
            "DlqItem".to_owned(),
            <DlqItem as utoipa::PartialSchema>::schema(),
        ),
        (
            "DlqItemPage".to_owned(),
            <DlqItemPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "JobKind".to_owned(),
            <JobKind as utoipa::PartialSchema>::schema(),
        ),
        (
            "JobStatus".to_owned(),
            <JobStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "JobCreateRequest".to_owned(),
            <JobCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        ("Job".to_owned(), <Job as utoipa::PartialSchema>::schema()),
        (
            "SwitchName".to_owned(),
            <SwitchName as utoipa::PartialSchema>::schema(),
        ),
        (
            "SwitchUpdate".to_owned(),
            <SwitchUpdate as utoipa::PartialSchema>::schema(),
        ),
        (
            "Switch".to_owned(),
            <Switch as utoipa::PartialSchema>::schema(),
        ),
        (
            "PlatformStatus".to_owned(),
            <PlatformStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "MailboxRestoreRequest".to_owned(),
            <MailboxRestoreRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "MailboxRestoreResult".to_owned(),
            <MailboxRestoreResult as utoipa::PartialSchema>::schema(),
        ),
        (
            "WaitlistInviteRequest".to_owned(),
            <WaitlistInviteRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "WaitlistInviteResult".to_owned(),
            <WaitlistInviteResult as utoipa::PartialSchema>::schema(),
        ),
        (
            "SnsMessage".to_owned(),
            <SnsMessage as utoipa::PartialSchema>::schema(),
        ),
        ("Jwks".to_owned(), <Jwks as utoipa::PartialSchema>::schema()),
        ("Jwk".to_owned(), <Jwk as utoipa::PartialSchema>::schema()),
        (
            "IdentityKeyKid".to_owned(),
            <IdentityKeyKid as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityKeyStatus".to_owned(),
            <IdentityKeyStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityKey".to_owned(),
            <IdentityKey as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityKeyPage".to_owned(),
            <IdentityKeyPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityKeyRotation".to_owned(),
            <IdentityKeyRotation as utoipa::PartialSchema>::schema(),
        ),
        (
            "AssertionRequest".to_owned(),
            <AssertionRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "AssertionResponse".to_owned(),
            <AssertionResponse as utoipa::PartialSchema>::schema(),
        ),
        (
            "HttpSignatureRequest".to_owned(),
            <HttpSignatureRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "HttpSignatureResponse".to_owned(),
            <HttpSignatureResponse as utoipa::PartialSchema>::schema(),
        ),
        (
            "HttpMessageSignaturesDirectory".to_owned(),
            <HttpMessageSignaturesDirectory as utoipa::PartialSchema>::schema(),
        ),
        (
            "DirectoryJwk".to_owned(),
            <DirectoryJwk as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantPage".to_owned(),
            <TenantPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "PartnerPage".to_owned(),
            <PartnerPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityPage".to_owned(),
            <IdentityPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "AddressPage".to_owned(),
            <AddressPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainPage".to_owned(),
            <DomainPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "ThreadPage".to_owned(),
            <ThreadPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessagePage".to_owned(),
            <MessagePage as utoipa::PartialSchema>::schema(),
        ),
        (
            "ContactPage".to_owned(),
            <ContactPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookPage".to_owned(),
            <WebhookPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookDeliveryPage".to_owned(),
            <WebhookDeliveryPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "SuppressionPage".to_owned(),
            <SuppressionPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftPage".to_owned(),
            <DraftPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftIndexPage".to_owned(),
            <DraftIndexPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "UploadPage".to_owned(),
            <UploadPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventConsumerPage".to_owned(),
            <EventConsumerPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "AgentSignupPage".to_owned(),
            <AgentSignupPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "ListEntryPage".to_owned(),
            <ListEntryPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "ApiKeyPage".to_owned(),
            <ApiKeyPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureRequestPage".to_owned(),
            <ErasureRequestPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "AuditEventPage".to_owned(),
            <AuditEventPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventType".to_owned(),
            <EventType as utoipa::PartialSchema>::schema(),
        ),
        (
            "Event".to_owned(),
            <Event as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageReceivedEvent".to_owned(),
            <MessageReceivedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageQuarantinedEvent".to_owned(),
            <MessageQuarantinedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageReleasedEvent".to_owned(),
            <MessageReleasedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageTriagedEvent".to_owned(),
            <MessageTriagedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageScreenedEvent".to_owned(),
            <MessageScreenedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageSentEvent".to_owned(),
            <MessageSentEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageDeliveredEvent".to_owned(),
            <MessageDeliveredEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageDeferredEvent".to_owned(),
            <MessageDeferredEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageBouncedEvent".to_owned(),
            <MessageBouncedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageComplainedEvent".to_owned(),
            <MessageComplainedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageRejectedEvent".to_owned(),
            <MessageRejectedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageFailedEvent".to_owned(),
            <MessageFailedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageUncertainEvent".to_owned(),
            <MessageUncertainEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageReconciledEvent".to_owned(),
            <MessageReconciledEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageSuppressedEvent".to_owned(),
            <MessageSuppressedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageCanceledEvent".to_owned(),
            <MessageCanceledEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageLabelsChangedEvent".to_owned(),
            <MessageLabelsChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VerificationReceivedEvent".to_owned(),
            <VerificationReceivedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "CalendarReceivedEvent".to_owned(),
            <CalendarReceivedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftApprovalRequestedEvent".to_owned(),
            <DraftApprovalRequestedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftApprovedEvent".to_owned(),
            <DraftApprovedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftRejectedEvent".to_owned(),
            <DraftRejectedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftSentEvent".to_owned(),
            <DraftSentEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftFailedEvent".to_owned(),
            <DraftFailedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityCreatedEvent".to_owned(),
            <IdentityCreatedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityUpdatedEvent".to_owned(),
            <IdentityUpdatedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityPausedEvent".to_owned(),
            <IdentityPausedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityResumedEvent".to_owned(),
            <IdentityResumedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityDeletedEvent".to_owned(),
            <IdentityDeletedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityAddressAddedEvent".to_owned(),
            <IdentityAddressAddedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityAddressActivatedEvent".to_owned(),
            <IdentityAddressActivatedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityAddressPromotedEvent".to_owned(),
            <IdentityAddressPromotedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityAddressRetiredEvent".to_owned(),
            <IdentityAddressRetiredEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityKeyCreatedEvent".to_owned(),
            <IdentityKeyCreatedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityKeyRotatedEvent".to_owned(),
            <IdentityKeyRotatedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityKeyRevokedEvent".to_owned(),
            <IdentityKeyRevokedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainCreatedEvent".to_owned(),
            <DomainCreatedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainVerifiedEvent".to_owned(),
            <DomainVerifiedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainDegradedEvent".to_owned(),
            <DomainDegradedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainFailingEvent".to_owned(),
            <DomainFailingEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainSuspendedEvent".to_owned(),
            <DomainSuspendedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainRecoveredEvent".to_owned(),
            <DomainRecoveredEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainReminderEvent".to_owned(),
            <DomainReminderEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainRemovedEvent".to_owned(),
            <DomainRemovedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureCompletedEvent".to_owned(),
            <ErasureCompletedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureFailedEvent".to_owned(),
            <ErasureFailedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "ExportCompletedEvent".to_owned(),
            <ExportCompletedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SuppressionCreatedEvent".to_owned(),
            <SuppressionCreatedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "QuotaWarningEvent".to_owned(),
            <QuotaWarningEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookDisabledEvent".to_owned(),
            <WebhookDisabledEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookTestEvent".to_owned(),
            <WebhookTestEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MemberInvitedEvent".to_owned(),
            <MemberInvitedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MemberJoinedEvent".to_owned(),
            <MemberJoinedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MemberRoleChangedEvent".to_owned(),
            <MemberRoleChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "MemberRemovedEvent".to_owned(),
            <MemberRemovedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "BillingPlanChangedEvent".to_owned(),
            <BillingPlanChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "BillingPaymentFailedEvent".to_owned(),
            <BillingPaymentFailedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "BillingLimitReachedEvent".to_owned(),
            <BillingLimitReachedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantPolicyUpdatedEvent".to_owned(),
            <TenantPolicyUpdatedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SignupVerifiedEvent".to_owned(),
            <SignupVerifiedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "AccountRequestedEvent".to_owned(),
            <AccountRequestedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "AccountApprovedEvent".to_owned(),
            <AccountApprovedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "AccountRejectedEvent".to_owned(),
            <AccountRejectedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "AccountClosedEvent".to_owned(),
            <AccountClosedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsReceivedEvent".to_owned(),
            <SmsReceivedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsQuarantinedEvent".to_owned(),
            <SmsQuarantinedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsReleasedEvent".to_owned(),
            <SmsReleasedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsScreenedEvent".to_owned(),
            <SmsScreenedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsVerificationReceivedEvent".to_owned(),
            <SmsVerificationReceivedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsSentEvent".to_owned(),
            <SmsSentEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsDeliveredEvent".to_owned(),
            <SmsDeliveredEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsFailedEvent".to_owned(),
            <SmsFailedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsDeliveryUnconfirmedEvent".to_owned(),
            <SmsDeliveryUnconfirmedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsUncertainEvent".to_owned(),
            <SmsUncertainEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsReconciledEvent".to_owned(),
            <SmsReconciledEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "CallStartedEvent".to_owned(),
            <CallStartedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "CallEndedEvent".to_owned(),
            <CallEndedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestRequestedEvent".to_owned(),
            <PhoneRequestRequestedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestApprovedEvent".to_owned(),
            <PhoneRequestApprovedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestRejectedEvent".to_owned(),
            <PhoneRequestRejectedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestCanceledEvent".to_owned(),
            <PhoneRequestCanceledEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestCompletedEvent".to_owned(),
            <PhoneRequestCompletedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestFailedEvent".to_owned(),
            <PhoneRequestFailedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestUncertainEvent".to_owned(),
            <PhoneRequestUncertainEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberStatusChangedEvent".to_owned(),
            <PhoneNumberStatusChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberCapabilityChangedEvent".to_owned(),
            <PhoneNumberCapabilityChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberExpiringEvent".to_owned(),
            <PhoneNumberExpiringEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberActionNeededEvent".to_owned(),
            <PhoneNumberActionNeededEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberPortOutRequestedEvent".to_owned(),
            <PhoneNumberPortOutRequestedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberPortOutDecidedEvent".to_owned(),
            <PhoneNumberPortOutDecidedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRegistrationStatusChangedEvent".to_owned(),
            <PhoneRegistrationStatusChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneComplianceStatusChangedEvent".to_owned(),
            <PhoneComplianceStatusChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneConsentChangedEvent".to_owned(),
            <PhoneConsentChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneSpendWarningEvent".to_owned(),
            <PhoneSpendWarningEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneSetupStatusChangedEvent".to_owned(),
            <PhoneSetupStatusChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneBudgetChangedEvent".to_owned(),
            <PhoneBudgetChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemCreatedEvent".to_owned(),
            <VaultItemCreatedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemUpdatedEvent".to_owned(),
            <VaultItemUpdatedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemRotatedEvent".to_owned(),
            <VaultItemRotatedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemDeletedEvent".to_owned(),
            <VaultItemDeletedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemRevealedEvent".to_owned(),
            <VaultItemRevealedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemHealthChangedEvent".to_owned(),
            <VaultItemHealthChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultGrantRequestedEvent".to_owned(),
            <VaultGrantRequestedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultGrantApprovedEvent".to_owned(),
            <VaultGrantApprovedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultGrantDeniedEvent".to_owned(),
            <VaultGrantDeniedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultGrantRevokedEvent".to_owned(),
            <VaultGrantRevokedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultGrantExpiredEvent".to_owned(),
            <VaultGrantExpiredEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseRequestedEvent".to_owned(),
            <VaultUseRequestedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseApprovalRequestedEvent".to_owned(),
            <VaultUseApprovalRequestedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseApprovedEvent".to_owned(),
            <VaultUseApprovedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseDeniedEvent".to_owned(),
            <VaultUseDeniedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseExpiredEvent".to_owned(),
            <VaultUseExpiredEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseCanceledEvent".to_owned(),
            <VaultUseCanceledEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseClosedEvent".to_owned(),
            <VaultUseClosedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultTicketIssuedEvent".to_owned(),
            <VaultTicketIssuedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultTicketRedeemedEvent".to_owned(),
            <VaultTicketRedeemedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultTicketRefusedEvent".to_owned(),
            <VaultTicketRefusedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultTicketReportedEvent".to_owned(),
            <VaultTicketReportedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultConsumerChangedEvent".to_owned(),
            <VaultConsumerChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultKeyChangedEvent".to_owned(),
            <VaultKeyChangedEvent as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsEventSummary".to_owned(),
            <SmsEventSummary as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultEventAudit".to_owned(),
            <VaultEventAudit as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsReceivedData".to_owned(),
            <SmsReceivedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsQuarantinedData".to_owned(),
            <SmsQuarantinedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsReleasedData".to_owned(),
            <SmsReleasedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsScreenedData".to_owned(),
            <SmsScreenedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsVerificationReceivedData".to_owned(),
            <SmsVerificationReceivedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsSentData".to_owned(),
            <SmsSentData as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsDeliveredData".to_owned(),
            <SmsDeliveredData as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsFailedData".to_owned(),
            <SmsFailedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsDeliveryUnconfirmedData".to_owned(),
            <SmsDeliveryUnconfirmedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsUncertainData".to_owned(),
            <SmsUncertainData as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsReconciledData".to_owned(),
            <SmsReconciledData as utoipa::PartialSchema>::schema(),
        ),
        (
            "CallEventData".to_owned(),
            <CallEventData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestRequestedData".to_owned(),
            <PhoneRequestRequestedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestApprovedData".to_owned(),
            <PhoneRequestApprovedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestRejectedData".to_owned(),
            <PhoneRequestRejectedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestCanceledData".to_owned(),
            <PhoneRequestCanceledData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestCompletedData".to_owned(),
            <PhoneRequestCompletedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestFailedData".to_owned(),
            <PhoneRequestFailedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestUncertainData".to_owned(),
            <PhoneRequestUncertainData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberStatusChangedData".to_owned(),
            <PhoneNumberStatusChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberCapabilityChangedData".to_owned(),
            <PhoneNumberCapabilityChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberExpiringData".to_owned(),
            <PhoneNumberExpiringData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberActionNeededData".to_owned(),
            <PhoneNumberActionNeededData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberPortOutRequestedData".to_owned(),
            <PhoneNumberPortOutRequestedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberPortOutDecidedData".to_owned(),
            <PhoneNumberPortOutDecidedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRegistrationStatusChangedData".to_owned(),
            <PhoneRegistrationStatusChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneComplianceStatusChangedData".to_owned(),
            <PhoneComplianceStatusChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneConsentChangedData".to_owned(),
            <PhoneConsentChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneSpendWarningData".to_owned(),
            <PhoneSpendWarningData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneSetupStatusChangedData".to_owned(),
            <PhoneSetupStatusChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneBudgetChangedData".to_owned(),
            <PhoneBudgetChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemEventData".to_owned(),
            <VaultItemEventData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemDeletedData".to_owned(),
            <VaultItemDeletedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemRevealedData".to_owned(),
            <VaultItemRevealedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemHealthChangedData".to_owned(),
            <VaultItemHealthChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultGrantEventData".to_owned(),
            <VaultGrantEventData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseEventData".to_owned(),
            <VaultUseEventData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultTicketIssuedData".to_owned(),
            <VaultTicketIssuedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultTicketRedeemedData".to_owned(),
            <VaultTicketRedeemedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultTicketRefusedData".to_owned(),
            <VaultTicketRefusedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultTicketReportedData".to_owned(),
            <VaultTicketReportedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultConsumerChangedData".to_owned(),
            <VaultConsumerChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultKeyChangedData".to_owned(),
            <VaultKeyChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventBase".to_owned(),
            <EventBase as utoipa::PartialSchema>::schema(),
        ),
        (
            "SendFailureReason".to_owned(),
            <SendFailureReason as utoipa::PartialSchema>::schema(),
        ),
        (
            "EventAttachment".to_owned(),
            <EventAttachment as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageReceivedData".to_owned(),
            <MessageReceivedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageQuarantinedData".to_owned(),
            <MessageQuarantinedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageReleasedData".to_owned(),
            <MessageReleasedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageTriagedData".to_owned(),
            <MessageTriagedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageScreenedData".to_owned(),
            <MessageScreenedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageSentData".to_owned(),
            <MessageSentData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageDeliveredData".to_owned(),
            <MessageDeliveredData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageDeferredData".to_owned(),
            <MessageDeferredData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageBouncedData".to_owned(),
            <MessageBouncedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageComplainedData".to_owned(),
            <MessageComplainedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageRejectedData".to_owned(),
            <MessageRejectedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageFailedData".to_owned(),
            <MessageFailedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageUncertainData".to_owned(),
            <MessageUncertainData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageReconciledData".to_owned(),
            <MessageReconciledData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageSuppressedData".to_owned(),
            <MessageSuppressedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageCanceledData".to_owned(),
            <MessageCanceledData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MessageLabelsChangedData".to_owned(),
            <MessageLabelsChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "VerificationReceivedData".to_owned(),
            <VerificationReceivedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "CalendarReceivedData".to_owned(),
            <CalendarReceivedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftSummary".to_owned(),
            <DraftSummary as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftApprovalRequestedData".to_owned(),
            <DraftApprovalRequestedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftApprovedData".to_owned(),
            <DraftApprovedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftRejectedData".to_owned(),
            <DraftRejectedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftSentData".to_owned(),
            <DraftSentData as utoipa::PartialSchema>::schema(),
        ),
        (
            "DraftFailedData".to_owned(),
            <DraftFailedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityCreatedData".to_owned(),
            <IdentityCreatedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityUpdatedData".to_owned(),
            <IdentityUpdatedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityPausedData".to_owned(),
            <IdentityPausedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityResumedData".to_owned(),
            <IdentityResumedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityDeletedData".to_owned(),
            <IdentityDeletedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityAddressData".to_owned(),
            <IdentityAddressData as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityAddressPromotedData".to_owned(),
            <IdentityAddressPromotedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityKeyEventData".to_owned(),
            <IdentityKeyEventData as utoipa::PartialSchema>::schema(),
        ),
        (
            "IdentityKeyRotatedData".to_owned(),
            <IdentityKeyRotatedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainEventData".to_owned(),
            <DomainEventData as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainDegradedData".to_owned(),
            <DomainDegradedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainFailingData".to_owned(),
            <DomainFailingData as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainSuspendedData".to_owned(),
            <DomainSuspendedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainRecoveredData".to_owned(),
            <DomainRecoveredData as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainReminderData".to_owned(),
            <DomainReminderData as utoipa::PartialSchema>::schema(),
        ),
        (
            "DomainRemovedData".to_owned(),
            <DomainRemovedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureCompletedData".to_owned(),
            <ErasureCompletedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "ErasureFailedData".to_owned(),
            <ErasureFailedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "ExportCompletedData".to_owned(),
            <ExportCompletedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "SuppressionCreatedData".to_owned(),
            <SuppressionCreatedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "QuotaWarningData".to_owned(),
            <QuotaWarningData as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookDisabledData".to_owned(),
            <WebhookDisabledData as utoipa::PartialSchema>::schema(),
        ),
        (
            "WebhookTestData".to_owned(),
            <WebhookTestData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MemberInvitedData".to_owned(),
            <MemberInvitedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MemberJoinedData".to_owned(),
            <MemberJoinedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MemberRoleChangedData".to_owned(),
            <MemberRoleChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "MemberRemovedData".to_owned(),
            <MemberRemovedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "BillingPlanChangedData".to_owned(),
            <BillingPlanChangedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "BillingPaymentFailedData".to_owned(),
            <BillingPaymentFailedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "BillingLimitReachedData".to_owned(),
            <BillingLimitReachedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "TenantPolicyUpdatedData".to_owned(),
            <TenantPolicyUpdatedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "SignupVerifiedData".to_owned(),
            <SignupVerifiedData as utoipa::PartialSchema>::schema(),
        ),
        (
            "AccountEventData".to_owned(),
            <AccountEventData as utoipa::PartialSchema>::schema(),
        ),
        (
            "AccountRejectedData".to_owned(),
            <AccountRejectedData as utoipa::PartialSchema>::schema(),
        ),
        ("E164".to_owned(), <E164 as utoipa::PartialSchema>::schema()),
        (
            "PhoneNumberId".to_owned(),
            <PhoneNumberId as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberType".to_owned(),
            <PhoneNumberType as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberStatus".to_owned(),
            <PhoneNumberStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "NextStep".to_owned(),
            <NextStep as utoipa::PartialSchema>::schema(),
        ),
        (
            "Capability".to_owned(),
            <Capability as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneCapabilities".to_owned(),
            <PhoneCapabilities as utoipa::PartialSchema>::schema(),
        ),
        (
            "FollowOn".to_owned(),
            <FollowOn as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneQuote".to_owned(),
            <PhoneQuote as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneCountry".to_owned(),
            <PhoneCountry as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneCountryList".to_owned(),
            <PhoneCountryList as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneSetupStep".to_owned(),
            <PhoneSetupStep as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneSetup".to_owned(),
            <PhoneSetup as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneSetupList".to_owned(),
            <PhoneSetupList as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneSetupDraftFields".to_owned(),
            <PhoneSetupDraftFields as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneSetupDraftRequest".to_owned(),
            <PhoneSetupDraftRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneSetupDraft".to_owned(),
            <PhoneSetupDraft as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneBudgetRequest".to_owned(),
            <PhoneBudgetRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneBudget".to_owned(),
            <PhoneBudget as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneBuyRequest".to_owned(),
            <PhoneBuyRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumber".to_owned(),
            <PhoneNumber as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberPage".to_owned(),
            <PhoneNumberPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneCandidate".to_owned(),
            <PhoneCandidate as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneCandidateList".to_owned(),
            <PhoneCandidateList as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhonePortabilityRequest".to_owned(),
            <PhonePortabilityRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhonePortabilityList".to_owned(),
            <PhonePortabilityList as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberUpdateRequest".to_owned(),
            <PhoneNumberUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberMoveRequest".to_owned(),
            <PhoneNumberMoveRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneNumberLookup".to_owned(),
            <PhoneNumberLookup as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneVoiceRoute".to_owned(),
            <PhoneVoiceRoute as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestKind".to_owned(),
            <PhoneRequestKind as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestStatus".to_owned(),
            <PhoneRequestStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestCreate".to_owned(),
            <PhoneRequestCreate as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequest".to_owned(),
            <PhoneRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestPage".to_owned(),
            <PhoneRequestPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestApproveRequest".to_owned(),
            <PhoneRequestApproveRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestRejectRequest".to_owned(),
            <PhoneRequestRejectRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRequestResolveRequest".to_owned(),
            <PhoneRequestResolveRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneComplianceProfileUpdate".to_owned(),
            <PhoneComplianceProfileUpdate as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneComplianceProfile".to_owned(),
            <PhoneComplianceProfile as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneDocumentCreate".to_owned(),
            <PhoneDocumentCreate as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneDocument".to_owned(),
            <PhoneDocument as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneDocumentUpload".to_owned(),
            <PhoneDocumentUpload as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneDocumentList".to_owned(),
            <PhoneDocumentList as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneExportRequest".to_owned(),
            <PhoneExportRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneExportLink".to_owned(),
            <PhoneExportLink as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneRegistrations".to_owned(),
            <PhoneRegistrations as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneOtpRequest".to_owned(),
            <PhoneOtpRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneConnectorCreate".to_owned(),
            <PhoneConnectorCreate as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneConnectorRotate".to_owned(),
            <PhoneConnectorRotate as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneConnector".to_owned(),
            <PhoneConnector as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneConnectorList".to_owned(),
            <PhoneConnectorList as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhonePortOut".to_owned(),
            <PhonePortOut as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhonePortOutList".to_owned(),
            <PhonePortOutList as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhonePortOutDecision".to_owned(),
            <PhonePortOutDecision as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhonePortOutAuthorization".to_owned(),
            <PhonePortOutAuthorization as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsSendRequest".to_owned(),
            <SmsSendRequest as utoipa::PartialSchema>::schema(),
        ),
        ("Sms".to_owned(), <Sms as utoipa::PartialSchema>::schema()),
        (
            "SmsDryRun".to_owned(),
            <SmsDryRun as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsPage".to_owned(),
            <SmsPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsConversationPage".to_owned(),
            <SmsConversationPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsWaitResult".to_owned(),
            <SmsWaitResult as utoipa::PartialSchema>::schema(),
        ),
        (
            "SmsResolveRequest".to_owned(),
            <SmsResolveRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "CallCreateRequest".to_owned(),
            <CallCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        ("Call".to_owned(), <Call as utoipa::PartialSchema>::schema()),
        (
            "CallOrRequest".to_owned(),
            <CallOrRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "CallPage".to_owned(),
            <CallPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneUsage".to_owned(),
            <PhoneUsage as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneConsentRequest".to_owned(),
            <PhoneConsentRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneConsent".to_owned(),
            <PhoneConsent as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneCharges".to_owned(),
            <PhoneCharges as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneStatement".to_owned(),
            <PhoneStatement as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneAccountProbe".to_owned(),
            <PhoneAccountProbe as utoipa::PartialSchema>::schema(),
        ),
        (
            "PhoneAdoptRequest".to_owned(),
            <PhoneAdoptRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemId".to_owned(),
            <VaultItemId as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultGrantId".to_owned(),
            <VaultGrantId as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseId".to_owned(),
            <VaultUseId as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultConsumerId".to_owned(),
            <VaultConsumerId as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultOrigin".to_owned(),
            <VaultOrigin as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemKind".to_owned(),
            <VaultItemKind as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultApprovalLevel".to_owned(),
            <VaultApprovalLevel as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultOperation".to_owned(),
            <VaultOperation as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultPhase".to_owned(),
            <VaultPhase as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultTrigger".to_owned(),
            <VaultTrigger as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultHealth".to_owned(),
            <VaultHealth as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultOkpJwk".to_owned(),
            <VaultOkpJwk as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultPasswordInput".to_owned(),
            <VaultPasswordInput as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultOtpChannel".to_owned(),
            <VaultOtpChannel as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultSessionState".to_owned(),
            <VaultSessionState as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemCreateRequest".to_owned(),
            <VaultItemCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemUpdateRequest".to_owned(),
            <VaultItemUpdateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemVersionRequest".to_owned(),
            <VaultItemVersionRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItem".to_owned(),
            <VaultItem as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemCreated".to_owned(),
            <VaultItemCreated as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultItemPage".to_owned(),
            <VaultItemPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultShredReceipt".to_owned(),
            <VaultShredReceipt as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultGrantRequest".to_owned(),
            <VaultGrantRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultGrantDecision".to_owned(),
            <VaultGrantDecision as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultGrant".to_owned(),
            <VaultGrant as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultGrantPage".to_owned(),
            <VaultGrantPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseCreateRequest".to_owned(),
            <VaultUseCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUse".to_owned(),
            <VaultUse as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseCreated".to_owned(),
            <VaultUseCreated as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseApproveRequest".to_owned(),
            <VaultUseApproveRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUsePage".to_owned(),
            <VaultUsePage as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultTicket".to_owned(),
            <VaultTicket as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultSignature".to_owned(),
            <VaultSignature as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultConsumerDeclarations".to_owned(),
            <VaultConsumerDeclarations as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultConsumerCreateRequest".to_owned(),
            <VaultConsumerCreateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultConsumerActivateRequest".to_owned(),
            <VaultConsumerActivateRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultConsumerKeysRequest".to_owned(),
            <VaultConsumerKeysRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultConsumer".to_owned(),
            <VaultConsumer as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultConsumerPage".to_owned(),
            <VaultConsumerPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultStatus".to_owned(),
            <VaultStatus as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultCustodyRequest".to_owned(),
            <VaultCustodyRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultCustodyResponse".to_owned(),
            <VaultCustodyResponse as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultRevokeAllRequest".to_owned(),
            <VaultRevokeAllRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultRevokeAllResult".to_owned(),
            <VaultRevokeAllResult as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultAuditRow".to_owned(),
            <VaultAuditRow as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultAuditPage".to_owned(),
            <VaultAuditPage as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultRedeemContext".to_owned(),
            <VaultRedeemContext as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultRedeemRequest".to_owned(),
            <VaultRedeemRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultSealedPacket".to_owned(),
            <VaultSealedPacket as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultRedeemResponse".to_owned(),
            <VaultRedeemResponse as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultReportRequest".to_owned(),
            <VaultReportRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultWriteBackRequest".to_owned(),
            <VaultWriteBackRequest as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultConsumerAck".to_owned(),
            <VaultConsumerAck as utoipa::PartialSchema>::schema(),
        ),
        (
            "VaultUseStatus".to_owned(),
            <VaultUseStatus as utoipa::PartialSchema>::schema(),
        ),
    ]
}

/// Deserialises `value` as the component schema `name` and serialises it again; `None` for an unknown name.
#[cfg(test)]
pub fn round_trip(
    name: &str,
    value: &serde_json::Value,
) -> Option<Result<serde_json::Value, String>> {
    fn rt<T: serde::de::DeserializeOwned + serde::Serialize>(
        v: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let t: T = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        serde_json::to_value(&t).map_err(|e| e.to_string())
    }
    Some(match name {
        "TenantId" => rt::<TenantId>(value),
        "PartnerId" => rt::<PartnerId>(value),
        "IdentityId" => rt::<IdentityId>(value),
        "AddressId" => rt::<AddressId>(value),
        "DomainId" => rt::<DomainId>(value),
        "ThreadId" => rt::<ThreadId>(value),
        "MessageId" => rt::<MessageId>(value),
        "AttachmentId" => rt::<AttachmentId>(value),
        "WebhookId" => rt::<WebhookId>(value),
        "DeliveryId" => rt::<DeliveryId>(value),
        "EventId" => rt::<EventId>(value),
        "KeyId" => rt::<KeyId>(value),
        "ErasureRequestId" => rt::<ErasureRequestId>(value),
        "ExportId" => rt::<ExportId>(value),
        "AuditEventId" => rt::<AuditEventId>(value),
        "UserId" => rt::<UserId>(value),
        "InvitationId" => rt::<InvitationId>(value),
        "DlqItemId" => rt::<DlqItemId>(value),
        "JobId" => rt::<JobId>(value),
        "RequestId" => rt::<RequestId>(value),
        "EmailAddress" => rt::<EmailAddress>(value),
        "MailboxAddress" => rt::<MailboxAddress>(value),
        "Label" => rt::<Label>(value),
        "Metadata" => rt::<Metadata>(value),
        "Direction" => rt::<Direction>(value),
        "Error" => rt::<Error>(value),
        "ErrorDetails" => rt::<ErrorDetails>(value),
        "ErrorCode" => rt::<ErrorCode>(value),
        "TransportUnavailableReason" => rt::<TransportUnavailableReason>(value),
        "Permission" => rt::<Permission>(value),
        "KeyLevel" => rt::<KeyLevel>(value),
        "Mode" => rt::<Mode>(value),
        "Me" => rt::<Me>(value),
        "Health" => rt::<Health>(value),
        "TenantStatus" => rt::<TenantStatus>(value),
        "TenantSlug" => rt::<TenantSlug>(value),
        "AddressSuffix" => rt::<AddressSuffix>(value),
        "Tenant" => rt::<Tenant>(value),
        "TenantCreateRequest" => rt::<TenantCreateRequest>(value),
        "TenantUpdateRequest" => rt::<TenantUpdateRequest>(value),
        "TenantPolicy" => rt::<TenantPolicy>(value),
        "TenantPolicyPatch" => rt::<TenantPolicyPatch>(value),
        "TenantPolicyView" => rt::<TenantPolicyView>(value),
        "PolicyFieldAccess" => rt::<PolicyFieldAccess>(value),
        "TriageCategoryDefinition" => rt::<TriageCategoryDefinition>(value),
        "TriageRule" => rt::<TriageRule>(value),
        "TriageRuleMatch" => rt::<TriageRuleMatch>(value),
        "TriageRuleSet" => rt::<TriageRuleSet>(value),
        "CustomRefPattern" => rt::<CustomRefPattern>(value),
        "ServiceAccountId" => rt::<ServiceAccountId>(value),
        "ServiceAccountStatus" => rt::<ServiceAccountStatus>(value),
        "ServiceAccount" => rt::<ServiceAccount>(value),
        "ServiceAccountCreateRequest" => rt::<ServiceAccountCreateRequest>(value),
        "ServiceAccountNoteRequest" => rt::<ServiceAccountNoteRequest>(value),
        "ServiceAccountPage" => rt::<ServiceAccountPage>(value),
        "AgentSignupId" => rt::<AgentSignupId>(value),
        "AgentSignupState" => rt::<AgentSignupState>(value),
        "AgentSignupCreateRequest" => rt::<AgentSignupCreateRequest>(value),
        "AgentSignupCreated" => rt::<AgentSignupCreated>(value),
        "AgentSignup" => rt::<AgentSignup>(value),
        "AgentSignupVerifyRequest" => rt::<AgentSignupVerifyRequest>(value),
        "AgentSignupHumanRequest" => rt::<AgentSignupHumanRequest>(value),
        "AgentSignupEnded" => rt::<AgentSignupEnded>(value),
        "AgentSignupListItem" => rt::<AgentSignupListItem>(value),
        "PartnerStatus" => rt::<PartnerStatus>(value),
        "PartnerBillingMode" => rt::<PartnerBillingMode>(value),
        "Partner" => rt::<Partner>(value),
        "PartnerCreateRequest" => rt::<PartnerCreateRequest>(value),
        "PartnerUpdateRequest" => rt::<PartnerUpdateRequest>(value),
        "PartnerMaxTenants" => rt::<PartnerMaxTenants>(value),
        "PartnerRampExempt" => rt::<PartnerRampExempt>(value),
        "IdentityStatus" => rt::<IdentityStatus>(value),
        "PauseReason" => rt::<PauseReason>(value),
        "Username" => rt::<Username>(value),
        "UsernameInput" => rt::<UsernameInput>(value),
        "DisplayName" => rt::<DisplayName>(value),
        "IdentityOwner" => rt::<IdentityOwner>(value),
        "IdentitySignature" => rt::<IdentitySignature>(value),
        "IdentitySendPolicy" => rt::<IdentitySendPolicy>(value),
        "IdentityRetention" => rt::<IdentityRetention>(value),
        "Identity" => rt::<Identity>(value),
        "IdentityCreateRequest" => rt::<IdentityCreateRequest>(value),
        "IdentityUpdateRequest" => rt::<IdentityUpdateRequest>(value),
        "AddressRole" => rt::<AddressRole>(value),
        "AddressStatus" => rt::<AddressStatus>(value),
        "AddressForwarding" => rt::<AddressForwarding>(value),
        "Address" => rt::<Address>(value),
        "AddressCreateRequest" => rt::<AddressCreateRequest>(value),
        "PromoteAddressRequest" => rt::<PromoteAddressRequest>(value),
        "RetireAddressRequest" => rt::<RetireAddressRequest>(value),
        "DomainMethod" => rt::<DomainMethod>(value),
        "DomainKind" => rt::<DomainKind>(value),
        "DomainInbound" => rt::<DomainInbound>(value),
        "DomainState" => rt::<DomainState>(value),
        "RoutingMode" => rt::<RoutingMode>(value),
        "Transport" => rt::<Transport>(value),
        "ReplyTokenMode" => rt::<ReplyTokenMode>(value),
        "SmtpSettings" => rt::<SmtpSettings>(value),
        "DomainSmtp" => rt::<DomainSmtp>(value),
        "DomainProbe" => rt::<DomainProbe>(value),
        "Domain" => rt::<Domain>(value),
        "DomainCreateRequest" => rt::<DomainCreateRequest>(value),
        "DomainUpdateRequest" => rt::<DomainUpdateRequest>(value),
        "ProbeAccepted" => rt::<ProbeAccepted>(value),
        "DnsRecordStatus" => rt::<DnsRecordStatus>(value),
        "DnsRecord" => rt::<DnsRecord>(value),
        "DomainRecords" => rt::<DomainRecords>(value),
        "DomainIssue" => rt::<DomainIssue>(value),
        "DomainCheck" => rt::<DomainCheck>(value),
        "DomainHealth" => rt::<DomainHealth>(value),
        "ThreadHold" => rt::<ThreadHold>(value),
        "ThreadHoldRequest" => rt::<ThreadHoldRequest>(value),
        "ThreadSummary" => rt::<ThreadSummary>(value),
        "Thread" => rt::<Thread>(value),
        "ThreadUpdateRequest" => rt::<ThreadUpdateRequest>(value),
        "MessageStatus" => rt::<MessageStatus>(value),
        "OutboundStatus" => rt::<OutboundStatus>(value),
        "MessageKind" => rt::<MessageKind>(value),
        "MessageFlag" => rt::<MessageFlag>(value),
        "QuarantineReason" => rt::<QuarantineReason>(value),
        "MessageHeader" => rt::<MessageHeader>(value),
        "Message" => rt::<Message>(value),
        "MessageSummary" => rt::<MessageSummary>(value),
        "MessageUpdateRequest" => rt::<MessageUpdateRequest>(value),
        "BatchMessageIds" => rt::<BatchMessageIds>(value),
        "BatchGetRequest" => rt::<BatchGetRequest>(value),
        "BatchMessage" => rt::<BatchMessage>(value),
        "BatchGetResponse" => rt::<BatchGetResponse>(value),
        "BatchUpdateRequest" => rt::<BatchUpdateRequest>(value),
        "BatchUpdateResponse" => rt::<BatchUpdateResponse>(value),
        "AttachmentRisk" => rt::<AttachmentRisk>(value),
        "AttachmentTextStatus" => rt::<AttachmentTextStatus>(value),
        "Attachment" => rt::<Attachment>(value),
        "AttachmentText" => rt::<AttachmentText>(value),
        "AuthVerdict" => rt::<AuthVerdict>(value),
        "AuthResult" => rt::<AuthResult>(value),
        "TrustFlag" => rt::<TrustFlag>(value),
        "Trust" => rt::<Trust>(value),
        "DkimDomainResult" => rt::<DkimDomainResult>(value),
        "TrustSummary" => rt::<TrustSummary>(value),
        "TriageStatus" => rt::<TriageStatus>(value),
        "BuiltinTriageCategory" => rt::<BuiltinTriageCategory>(value),
        "RiskFlag" => rt::<RiskFlag>(value),
        "Triage" => rt::<Triage>(value),
        "DeliveryStatus" => rt::<DeliveryStatus>(value),
        "Delivery" => rt::<Delivery>(value),
        "Ref" => rt::<Ref>(value),
        "ScreeningStatus" => rt::<ScreeningStatus>(value),
        "ScreeningReason" => rt::<ScreeningReason>(value),
        "ScreeningSignal" => rt::<ScreeningSignal>(value),
        "Screening" => rt::<Screening>(value),
        "ScreeningFeedbackVerdict" => rt::<ScreeningFeedbackVerdict>(value),
        "ScreeningFeedbackRequest" => rt::<ScreeningFeedbackRequest>(value),
        "ScreeningFeedback" => rt::<ScreeningFeedback>(value),
        "ScreeningSummary" => rt::<ScreeningSummary>(value),
        "Recipient" => rt::<Recipient>(value),
        "AttachmentInput" => rt::<AttachmentInput>(value),
        "Unsubscribe" => rt::<Unsubscribe>(value),
        "Consent" => rt::<Consent>(value),
        "SendHeaders" => rt::<SendHeaders>(value),
        "SendRequest" => rt::<SendRequest>(value),
        "ReplyRequest" => rt::<ReplyRequest>(value),
        "ForwardRequest" => rt::<ForwardRequest>(value),
        "SendResponse" => rt::<SendResponse>(value),
        "DryRunResult" => rt::<DryRunResult>(value),
        "DryRunRecipient" => rt::<DryRunRecipient>(value),
        "ResolveRequest" => rt::<ResolveRequest>(value),
        "ReleaseRequest" => rt::<ReleaseRequest>(value),
        "CalendarSendInput" => rt::<CalendarSendInput>(value),
        "CalendarReplyRequest" => rt::<CalendarReplyRequest>(value),
        "CalendarTime" => rt::<CalendarTime>(value),
        "CalendarParty" => rt::<CalendarParty>(value),
        "CalendarAttendee" => rt::<CalendarAttendee>(value),
        "MessageCalendar" => rt::<MessageCalendar>(value),
        "DraftId" => rt::<DraftId>(value),
        "DraftStatus" => rt::<DraftStatus>(value),
        "DraftKind" => rt::<DraftKind>(value),
        "DraftApprovalReason" => rt::<DraftApprovalReason>(value),
        "RequireApproval" => rt::<RequireApproval>(value),
        "DraftFailureReason" => rt::<DraftFailureReason>(value),
        "DraftAttachmentInput" => rt::<DraftAttachmentInput>(value),
        "DraftAttachment" => rt::<DraftAttachment>(value),
        "DraftCreateRequest" => rt::<DraftCreateRequest>(value),
        "DraftUpdateRequest" => rt::<DraftUpdateRequest>(value),
        "DraftSendRequest" => rt::<DraftSendRequest>(value),
        "DraftApproveRequest" => rt::<DraftApproveRequest>(value),
        "DraftRejectRequest" => rt::<DraftRejectRequest>(value),
        "Draft" => rt::<Draft>(value),
        "DraftDryRun" => rt::<DraftDryRun>(value),
        "DraftIndexEntry" => rt::<DraftIndexEntry>(value),
        "UploadId" => rt::<UploadId>(value),
        "UploadStatus" => rt::<UploadStatus>(value),
        "UploadCreateRequest" => rt::<UploadCreateRequest>(value),
        "Upload" => rt::<Upload>(value),
        "AttachmentUploadInput" => rt::<AttachmentUploadInput>(value),
        "SendAttachment" => rt::<SendAttachment>(value),
        "SearchFilters" => rt::<SearchFilters>(value),
        "SearchRequest" => rt::<SearchRequest>(value),
        "TenantSearchRequest" => rt::<TenantSearchRequest>(value),
        "IdentityIdFilter" => rt::<IdentityIdFilter>(value),
        "AgenticBudget" => rt::<AgenticBudget>(value),
        "AgenticRequest" => rt::<AgenticRequest>(value),
        "TenantAgenticRequest" => rt::<TenantAgenticRequest>(value),
        "SearchMode" => rt::<SearchMode>(value),
        "SearchResponse" => rt::<SearchResponse>(value),
        "TenantSearchResponse" => rt::<TenantSearchResponse>(value),
        "AttachmentHit" => rt::<AttachmentHit>(value),
        "SearchHit" => rt::<SearchHit>(value),
        "ThreadHit" => rt::<ThreadHit>(value),
        "FacetCounts" => rt::<FacetCounts>(value),
        "Facets" => rt::<Facets>(value),
        "AgenticStatus" => rt::<AgenticStatus>(value),
        "AgenticSentence" => rt::<AgenticSentence>(value),
        "AgenticAnswer" => rt::<AgenticAnswer>(value),
        "AgenticEvidence" => rt::<AgenticEvidence>(value),
        "AgenticTraceStep" => rt::<AgenticTraceStep>(value),
        "AgenticUsage" => rt::<AgenticUsage>(value),
        "AgenticResponse" => rt::<AgenticResponse>(value),
        "SearchHitList" => rt::<SearchHitList>(value),
        "Contact" => rt::<Contact>(value),
        "Verification" => rt::<Verification>(value),
        "WaitResponse" => rt::<WaitResponse>(value),
        "WebhookEventFilter" => rt::<WebhookEventFilter>(value),
        "Webhook" => rt::<Webhook>(value),
        "WebhookCreated" => rt::<WebhookCreated>(value),
        "WebhookCreateRequest" => rt::<WebhookCreateRequest>(value),
        "WebhookUpdateRequest" => rt::<WebhookUpdateRequest>(value),
        "WebhookHeaderMap" => rt::<WebhookHeaderMap>(value),
        "WebhookHeadersUpdateRequest" => rt::<WebhookHeadersUpdateRequest>(value),
        "RotateRequest" => rt::<RotateRequest>(value),
        "WebhookDeliveryStatus" => rt::<WebhookDeliveryStatus>(value),
        "WebhookDelivery" => rt::<WebhookDelivery>(value),
        "ReplayRequest" => rt::<ReplayRequest>(value),
        "ReplayByIds" => rt::<ReplayByIds>(value),
        "ReplayByRange" => rt::<ReplayByRange>(value),
        "ReplayResponse" => rt::<ReplayResponse>(value),
        "EventConsumerId" => rt::<EventConsumerId>(value),
        "EventCursor" => rt::<EventCursor>(value),
        "EventFeedItem" => rt::<EventFeedItem>(value),
        "EventFeedPage" => rt::<EventFeedPage>(value),
        "EventConsumer" => rt::<EventConsumer>(value),
        "EventConsumerCreateRequest" => rt::<EventConsumerCreateRequest>(value),
        "EventConsumerCommitRequest" => rt::<EventConsumerCommitRequest>(value),
        "EventConsumerResetRequest" => rt::<EventConsumerResetRequest>(value),
        "StreamToken" => rt::<StreamToken>(value),
        "SuppressionReason" => rt::<SuppressionReason>(value),
        "Suppression" => rt::<Suppression>(value),
        "SuppressionCreateRequest" => rt::<SuppressionCreateRequest>(value),
        "SuppressionDeleteRequest" => rt::<SuppressionDeleteRequest>(value),
        "ListDirection" => rt::<ListDirection>(value),
        "ListKind" => rt::<ListKind>(value),
        "ListEntryValue" => rt::<ListEntryValue>(value),
        "ListEntry" => rt::<ListEntry>(value),
        "ListEntryPutRequest" => rt::<ListEntryPutRequest>(value),
        "McpToolName" => rt::<McpToolName>(value),
        "McpToolList" => rt::<McpToolList>(value),
        "ApiKey" => rt::<ApiKey>(value),
        "ApiKeyCreated" => rt::<ApiKeyCreated>(value),
        "ApiKeyCreateRequest" => rt::<ApiKeyCreateRequest>(value),
        "ErasureScope" => rt::<ErasureScope>(value),
        "ErasureStatus" => rt::<ErasureStatus>(value),
        "ErasureRequestCreate" => rt::<ErasureRequestCreate>(value),
        "ErasureRequestCreateMessage" => rt::<ErasureRequestCreateMessage>(value),
        "ErasureRequestCreateThread" => rt::<ErasureRequestCreateThread>(value),
        "ErasureRequestCreateCounterparty" => rt::<ErasureRequestCreateCounterparty>(value),
        "ErasureRequestCreateIdentity" => rt::<ErasureRequestCreateIdentity>(value),
        "ErasureRequestCreateTenant" => rt::<ErasureRequestCreateTenant>(value),
        "ErasureHeldItem" => rt::<ErasureHeldItem>(value),
        "ErasureReceipt" => rt::<ErasureReceipt>(value),
        "ErasureRequest" => rt::<ErasureRequest>(value),
        "ExportScope" => rt::<ExportScope>(value),
        "ExportStatus" => rt::<ExportStatus>(value),
        "ExportCreate" => rt::<ExportCreate>(value),
        "ExportCreateCounterparty" => rt::<ExportCreateCounterparty>(value),
        "ExportCreateIdentity" => rt::<ExportCreateIdentity>(value),
        "Export" => rt::<Export>(value),
        "UsageDay" => rt::<UsageDay>(value),
        "DailyUsage" => rt::<DailyUsage>(value),
        "BillingMode" => rt::<BillingMode>(value),
        "UsageFeatureName" => rt::<UsageFeatureName>(value),
        "UsageFeature" => rt::<UsageFeature>(value),
        "PlanState" => rt::<PlanState>(value),
        "Plan" => rt::<Plan>(value),
        "Topups" => rt::<Topups>(value),
        "UsageSummary" => rt::<UsageSummary>(value),
        "PlanCatalog" => rt::<PlanCatalog>(value),
        "BillingAccount" => rt::<BillingAccount>(value),
        "BillingAccountUpdateRequest" => rt::<BillingAccountUpdateRequest>(value),
        "AuditEvent" => rt::<AuditEvent>(value),
        "MetricEventType" => rt::<MetricEventType>(value),
        "RateWindow" => rt::<RateWindow>(value),
        "SendingStop" => rt::<SendingStop>(value),
        "Rates" => rt::<Rates>(value),
        "EventCounts" => rt::<EventCounts>(value),
        "MemberRole" => rt::<MemberRole>(value),
        "InvitationRole" => rt::<InvitationRole>(value),
        "Member" => rt::<Member>(value),
        "Invitation" => rt::<Invitation>(value),
        "InvitationCreateRequest" => rt::<InvitationCreateRequest>(value),
        "MemberList" => rt::<MemberList>(value),
        "SigningKeyPurpose" => rt::<SigningKeyPurpose>(value),
        "SigningKeyRotation" => rt::<SigningKeyRotation>(value),
        "DlqQueue" => rt::<DlqQueue>(value),
        "DlqItem" => rt::<DlqItem>(value),
        "DlqItemPage" => rt::<DlqItemPage>(value),
        "JobKind" => rt::<JobKind>(value),
        "JobStatus" => rt::<JobStatus>(value),
        "JobCreateRequest" => rt::<JobCreateRequest>(value),
        "Job" => rt::<Job>(value),
        "SwitchName" => rt::<SwitchName>(value),
        "SwitchUpdate" => rt::<SwitchUpdate>(value),
        "Switch" => rt::<Switch>(value),
        "PlatformStatus" => rt::<PlatformStatus>(value),
        "MailboxRestoreRequest" => rt::<MailboxRestoreRequest>(value),
        "MailboxRestoreResult" => rt::<MailboxRestoreResult>(value),
        "WaitlistInviteRequest" => rt::<WaitlistInviteRequest>(value),
        "WaitlistInviteResult" => rt::<WaitlistInviteResult>(value),
        "SnsMessage" => rt::<SnsMessage>(value),
        "Jwks" => rt::<Jwks>(value),
        "Jwk" => rt::<Jwk>(value),
        "IdentityKeyKid" => rt::<IdentityKeyKid>(value),
        "IdentityKeyStatus" => rt::<IdentityKeyStatus>(value),
        "IdentityKey" => rt::<IdentityKey>(value),
        "IdentityKeyPage" => rt::<IdentityKeyPage>(value),
        "IdentityKeyRotation" => rt::<IdentityKeyRotation>(value),
        "AssertionRequest" => rt::<AssertionRequest>(value),
        "AssertionResponse" => rt::<AssertionResponse>(value),
        "HttpSignatureRequest" => rt::<HttpSignatureRequest>(value),
        "HttpSignatureResponse" => rt::<HttpSignatureResponse>(value),
        "HttpMessageSignaturesDirectory" => rt::<HttpMessageSignaturesDirectory>(value),
        "DirectoryJwk" => rt::<DirectoryJwk>(value),
        "TenantPage" => rt::<TenantPage>(value),
        "PartnerPage" => rt::<PartnerPage>(value),
        "IdentityPage" => rt::<IdentityPage>(value),
        "AddressPage" => rt::<AddressPage>(value),
        "DomainPage" => rt::<DomainPage>(value),
        "ThreadPage" => rt::<ThreadPage>(value),
        "MessagePage" => rt::<MessagePage>(value),
        "ContactPage" => rt::<ContactPage>(value),
        "WebhookPage" => rt::<WebhookPage>(value),
        "WebhookDeliveryPage" => rt::<WebhookDeliveryPage>(value),
        "SuppressionPage" => rt::<SuppressionPage>(value),
        "DraftPage" => rt::<DraftPage>(value),
        "DraftIndexPage" => rt::<DraftIndexPage>(value),
        "UploadPage" => rt::<UploadPage>(value),
        "EventConsumerPage" => rt::<EventConsumerPage>(value),
        "AgentSignupPage" => rt::<AgentSignupPage>(value),
        "ListEntryPage" => rt::<ListEntryPage>(value),
        "ApiKeyPage" => rt::<ApiKeyPage>(value),
        "ErasureRequestPage" => rt::<ErasureRequestPage>(value),
        "AuditEventPage" => rt::<AuditEventPage>(value),
        "EventType" => rt::<EventType>(value),
        "Event" => rt::<Event>(value),
        "MessageReceivedEvent" => rt::<MessageReceivedEvent>(value),
        "MessageQuarantinedEvent" => rt::<MessageQuarantinedEvent>(value),
        "MessageReleasedEvent" => rt::<MessageReleasedEvent>(value),
        "MessageTriagedEvent" => rt::<MessageTriagedEvent>(value),
        "MessageScreenedEvent" => rt::<MessageScreenedEvent>(value),
        "MessageSentEvent" => rt::<MessageSentEvent>(value),
        "MessageDeliveredEvent" => rt::<MessageDeliveredEvent>(value),
        "MessageDeferredEvent" => rt::<MessageDeferredEvent>(value),
        "MessageBouncedEvent" => rt::<MessageBouncedEvent>(value),
        "MessageComplainedEvent" => rt::<MessageComplainedEvent>(value),
        "MessageRejectedEvent" => rt::<MessageRejectedEvent>(value),
        "MessageFailedEvent" => rt::<MessageFailedEvent>(value),
        "MessageUncertainEvent" => rt::<MessageUncertainEvent>(value),
        "MessageReconciledEvent" => rt::<MessageReconciledEvent>(value),
        "MessageSuppressedEvent" => rt::<MessageSuppressedEvent>(value),
        "MessageCanceledEvent" => rt::<MessageCanceledEvent>(value),
        "MessageLabelsChangedEvent" => rt::<MessageLabelsChangedEvent>(value),
        "VerificationReceivedEvent" => rt::<VerificationReceivedEvent>(value),
        "CalendarReceivedEvent" => rt::<CalendarReceivedEvent>(value),
        "DraftApprovalRequestedEvent" => rt::<DraftApprovalRequestedEvent>(value),
        "DraftApprovedEvent" => rt::<DraftApprovedEvent>(value),
        "DraftRejectedEvent" => rt::<DraftRejectedEvent>(value),
        "DraftSentEvent" => rt::<DraftSentEvent>(value),
        "DraftFailedEvent" => rt::<DraftFailedEvent>(value),
        "IdentityCreatedEvent" => rt::<IdentityCreatedEvent>(value),
        "IdentityUpdatedEvent" => rt::<IdentityUpdatedEvent>(value),
        "IdentityPausedEvent" => rt::<IdentityPausedEvent>(value),
        "IdentityResumedEvent" => rt::<IdentityResumedEvent>(value),
        "IdentityDeletedEvent" => rt::<IdentityDeletedEvent>(value),
        "IdentityAddressAddedEvent" => rt::<IdentityAddressAddedEvent>(value),
        "IdentityAddressActivatedEvent" => rt::<IdentityAddressActivatedEvent>(value),
        "IdentityAddressPromotedEvent" => rt::<IdentityAddressPromotedEvent>(value),
        "IdentityAddressRetiredEvent" => rt::<IdentityAddressRetiredEvent>(value),
        "IdentityKeyCreatedEvent" => rt::<IdentityKeyCreatedEvent>(value),
        "IdentityKeyRotatedEvent" => rt::<IdentityKeyRotatedEvent>(value),
        "IdentityKeyRevokedEvent" => rt::<IdentityKeyRevokedEvent>(value),
        "DomainCreatedEvent" => rt::<DomainCreatedEvent>(value),
        "DomainVerifiedEvent" => rt::<DomainVerifiedEvent>(value),
        "DomainDegradedEvent" => rt::<DomainDegradedEvent>(value),
        "DomainFailingEvent" => rt::<DomainFailingEvent>(value),
        "DomainSuspendedEvent" => rt::<DomainSuspendedEvent>(value),
        "DomainRecoveredEvent" => rt::<DomainRecoveredEvent>(value),
        "DomainReminderEvent" => rt::<DomainReminderEvent>(value),
        "DomainRemovedEvent" => rt::<DomainRemovedEvent>(value),
        "ErasureCompletedEvent" => rt::<ErasureCompletedEvent>(value),
        "ErasureFailedEvent" => rt::<ErasureFailedEvent>(value),
        "ExportCompletedEvent" => rt::<ExportCompletedEvent>(value),
        "SuppressionCreatedEvent" => rt::<SuppressionCreatedEvent>(value),
        "QuotaWarningEvent" => rt::<QuotaWarningEvent>(value),
        "WebhookDisabledEvent" => rt::<WebhookDisabledEvent>(value),
        "WebhookTestEvent" => rt::<WebhookTestEvent>(value),
        "MemberInvitedEvent" => rt::<MemberInvitedEvent>(value),
        "MemberJoinedEvent" => rt::<MemberJoinedEvent>(value),
        "MemberRoleChangedEvent" => rt::<MemberRoleChangedEvent>(value),
        "MemberRemovedEvent" => rt::<MemberRemovedEvent>(value),
        "BillingPlanChangedEvent" => rt::<BillingPlanChangedEvent>(value),
        "BillingPaymentFailedEvent" => rt::<BillingPaymentFailedEvent>(value),
        "BillingLimitReachedEvent" => rt::<BillingLimitReachedEvent>(value),
        "TenantPolicyUpdatedEvent" => rt::<TenantPolicyUpdatedEvent>(value),
        "SignupVerifiedEvent" => rt::<SignupVerifiedEvent>(value),
        "AccountRequestedEvent" => rt::<AccountRequestedEvent>(value),
        "AccountApprovedEvent" => rt::<AccountApprovedEvent>(value),
        "AccountRejectedEvent" => rt::<AccountRejectedEvent>(value),
        "AccountClosedEvent" => rt::<AccountClosedEvent>(value),
        "SmsReceivedEvent" => rt::<SmsReceivedEvent>(value),
        "SmsQuarantinedEvent" => rt::<SmsQuarantinedEvent>(value),
        "SmsReleasedEvent" => rt::<SmsReleasedEvent>(value),
        "SmsScreenedEvent" => rt::<SmsScreenedEvent>(value),
        "SmsVerificationReceivedEvent" => rt::<SmsVerificationReceivedEvent>(value),
        "SmsSentEvent" => rt::<SmsSentEvent>(value),
        "SmsDeliveredEvent" => rt::<SmsDeliveredEvent>(value),
        "SmsFailedEvent" => rt::<SmsFailedEvent>(value),
        "SmsDeliveryUnconfirmedEvent" => rt::<SmsDeliveryUnconfirmedEvent>(value),
        "SmsUncertainEvent" => rt::<SmsUncertainEvent>(value),
        "SmsReconciledEvent" => rt::<SmsReconciledEvent>(value),
        "CallStartedEvent" => rt::<CallStartedEvent>(value),
        "CallEndedEvent" => rt::<CallEndedEvent>(value),
        "PhoneRequestRequestedEvent" => rt::<PhoneRequestRequestedEvent>(value),
        "PhoneRequestApprovedEvent" => rt::<PhoneRequestApprovedEvent>(value),
        "PhoneRequestRejectedEvent" => rt::<PhoneRequestRejectedEvent>(value),
        "PhoneRequestCanceledEvent" => rt::<PhoneRequestCanceledEvent>(value),
        "PhoneRequestCompletedEvent" => rt::<PhoneRequestCompletedEvent>(value),
        "PhoneRequestFailedEvent" => rt::<PhoneRequestFailedEvent>(value),
        "PhoneRequestUncertainEvent" => rt::<PhoneRequestUncertainEvent>(value),
        "PhoneNumberStatusChangedEvent" => rt::<PhoneNumberStatusChangedEvent>(value),
        "PhoneNumberCapabilityChangedEvent" => rt::<PhoneNumberCapabilityChangedEvent>(value),
        "PhoneNumberExpiringEvent" => rt::<PhoneNumberExpiringEvent>(value),
        "PhoneNumberActionNeededEvent" => rt::<PhoneNumberActionNeededEvent>(value),
        "PhoneNumberPortOutRequestedEvent" => rt::<PhoneNumberPortOutRequestedEvent>(value),
        "PhoneNumberPortOutDecidedEvent" => rt::<PhoneNumberPortOutDecidedEvent>(value),
        "PhoneRegistrationStatusChangedEvent" => rt::<PhoneRegistrationStatusChangedEvent>(value),
        "PhoneComplianceStatusChangedEvent" => rt::<PhoneComplianceStatusChangedEvent>(value),
        "PhoneConsentChangedEvent" => rt::<PhoneConsentChangedEvent>(value),
        "PhoneSpendWarningEvent" => rt::<PhoneSpendWarningEvent>(value),
        "PhoneSetupStatusChangedEvent" => rt::<PhoneSetupStatusChangedEvent>(value),
        "PhoneBudgetChangedEvent" => rt::<PhoneBudgetChangedEvent>(value),
        "VaultItemCreatedEvent" => rt::<VaultItemCreatedEvent>(value),
        "VaultItemUpdatedEvent" => rt::<VaultItemUpdatedEvent>(value),
        "VaultItemRotatedEvent" => rt::<VaultItemRotatedEvent>(value),
        "VaultItemDeletedEvent" => rt::<VaultItemDeletedEvent>(value),
        "VaultItemRevealedEvent" => rt::<VaultItemRevealedEvent>(value),
        "VaultItemHealthChangedEvent" => rt::<VaultItemHealthChangedEvent>(value),
        "VaultGrantRequestedEvent" => rt::<VaultGrantRequestedEvent>(value),
        "VaultGrantApprovedEvent" => rt::<VaultGrantApprovedEvent>(value),
        "VaultGrantDeniedEvent" => rt::<VaultGrantDeniedEvent>(value),
        "VaultGrantRevokedEvent" => rt::<VaultGrantRevokedEvent>(value),
        "VaultGrantExpiredEvent" => rt::<VaultGrantExpiredEvent>(value),
        "VaultUseRequestedEvent" => rt::<VaultUseRequestedEvent>(value),
        "VaultUseApprovalRequestedEvent" => rt::<VaultUseApprovalRequestedEvent>(value),
        "VaultUseApprovedEvent" => rt::<VaultUseApprovedEvent>(value),
        "VaultUseDeniedEvent" => rt::<VaultUseDeniedEvent>(value),
        "VaultUseExpiredEvent" => rt::<VaultUseExpiredEvent>(value),
        "VaultUseCanceledEvent" => rt::<VaultUseCanceledEvent>(value),
        "VaultUseClosedEvent" => rt::<VaultUseClosedEvent>(value),
        "VaultTicketIssuedEvent" => rt::<VaultTicketIssuedEvent>(value),
        "VaultTicketRedeemedEvent" => rt::<VaultTicketRedeemedEvent>(value),
        "VaultTicketRefusedEvent" => rt::<VaultTicketRefusedEvent>(value),
        "VaultTicketReportedEvent" => rt::<VaultTicketReportedEvent>(value),
        "VaultConsumerChangedEvent" => rt::<VaultConsumerChangedEvent>(value),
        "VaultKeyChangedEvent" => rt::<VaultKeyChangedEvent>(value),
        "SmsEventSummary" => rt::<SmsEventSummary>(value),
        "VaultEventAudit" => rt::<VaultEventAudit>(value),
        "SmsReceivedData" => rt::<SmsReceivedData>(value),
        "SmsQuarantinedData" => rt::<SmsQuarantinedData>(value),
        "SmsReleasedData" => rt::<SmsReleasedData>(value),
        "SmsScreenedData" => rt::<SmsScreenedData>(value),
        "SmsVerificationReceivedData" => rt::<SmsVerificationReceivedData>(value),
        "SmsSentData" => rt::<SmsSentData>(value),
        "SmsDeliveredData" => rt::<SmsDeliveredData>(value),
        "SmsFailedData" => rt::<SmsFailedData>(value),
        "SmsDeliveryUnconfirmedData" => rt::<SmsDeliveryUnconfirmedData>(value),
        "SmsUncertainData" => rt::<SmsUncertainData>(value),
        "SmsReconciledData" => rt::<SmsReconciledData>(value),
        "CallEventData" => rt::<CallEventData>(value),
        "PhoneRequestRequestedData" => rt::<PhoneRequestRequestedData>(value),
        "PhoneRequestApprovedData" => rt::<PhoneRequestApprovedData>(value),
        "PhoneRequestRejectedData" => rt::<PhoneRequestRejectedData>(value),
        "PhoneRequestCanceledData" => rt::<PhoneRequestCanceledData>(value),
        "PhoneRequestCompletedData" => rt::<PhoneRequestCompletedData>(value),
        "PhoneRequestFailedData" => rt::<PhoneRequestFailedData>(value),
        "PhoneRequestUncertainData" => rt::<PhoneRequestUncertainData>(value),
        "PhoneNumberStatusChangedData" => rt::<PhoneNumberStatusChangedData>(value),
        "PhoneNumberCapabilityChangedData" => rt::<PhoneNumberCapabilityChangedData>(value),
        "PhoneNumberExpiringData" => rt::<PhoneNumberExpiringData>(value),
        "PhoneNumberActionNeededData" => rt::<PhoneNumberActionNeededData>(value),
        "PhoneNumberPortOutRequestedData" => rt::<PhoneNumberPortOutRequestedData>(value),
        "PhoneNumberPortOutDecidedData" => rt::<PhoneNumberPortOutDecidedData>(value),
        "PhoneRegistrationStatusChangedData" => rt::<PhoneRegistrationStatusChangedData>(value),
        "PhoneComplianceStatusChangedData" => rt::<PhoneComplianceStatusChangedData>(value),
        "PhoneConsentChangedData" => rt::<PhoneConsentChangedData>(value),
        "PhoneSpendWarningData" => rt::<PhoneSpendWarningData>(value),
        "PhoneSetupStatusChangedData" => rt::<PhoneSetupStatusChangedData>(value),
        "PhoneBudgetChangedData" => rt::<PhoneBudgetChangedData>(value),
        "VaultItemEventData" => rt::<VaultItemEventData>(value),
        "VaultItemDeletedData" => rt::<VaultItemDeletedData>(value),
        "VaultItemRevealedData" => rt::<VaultItemRevealedData>(value),
        "VaultItemHealthChangedData" => rt::<VaultItemHealthChangedData>(value),
        "VaultGrantEventData" => rt::<VaultGrantEventData>(value),
        "VaultUseEventData" => rt::<VaultUseEventData>(value),
        "VaultTicketIssuedData" => rt::<VaultTicketIssuedData>(value),
        "VaultTicketRedeemedData" => rt::<VaultTicketRedeemedData>(value),
        "VaultTicketRefusedData" => rt::<VaultTicketRefusedData>(value),
        "VaultTicketReportedData" => rt::<VaultTicketReportedData>(value),
        "VaultConsumerChangedData" => rt::<VaultConsumerChangedData>(value),
        "VaultKeyChangedData" => rt::<VaultKeyChangedData>(value),
        "EventBase" => rt::<EventBase>(value),
        "SendFailureReason" => rt::<SendFailureReason>(value),
        "EventAttachment" => rt::<EventAttachment>(value),
        "MessageReceivedData" => rt::<MessageReceivedData>(value),
        "MessageQuarantinedData" => rt::<MessageQuarantinedData>(value),
        "MessageReleasedData" => rt::<MessageReleasedData>(value),
        "MessageTriagedData" => rt::<MessageTriagedData>(value),
        "MessageScreenedData" => rt::<MessageScreenedData>(value),
        "MessageSentData" => rt::<MessageSentData>(value),
        "MessageDeliveredData" => rt::<MessageDeliveredData>(value),
        "MessageDeferredData" => rt::<MessageDeferredData>(value),
        "MessageBouncedData" => rt::<MessageBouncedData>(value),
        "MessageComplainedData" => rt::<MessageComplainedData>(value),
        "MessageRejectedData" => rt::<MessageRejectedData>(value),
        "MessageFailedData" => rt::<MessageFailedData>(value),
        "MessageUncertainData" => rt::<MessageUncertainData>(value),
        "MessageReconciledData" => rt::<MessageReconciledData>(value),
        "MessageSuppressedData" => rt::<MessageSuppressedData>(value),
        "MessageCanceledData" => rt::<MessageCanceledData>(value),
        "MessageLabelsChangedData" => rt::<MessageLabelsChangedData>(value),
        "VerificationReceivedData" => rt::<VerificationReceivedData>(value),
        "CalendarReceivedData" => rt::<CalendarReceivedData>(value),
        "DraftSummary" => rt::<DraftSummary>(value),
        "DraftApprovalRequestedData" => rt::<DraftApprovalRequestedData>(value),
        "DraftApprovedData" => rt::<DraftApprovedData>(value),
        "DraftRejectedData" => rt::<DraftRejectedData>(value),
        "DraftSentData" => rt::<DraftSentData>(value),
        "DraftFailedData" => rt::<DraftFailedData>(value),
        "IdentityCreatedData" => rt::<IdentityCreatedData>(value),
        "IdentityUpdatedData" => rt::<IdentityUpdatedData>(value),
        "IdentityPausedData" => rt::<IdentityPausedData>(value),
        "IdentityResumedData" => rt::<IdentityResumedData>(value),
        "IdentityDeletedData" => rt::<IdentityDeletedData>(value),
        "IdentityAddressData" => rt::<IdentityAddressData>(value),
        "IdentityAddressPromotedData" => rt::<IdentityAddressPromotedData>(value),
        "IdentityKeyEventData" => rt::<IdentityKeyEventData>(value),
        "IdentityKeyRotatedData" => rt::<IdentityKeyRotatedData>(value),
        "DomainEventData" => rt::<DomainEventData>(value),
        "DomainDegradedData" => rt::<DomainDegradedData>(value),
        "DomainFailingData" => rt::<DomainFailingData>(value),
        "DomainSuspendedData" => rt::<DomainSuspendedData>(value),
        "DomainRecoveredData" => rt::<DomainRecoveredData>(value),
        "DomainReminderData" => rt::<DomainReminderData>(value),
        "DomainRemovedData" => rt::<DomainRemovedData>(value),
        "ErasureCompletedData" => rt::<ErasureCompletedData>(value),
        "ErasureFailedData" => rt::<ErasureFailedData>(value),
        "ExportCompletedData" => rt::<ExportCompletedData>(value),
        "SuppressionCreatedData" => rt::<SuppressionCreatedData>(value),
        "QuotaWarningData" => rt::<QuotaWarningData>(value),
        "WebhookDisabledData" => rt::<WebhookDisabledData>(value),
        "WebhookTestData" => rt::<WebhookTestData>(value),
        "MemberInvitedData" => rt::<MemberInvitedData>(value),
        "MemberJoinedData" => rt::<MemberJoinedData>(value),
        "MemberRoleChangedData" => rt::<MemberRoleChangedData>(value),
        "MemberRemovedData" => rt::<MemberRemovedData>(value),
        "BillingPlanChangedData" => rt::<BillingPlanChangedData>(value),
        "BillingPaymentFailedData" => rt::<BillingPaymentFailedData>(value),
        "BillingLimitReachedData" => rt::<BillingLimitReachedData>(value),
        "TenantPolicyUpdatedData" => rt::<TenantPolicyUpdatedData>(value),
        "SignupVerifiedData" => rt::<SignupVerifiedData>(value),
        "AccountEventData" => rt::<AccountEventData>(value),
        "AccountRejectedData" => rt::<AccountRejectedData>(value),
        "E164" => rt::<E164>(value),
        "PhoneNumberId" => rt::<PhoneNumberId>(value),
        "PhoneNumberType" => rt::<PhoneNumberType>(value),
        "PhoneNumberStatus" => rt::<PhoneNumberStatus>(value),
        "NextStep" => rt::<NextStep>(value),
        "Capability" => rt::<Capability>(value),
        "PhoneCapabilities" => rt::<PhoneCapabilities>(value),
        "FollowOn" => rt::<FollowOn>(value),
        "PhoneQuote" => rt::<PhoneQuote>(value),
        "PhoneCountry" => rt::<PhoneCountry>(value),
        "PhoneCountryList" => rt::<PhoneCountryList>(value),
        "PhoneSetupStep" => rt::<PhoneSetupStep>(value),
        "PhoneSetup" => rt::<PhoneSetup>(value),
        "PhoneSetupList" => rt::<PhoneSetupList>(value),
        "PhoneSetupDraftFields" => rt::<PhoneSetupDraftFields>(value),
        "PhoneSetupDraftRequest" => rt::<PhoneSetupDraftRequest>(value),
        "PhoneSetupDraft" => rt::<PhoneSetupDraft>(value),
        "PhoneBudgetRequest" => rt::<PhoneBudgetRequest>(value),
        "PhoneBudget" => rt::<PhoneBudget>(value),
        "PhoneBuyRequest" => rt::<PhoneBuyRequest>(value),
        "PhoneNumber" => rt::<PhoneNumber>(value),
        "PhoneNumberPage" => rt::<PhoneNumberPage>(value),
        "PhoneCandidate" => rt::<PhoneCandidate>(value),
        "PhoneCandidateList" => rt::<PhoneCandidateList>(value),
        "PhonePortabilityRequest" => rt::<PhonePortabilityRequest>(value),
        "PhonePortabilityList" => rt::<PhonePortabilityList>(value),
        "PhoneNumberUpdateRequest" => rt::<PhoneNumberUpdateRequest>(value),
        "PhoneNumberMoveRequest" => rt::<PhoneNumberMoveRequest>(value),
        "PhoneNumberLookup" => rt::<PhoneNumberLookup>(value),
        "PhoneVoiceRoute" => rt::<PhoneVoiceRoute>(value),
        "PhoneRequestKind" => rt::<PhoneRequestKind>(value),
        "PhoneRequestStatus" => rt::<PhoneRequestStatus>(value),
        "PhoneRequestCreate" => rt::<PhoneRequestCreate>(value),
        "PhoneRequest" => rt::<PhoneRequest>(value),
        "PhoneRequestPage" => rt::<PhoneRequestPage>(value),
        "PhoneRequestApproveRequest" => rt::<PhoneRequestApproveRequest>(value),
        "PhoneRequestRejectRequest" => rt::<PhoneRequestRejectRequest>(value),
        "PhoneRequestResolveRequest" => rt::<PhoneRequestResolveRequest>(value),
        "PhoneComplianceProfileUpdate" => rt::<PhoneComplianceProfileUpdate>(value),
        "PhoneComplianceProfile" => rt::<PhoneComplianceProfile>(value),
        "PhoneDocumentCreate" => rt::<PhoneDocumentCreate>(value),
        "PhoneDocument" => rt::<PhoneDocument>(value),
        "PhoneDocumentUpload" => rt::<PhoneDocumentUpload>(value),
        "PhoneDocumentList" => rt::<PhoneDocumentList>(value),
        "PhoneExportRequest" => rt::<PhoneExportRequest>(value),
        "PhoneExportLink" => rt::<PhoneExportLink>(value),
        "PhoneRegistrations" => rt::<PhoneRegistrations>(value),
        "PhoneOtpRequest" => rt::<PhoneOtpRequest>(value),
        "PhoneConnectorCreate" => rt::<PhoneConnectorCreate>(value),
        "PhoneConnectorRotate" => rt::<PhoneConnectorRotate>(value),
        "PhoneConnector" => rt::<PhoneConnector>(value),
        "PhoneConnectorList" => rt::<PhoneConnectorList>(value),
        "PhonePortOut" => rt::<PhonePortOut>(value),
        "PhonePortOutList" => rt::<PhonePortOutList>(value),
        "PhonePortOutDecision" => rt::<PhonePortOutDecision>(value),
        "PhonePortOutAuthorization" => rt::<PhonePortOutAuthorization>(value),
        "SmsSendRequest" => rt::<SmsSendRequest>(value),
        "Sms" => rt::<Sms>(value),
        "SmsDryRun" => rt::<SmsDryRun>(value),
        "SmsPage" => rt::<SmsPage>(value),
        "SmsConversationPage" => rt::<SmsConversationPage>(value),
        "SmsWaitResult" => rt::<SmsWaitResult>(value),
        "SmsResolveRequest" => rt::<SmsResolveRequest>(value),
        "CallCreateRequest" => rt::<CallCreateRequest>(value),
        "Call" => rt::<Call>(value),
        "CallOrRequest" => rt::<CallOrRequest>(value),
        "CallPage" => rt::<CallPage>(value),
        "PhoneUsage" => rt::<PhoneUsage>(value),
        "PhoneConsentRequest" => rt::<PhoneConsentRequest>(value),
        "PhoneConsent" => rt::<PhoneConsent>(value),
        "PhoneCharges" => rt::<PhoneCharges>(value),
        "PhoneStatement" => rt::<PhoneStatement>(value),
        "PhoneAccountProbe" => rt::<PhoneAccountProbe>(value),
        "PhoneAdoptRequest" => rt::<PhoneAdoptRequest>(value),
        "VaultItemId" => rt::<VaultItemId>(value),
        "VaultGrantId" => rt::<VaultGrantId>(value),
        "VaultUseId" => rt::<VaultUseId>(value),
        "VaultConsumerId" => rt::<VaultConsumerId>(value),
        "VaultOrigin" => rt::<VaultOrigin>(value),
        "VaultItemKind" => rt::<VaultItemKind>(value),
        "VaultApprovalLevel" => rt::<VaultApprovalLevel>(value),
        "VaultOperation" => rt::<VaultOperation>(value),
        "VaultPhase" => rt::<VaultPhase>(value),
        "VaultTrigger" => rt::<VaultTrigger>(value),
        "VaultHealth" => rt::<VaultHealth>(value),
        "VaultOkpJwk" => rt::<VaultOkpJwk>(value),
        "VaultPasswordInput" => rt::<VaultPasswordInput>(value),
        "VaultOtpChannel" => rt::<VaultOtpChannel>(value),
        "VaultSessionState" => rt::<VaultSessionState>(value),
        "VaultItemCreateRequest" => rt::<VaultItemCreateRequest>(value),
        "VaultItemUpdateRequest" => rt::<VaultItemUpdateRequest>(value),
        "VaultItemVersionRequest" => rt::<VaultItemVersionRequest>(value),
        "VaultItem" => rt::<VaultItem>(value),
        "VaultItemCreated" => rt::<VaultItemCreated>(value),
        "VaultItemPage" => rt::<VaultItemPage>(value),
        "VaultShredReceipt" => rt::<VaultShredReceipt>(value),
        "VaultGrantRequest" => rt::<VaultGrantRequest>(value),
        "VaultGrantDecision" => rt::<VaultGrantDecision>(value),
        "VaultGrant" => rt::<VaultGrant>(value),
        "VaultGrantPage" => rt::<VaultGrantPage>(value),
        "VaultUseCreateRequest" => rt::<VaultUseCreateRequest>(value),
        "VaultUse" => rt::<VaultUse>(value),
        "VaultUseCreated" => rt::<VaultUseCreated>(value),
        "VaultUseApproveRequest" => rt::<VaultUseApproveRequest>(value),
        "VaultUsePage" => rt::<VaultUsePage>(value),
        "VaultTicket" => rt::<VaultTicket>(value),
        "VaultSignature" => rt::<VaultSignature>(value),
        "VaultConsumerDeclarations" => rt::<VaultConsumerDeclarations>(value),
        "VaultConsumerCreateRequest" => rt::<VaultConsumerCreateRequest>(value),
        "VaultConsumerActivateRequest" => rt::<VaultConsumerActivateRequest>(value),
        "VaultConsumerKeysRequest" => rt::<VaultConsumerKeysRequest>(value),
        "VaultConsumer" => rt::<VaultConsumer>(value),
        "VaultConsumerPage" => rt::<VaultConsumerPage>(value),
        "VaultStatus" => rt::<VaultStatus>(value),
        "VaultCustodyRequest" => rt::<VaultCustodyRequest>(value),
        "VaultCustodyResponse" => rt::<VaultCustodyResponse>(value),
        "VaultRevokeAllRequest" => rt::<VaultRevokeAllRequest>(value),
        "VaultRevokeAllResult" => rt::<VaultRevokeAllResult>(value),
        "VaultAuditRow" => rt::<VaultAuditRow>(value),
        "VaultAuditPage" => rt::<VaultAuditPage>(value),
        "VaultRedeemContext" => rt::<VaultRedeemContext>(value),
        "VaultRedeemRequest" => rt::<VaultRedeemRequest>(value),
        "VaultSealedPacket" => rt::<VaultSealedPacket>(value),
        "VaultRedeemResponse" => rt::<VaultRedeemResponse>(value),
        "VaultReportRequest" => rt::<VaultReportRequest>(value),
        "VaultWriteBackRequest" => rt::<VaultWriteBackRequest>(value),
        "VaultConsumerAck" => rt::<VaultConsumerAck>(value),
        "VaultUseStatus" => rt::<VaultUseStatus>(value),
        _ => return None,
    })
}
