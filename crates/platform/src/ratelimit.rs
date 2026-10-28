//! Rate limiting (bindings `RL_*`). The bindings count per Cloudflare location and are eventually
//! consistent; exact daily caps live in `TenantQuota`.

use crate::PResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RlBucket {
    /// `RL_API`, keyed by API key ID.
    Api,
    /// `RL_SEARCH`, keyed by API key ID.
    Search,
    /// `RL_AGENTIC`, keyed by API key ID.
    Agentic,
    /// `RL_SEND`, keyed by identity ID.
    Send,
    /// `RL_SIGNIN`, keyed by client network.
    SignIn,
    /// `RL_SIGN`, keyed by identity ID (assertions and HTTP signatures).
    Sign,
    /// `RL_PARTNER`, keyed by partner ID (tenant creation and invitations by partner keys).
    Partner,
    /// `RL_TENANT`, keyed by tenant ID (every request of a tenant or identity key).
    Tenant,
    /// `RL_PARTNER_API`, keyed by partner ID (every request of a partner key).
    PartnerApi,
    /// `RL_AI`, keyed by tenant ID (text generation), or `screen:{tenant_id}` for screening.
    Ai,
    /// `RL_EMBED`, keyed by tenant ID.
    Embed,
}

impl RlBucket {
    pub const fn binding(self) -> &'static str {
        match self {
            RlBucket::Api => "RL_API",
            RlBucket::Search => "RL_SEARCH",
            RlBucket::Agentic => "RL_AGENTIC",
            RlBucket::Send => "RL_SEND",
            RlBucket::SignIn => "RL_SIGNIN",
            RlBucket::Sign => "RL_SIGN",
            RlBucket::Partner => "RL_PARTNER",
            RlBucket::Tenant => "RL_TENANT",
            RlBucket::PartnerApi => "RL_PARTNER_API",
            RlBucket::Ai => "RL_AI",
            RlBucket::Embed => "RL_EMBED",
        }
    }

    /// Requests per 60 seconds (Configuration › Bindings); the fake uses the same limits.
    pub const fn limit(self) -> u32 {
        match self {
            RlBucket::Api | RlBucket::Embed | RlBucket::Sign => 600,
            RlBucket::Tenant | RlBucket::PartnerApi => 1_800,
            RlBucket::Search | RlBucket::Send => 120,
            RlBucket::Agentic => 20,
            RlBucket::Ai => 60,
            RlBucket::SignIn | RlBucket::Partner => 10,
        }
    }
}

pub trait RateLimiter {
    async fn allow(&self, bucket: RlBucket, key: &str) -> PResult<bool>;
}
