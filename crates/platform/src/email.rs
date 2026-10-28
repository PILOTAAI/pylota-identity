//! Email Sending (binding `EMAIL`): structured mail through `SendEmail::send_with_builder`, raw MIME
//! through `SendEmail::send`. Each send is raced against `deadline_ms`; losing the race abandons the
//! promise and returns [`SendError::Timeout`].

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutAttachment {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
    /// Set for an inline part referenced as `cid:` from the HTML.
    pub inline_content_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StructuredEmail {
    /// (address, display name)
    pub from: (String, String),
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub reply_to: Option<String>,
    pub subject: String,
    pub text: Option<String>,
    pub html: Option<String>,
    pub headers: Vec<(String, String)>,
    pub attachments: Vec<OutAttachment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendError {
    /// A thrown `Error` with a `code` property.
    Coded { code: String, message: String },
    /// Thrown without a code.
    Exception { message: String },
    /// Our deadline expired before the promise settled.
    Timeout,
}

/// The deadline when the caller has none (outbound.md).
pub const DEFAULT_DEADLINE_MS: u32 = 30_000;

pub trait MailSender {
    /// The provider's `messageId`.
    async fn send(&self, msg: &StructuredEmail, deadline_ms: u32) -> Result<String, SendError>;
    async fn send_raw(
        &self,
        from: &str,
        to: &str,
        mime: &[u8],
        deadline_ms: u32,
    ) -> Result<String, SendError>;
}
