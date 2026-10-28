//! The compiled-in limit of every `RL_*` binding, per 60 seconds (Configuration › Bindings). The
//! binding enforces it; the Worker needs it for the `RateLimit-Limit` header, and
//! `xtask::ratelimit_limits_match_template` checks it against `deploy/wrangler.toml.tmpl`.

use pylota_identity_api_types::ErrorCode;
use pylota_identity_platform::ratelimit::RlBucket;
use pylota_identity_platform::{Platform, RateLimiter as _};

use crate::errors::ApiError;

/// `(binding, limit, period seconds)`.
pub const LIMITS: &[(&str, u32, u32)] = &[
    ("RL_API", 600, 60),
    ("RL_TENANT", 1_800, 60),
    ("RL_PARTNER_API", 1_800, 60),
    ("RL_SEARCH", 120, 60),
    ("RL_AGENTIC", 20, 60),
    ("RL_SEND", 120, 60),
    ("RL_AI", 60, 60),
    ("RL_EMBED", 600, 60),
    ("RL_SIGNIN", 10, 60),
    ("RL_SIGN", 600, 60),
    ("RL_PARTNER", 10, 60),
];

pub fn limit(bucket: RlBucket) -> u32 {
    LIMITS
        .iter()
        .find(|(b, _, _)| *b == bucket.binding())
        .map_or(0, |(_, l, _)| *l)
}

/// The `details.bucket` name of a refusal.
pub fn bucket_name(bucket: RlBucket) -> &'static str {
    match bucket {
        RlBucket::Api => "key",
        RlBucket::Tenant => "tenant",
        RlBucket::PartnerApi => "partner",
        RlBucket::Search => "search",
        RlBucket::Agentic => "agentic",
        RlBucket::Send => "send",
        RlBucket::Sign => "sign",
        RlBucket::Partner => "partner_create",
        RlBucket::SignIn => "signin",
        RlBucket::Ai => "ai",
        RlBucket::Embed => "embed",
    }
}

/// Asks the bucket; a refusal is `429 rate_limited` with `Retry-After` and `RateLimit-Reset`.
pub async fn check<P: Platform>(p: &P, bucket: RlBucket, key: &str) -> Result<(), ApiError> {
    if p.limits().allow(bucket, key).await? {
        return Ok(());
    }
    let period = LIMITS
        .iter()
        .find(|(b, _, _)| *b == bucket.binding())
        .map_or(60, |(_, _, s)| *s);
    Err(ApiError::new(ErrorCode::RateLimited, "Too many requests.")
        .details(serde_json::json!({ "bucket": bucket_name(bucket) }))
        .header("retry-after", period.to_string())
        .header("ratelimit-reset", period.to_string())
        .header("ratelimit-limit", limit(bucket).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The compiled-in limits equal the fakes' and the bindings' documented ones.
    #[test]
    fn limits_cover_every_bucket() {
        for b in [
            RlBucket::Api,
            RlBucket::Search,
            RlBucket::Agentic,
            RlBucket::Send,
            RlBucket::SignIn,
            RlBucket::Sign,
            RlBucket::Partner,
            RlBucket::Tenant,
            RlBucket::PartnerApi,
            RlBucket::Ai,
            RlBucket::Embed,
        ] {
            assert_eq!(limit(b), b.limit(), "{}", b.binding());
        }
    }
}
