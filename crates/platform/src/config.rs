//! Variables and secrets, read and validated once per isolate ([Rust workspace §6.1],
//! [Configuration]). Every row of the startup rules has one outcome:
//!
//! - a required variable or secret missing, a secret of the wrong length, or an optional variable set
//!   but malformed: [`ConfigInvalid`] naming the variable; every handler then answers `503
//!   unavailable` and `/health` carries `details.config_invalid`;
//! - SES without `PM_SES_SNS_TOPIC_ARN`, and `PM_BILLING=stripe` without its secrets: the Worker starts
//!   with the feature off and [`Config::degraded`] lists why;
//! - an invalid `PM_DOCS_URL`: the Worker starts with the default and [`Config::warnings`] carries
//!   `docs_url_invalid`, because a wrong documentation link must never take the API down.
//!
//! An empty value counts as unset (`wrangler.toml` writes optional variables only when they are set).
//! No value is ever logged or put in an error: only names.
//!
//! [Rust workspace §6.1]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/rust-workspace.md
//! [Configuration]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/configuration.md

use serde_json::Value;

use crate::base64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jurisdiction {
    Eu,
    Default,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeployEnv {
    Production,
    Staging,
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

/// `PM_SYSTEM_FROM`: the system identity (Identities and domains › The system identity).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemFrom {
    pub display_name: String,
    pub address: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MasterSlot {
    A,
    B,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SesInbound {
    pub bucket: String,
    pub topic_arn: String,
    pub queue_url: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SesCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct StripeSecrets {
    pub secret_key: String,
    pub webhook_secret: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignupMode {
    Closed,
    Waitlist,
    Open,
}

/// The console's variables; `None` in [`Config::console`] when `PM_CONSOLE = off`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleConfig {
    pub signup: SignupMode,
    /// Set whenever `signup` is not `Closed`, with the three below.
    pub terms_url: Option<String>,
    pub privacy_url: Option<String>,
    pub dpa_url: Option<String>,
    pub terms_version: Option<String>,
    pub signup_blocked_domains: Vec<String>,
    /// Set only with its secret.
    pub oauth_google_client_id: Option<String>,
    pub oauth_github_client_id: Option<String>,
    /// `PM_QUARANTINE_KEY_RELEASE`, default on.
    pub quarantine_key_release: bool,
    pub agent_signup: bool,
    pub agent_signup_daily_cap: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BillingMode {
    Off,
    Stripe,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BillingConfig {
    pub mode: BillingMode,
    /// `false` with `PM_BILLING=stripe` but a Stripe secret missing: every workspace behaves as
    /// billing `disabled` and the Stripe routes answer `503`.
    pub started: bool,
    /// `PM_PLAN_CATALOG`; `None` means the built-in catalog.
    pub plan_catalog: Option<Value>,
    pub grace_days: u32,
}

/// Phone numbers; `None` in [`Config::phone`] when `PM_PHONE = off`.
#[derive(Clone, Debug, PartialEq)]
pub struct PhoneConfig {
    pub telnyx_api_base: String,
    pub telnyx_account_id: String,
    /// Without it `/hooks/telnyx` answers `503` (`not_configured`).
    pub telnyx_public_key: Option<String>,
    /// `None` means the built-in catalogue or rate card.
    pub catalogue: Option<Value>,
    pub rate_card: Option<Value>,
    pub kyc_retention_days: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VaultConfig {
    pub enabled: bool,
    pub refused_domains: Vec<String>,
    pub clearance_cookies: Vec<String>,
}

#[derive(Debug)]
pub struct Config {
    pub platform_domain: String,
    pub api_host: String,
    pub jurisdiction: Jurisdiction,
    /// `PM_CONSOLE_HOST`, default `api_host`; set even with `PM_CONSOLE = off`.
    pub console_host: String,
    pub system_from: SystemFrom,
    pub env: DeployEnv,
    pub embed_model: String,
    /// `None` when `PM_RERANK_MODEL = none`.
    pub rerank_model: Option<String>,
    pub embed_model_previous: Option<String>,
    pub agent_model: String,
    pub triage_model: String,
    pub screen_model: String,
    pub ai_gateway: Option<String>,
    /// No trailing `/`.
    pub docs_url: String,
    pub trusted_authserv_id: Option<String>,
    pub doh_resolvers: [String; 2],
    pub ses_region: Option<String>,
    pub ses_sns_topic_arn: Option<String>,
    pub scanner_url: Option<String>,
    pub security_contact: Option<String>,
    pub log_level: LogLevel,
    /// A JSON object merged over the built-in tenant policy defaults.
    pub default_policy: Value,
    pub cf_account_id: Option<String>,
    pub daily_send_quota: Option<u32>,
    pub backup_bucket: Option<String>,
    /// `PM_ALERT_EMAIL`, else the `mailto:` address of `PM_SECURITY_CONTACT`.
    pub alert_email: Option<String>,
    pub heartbeat_key_id: Option<String>,
    pub break_glass_key_id: Option<String>,
    pub mailbox_provider_domains: Vec<String>,
    pub frozen: bool,
    pub frozen_since: Option<String>,
    /// Honoured only with `PM_ENV = local`.
    pub eval_rest: bool,
    pub ses_inbound: Option<SesInbound>,
    pub ses_rule_set: String,
    pub cf_subdomain_setup: bool,
    pub web_bot_auth: bool,
    pub identity_key_overlap_days: u32,
    pub notifications: bool,
    pub console: Option<ConsoleConfig>,
    pub billing: BillingConfig,
    pub phone: Option<PhoneConfig>,
    pub vault: VaultConfig,
    pub secrets: Secrets,
    /// What `/health` reports as `degraded`: `("ses", "sns_topic_missing")`,
    /// `("billing", "stripe_secrets_missing")`.
    pub degraded: Vec<(&'static str, &'static str)>,
    /// Logged once per isolate (`docs_url_invalid`).
    pub warnings: Vec<&'static str>,
    /// `PM_ITEST_FAKES_URL` and `PM_ITEST_TOKEN`: read only with `PM_ENV = local` (Testing § 6.1–6.2).
    pub itest_fakes_url: Option<String>,
    pub itest_token: Option<String>,
}

/// Each decoded from base64; 32 bytes for the `PM_*` keys. `Debug` prints which are set, never a value.
pub struct Secrets {
    pub master_key_a: Option<[u8; 32]>,
    pub master_key_b: Option<[u8; 32]>,
    pub master_key_active: MasterSlot,
    pub key_pepper: [u8; 32],
    pub hash_key: [u8; 32],
    pub cf_api_token: Option<String>,
    /// `None` unless the SES transport is fully configured (with `PM_SES_SNS_TOPIC_ARN`).
    pub ses: Option<SesCredentials>,
    /// `None` unless `PM_BILLING = stripe` and both secrets are set.
    pub stripe: Option<StripeSecrets>,
    pub oauth_google_secret: Option<String>,
    pub oauth_github_secret: Option<String>,
    pub telnyx_api_key: Option<String>,
}

impl Secrets {
    /// The key of the active master-key slot (always set: startup refuses an empty active slot).
    pub fn active_master_key(&self) -> &[u8; 32] {
        let slot = match self.master_key_active {
            MasterSlot::A => &self.master_key_a,
            MasterSlot::B => &self.master_key_b,
        };
        slot.as_ref().expect("startup refuses an empty active slot")
    }
}

impl core::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Secrets")
            .field("master_key_a", &self.master_key_a.is_some())
            .field("master_key_b", &self.master_key_b.is_some())
            .field("master_key_active", &self.master_key_active)
            .field("cf_api_token", &self.cf_api_token.is_some())
            .field("ses", &self.ses.is_some())
            .field("stripe", &self.stripe.is_some())
            .field("oauth_google_secret", &self.oauth_google_secret.is_some())
            .field("oauth_github_secret", &self.oauth_github_secret.is_some())
            .field("telnyx_api_key", &self.telnyx_api_key.is_some())
            .finish_non_exhaustive()
    }
}

/// The startup error: the variable or secret to fix, and why, never its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigInvalid {
    pub name: &'static str,
    pub reason: &'static str,
}

impl core::fmt::Display for ConfigInvalid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "config_invalid {}: {}", self.name, self.reason)
    }
}

/// What the release was built with, for the rows that depend on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Build {
    /// Signed HTTP requests (Web Bot Auth) are in the release: spike S13 passed (2026-10-27).
    pub signed_requests: bool,
}

impl Build {
    pub const RELEASE: Build = Build {
        signed_requests: true,
    };
}

pub const DEFAULT_DOCS_URL: &str = "https://pylotamail.com/docs";
pub const DEFAULT_DOH_RESOLVERS: [&str; 2] = [
    "https://cloudflare-dns.com/dns-query",
    "https://dns.google/resolve",
];
pub const TELNYX_API_BASES: [&str; 2] = ["https://api.telnyx.com/v2", "https://api.telnyx.eu/v2"];

type Res<T> = Result<T, ConfigInvalid>;

fn invalid(name: &'static str, reason: &'static str) -> ConfigInvalid {
    ConfigInvalid { name, reason }
}

/// Reads one name at a time; empty values are unset.
struct Vars<'a> {
    get: &'a dyn Fn(&str) -> Option<String>,
}

impl Vars<'_> {
    fn opt(&self, name: &'static str) -> Option<String> {
        (self.get)(name)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    }

    fn required(&self, name: &'static str) -> Res<String> {
        self.opt(name).ok_or(invalid(name, "missing"))
    }

    fn or(&self, name: &'static str, default: &str) -> String {
        self.opt(name).unwrap_or_else(|| default.to_owned())
    }

    fn one_of<T: Copy>(&self, name: &'static str, default: T, allowed: &[(&str, T)]) -> Res<T> {
        match self.opt(name) {
            None => Ok(default),
            Some(v) => allowed
                .iter()
                .find(|(s, _)| *s == v)
                .map(|(_, t)| *t)
                .ok_or(invalid(name, "not an allowed value")),
        }
    }

    fn on_off(&self, name: &'static str, default: bool) -> Res<bool> {
        self.one_of(name, default, &[("on", true), ("off", false)])
    }

    fn number(&self, name: &'static str, min: u32, max: u32) -> Res<Option<u32>> {
        match self.opt(name) {
            None => Ok(None),
            Some(v) => match v.parse::<u32>() {
                Ok(n) if (min..=max).contains(&n) => Ok(Some(n)),
                Ok(_) => Err(invalid(name, "out of range")),
                Err(_) => Err(invalid(name, "not a number")),
            },
        }
    }

    fn json(&self, name: &'static str) -> Res<Option<Value>> {
        match self.opt(name) {
            None => Ok(None),
            Some(v) => serde_json::from_str(&v)
                .map(Some)
                .map_err(|_| invalid(name, "invalid JSON")),
        }
    }

    fn list(&self, name: &'static str, lower: bool) -> Vec<String> {
        self.opt(name)
            .map(|v| {
                v.split(',')
                    .map(|s| {
                        let s = s.trim();
                        if lower {
                            s.to_ascii_lowercase()
                        } else {
                            s.to_owned()
                        }
                    })
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A base64 secret of exactly 32 bytes.
    fn key(&self, name: &'static str) -> Res<Option<[u8; 32]>> {
        match self.opt(name) {
            None => Ok(None),
            Some(v) => base64::decode(&v)
                .and_then(|b| <[u8; 32]>::try_from(b).ok())
                .map(Some)
                .ok_or(invalid(name, "not 32 bytes of base64")),
        }
    }
}

/// `Display Name <local@domain>` or a bare `local@domain`.
fn parse_system_from(text: &str) -> Option<SystemFrom> {
    let (display, address) = match text.rfind('<') {
        Some(open) => {
            let rest = text[open + 1..].strip_suffix('>')?;
            (text[..open].trim().trim_matches('"').trim(), rest.trim())
        }
        None => ("", text.trim()),
    };
    let (local, domain) = address.split_once('@')?;
    let ok = !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !address.contains(char::is_whitespace)
        && !domain.contains('@')
        && !display.contains(['<', '>']);
    ok.then(|| SystemFrom {
        display_name: display.to_owned(),
        address: address.to_ascii_lowercase(),
    })
}

/// `PM_DOCS_URL`: `https`, no query or fragment, at most 200 bytes; a trailing `/` is removed.
fn valid_docs_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split('/').next().unwrap_or("");
    let ok = url.len() <= 200
        && !host.is_empty()
        && !url.contains(['?', '#'])
        && !url.contains(char::is_whitespace);
    ok.then(|| url.trim_end_matches('/').to_owned())
}

fn https_url(url: &str) -> bool {
    url.strip_prefix("https://")
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
        && !url.contains(char::is_whitespace)
}

impl Config {
    /// Reads every variable and secret through `get` (on Cloudflare: the secret of that name, else the
    /// variable) and applies the startup rules.
    pub fn read(
        get: &dyn Fn(&str) -> Option<String>,
        build: Build,
    ) -> Result<Config, ConfigInvalid> {
        let v = Vars { get };
        let mut degraded = Vec::new();
        let mut warnings = Vec::new();

        // Required.
        let platform_domain = v.required("PM_PLATFORM_DOMAIN")?.to_ascii_lowercase();
        let api_host = v.required("PM_API_HOST")?;
        let master_key_a = v.key("PM_MASTER_KEY")?;
        if master_key_a.is_none() {
            return Err(invalid("PM_MASTER_KEY", "missing"));
        }
        let key_pepper = v
            .key("PM_KEY_PEPPER")?
            .ok_or(invalid("PM_KEY_PEPPER", "missing"))?;
        let hash_key = v
            .key("PM_HASH_KEY")?
            .ok_or(invalid("PM_HASH_KEY", "missing"))?;

        // The master-key slots (Security § 6.2).
        let master_key_b = v.key("PM_MASTER_KEY_B")?;
        let master_key_active = v.one_of(
            "PM_MASTER_KEY_ACTIVE",
            MasterSlot::A,
            &[("a", MasterSlot::A), ("b", MasterSlot::B)],
        )?;
        if master_key_active == MasterSlot::B && master_key_b.is_none() {
            return Err(invalid("PM_MASTER_KEY_B", "the active slot is empty"));
        }
        // Equal key IDs (the first 8 bytes of SHA-256 of the key) mean equal keys.
        if master_key_b.is_some() && master_key_a == master_key_b {
            return Err(invalid("PM_MASTER_KEY_B", "same key ID as PM_MASTER_KEY"));
        }

        // Optional, each validated when set.
        let jurisdiction = v.one_of(
            "PM_JURISDICTION",
            Jurisdiction::Eu,
            &[("eu", Jurisdiction::Eu), ("default", Jurisdiction::Default)],
        )?;
        let env = v.one_of(
            "PM_ENV",
            DeployEnv::Production,
            &[
                ("production", DeployEnv::Production),
                ("staging", DeployEnv::Staging),
                ("local", DeployEnv::Local),
            ],
        )?;
        let log_level = v.one_of(
            "PM_LOG_LEVEL",
            LogLevel::Info,
            &[
                ("error", LogLevel::Error),
                ("warn", LogLevel::Warn),
                ("info", LogLevel::Info),
                ("debug", LogLevel::Debug),
            ],
        )?;
        let console_host = v.or("PM_CONSOLE_HOST", &api_host);
        let system_from = match v.opt("PM_SYSTEM_FROM") {
            Some(text) => {
                parse_system_from(&text).ok_or(invalid("PM_SYSTEM_FROM", "unparsable"))?
            }
            None => SystemFrom {
                display_name: "Pylota Identity".to_owned(),
                address: format!("no-reply@{platform_domain}"),
            },
        };
        let doh_resolvers = match v.opt("PM_DOH_RESOLVERS") {
            None => DEFAULT_DOH_RESOLVERS.map(str::to_owned),
            Some(text) => {
                let urls: Vec<&str> = text.split(',').map(str::trim).collect();
                match urls[..] {
                    [a, b] if https_url(a) && https_url(b) => [a.to_owned(), b.to_owned()],
                    _ => {
                        return Err(invalid("PM_DOH_RESOLVERS", "not exactly two https:// URLs"));
                    }
                }
            }
        };
        let default_policy = match v.json("PM_DEFAULT_POLICY")? {
            None => Value::Object(Default::default()),
            Some(p @ Value::Object(_)) => p,
            Some(_) => return Err(invalid("PM_DEFAULT_POLICY", "not a JSON object")),
        };
        let docs_url = match v.opt("PM_DOCS_URL") {
            None => DEFAULT_DOCS_URL.to_owned(),
            Some(url) => valid_docs_url(&url).unwrap_or_else(|| {
                warnings.push("docs_url_invalid");
                DEFAULT_DOCS_URL.to_owned()
            }),
        };
        let rerank_model = match v.or("PM_RERANK_MODEL", "@cf/baai/bge-reranker-base") {
            m if m == "none" => None,
            m => Some(m),
        };
        let daily_send_quota = v.number("PM_DAILY_SEND_QUOTA", 1, u32::MAX)?;
        let identity_key_overlap_days = v
            .number("PM_IDENTITY_KEY_OVERLAP_DAYS", 1, 365)?
            .unwrap_or(7);
        let frozen = v.on_off("PM_FREEZE", false)?;
        let eval_rest = v.on_off("PM_EVAL_REST", false)? && env == DeployEnv::Local;
        let cf_subdomain_setup = v.on_off("PM_CF_SUBDOMAIN_SETUP", false)?;
        let web_bot_auth = v.on_off("PM_WEB_BOT_AUTH", false)?;
        if web_bot_auth && !build.signed_requests {
            return Err(invalid(
                "PM_WEB_BOT_AUTH",
                "this release was built without signed HTTP requests",
            ));
        }
        let notifications = v.on_off("PM_NOTIFICATIONS", true)?;
        let security_contact = v.opt("PM_SECURITY_CONTACT");
        let alert_email = v.opt("PM_ALERT_EMAIL").or_else(|| {
            security_contact
                .as_deref()
                .and_then(|c| c.strip_prefix("mailto:"))
                .map(str::to_owned)
        });

        // SES (both directions).
        let ses_region = v.opt("PM_SES_REGION");
        let ses_sns_topic_arn = v.opt("PM_SES_SNS_TOPIC_ARN");
        let ses_key = v.opt("PM_SES_ACCESS_KEY_ID");
        let ses_secret = v.opt("PM_SES_SECRET_ACCESS_KEY");
        let ses_credentials = match (ses_key, ses_secret) {
            (Some(access_key_id), Some(secret_access_key)) => Some(SesCredentials {
                access_key_id,
                secret_access_key,
            }),
            (None, None) => None,
            (Some(_), None) => return Err(invalid("PM_SES_SECRET_ACCESS_KEY", "missing")),
            (None, Some(_)) => return Err(invalid("PM_SES_ACCESS_KEY_ID", "missing")),
        };
        let ses = match (&ses_credentials, &ses_region, &ses_sns_topic_arn) {
            (Some(_), Some(_), Some(_)) => ses_credentials,
            (Some(_), Some(_), None) => {
                degraded.push(("ses", "sns_topic_missing"));
                None
            }
            _ => None,
        };
        let ses_inbound = match (
            v.opt("PM_SES_INBOUND_BUCKET"),
            v.opt("PM_SES_INBOUND_TOPIC_ARN"),
            v.opt("PM_SES_INBOUND_QUEUE_URL"),
        ) {
            (Some(bucket), Some(topic_arn), Some(queue_url)) => Some(SesInbound {
                bucket,
                topic_arn,
                queue_url,
            }),
            _ => None,
        };

        // Console and sign-up; validated even with PM_CONSOLE = off, so no value is silently ignored.
        let console_on = v.on_off("PM_CONSOLE", true)?;
        let signup = v.one_of(
            "PM_SIGNUP",
            SignupMode::Closed,
            &[
                ("closed", SignupMode::Closed),
                ("waitlist", SignupMode::Waitlist),
                ("open", SignupMode::Open),
            ],
        )?;
        let quarantine_key_release = v.on_off("PM_QUARANTINE_KEY_RELEASE", true)?;
        let agent_signup = v.on_off("PM_AGENT_SIGNUP", false)?;
        let agent_signup_daily_cap = v
            .number("PM_AGENT_SIGNUP_DAILY_CAP", 1, 10_000)?
            .unwrap_or(200);
        let terms_url = v.opt("PM_TERMS_URL");
        let privacy_url = v.opt("PM_PRIVACY_URL");
        let dpa_url = v.opt("PM_DPA_URL");
        let terms_version = v.opt("PM_TERMS_VERSION");
        let google_secret = v.opt("PM_OAUTH_GOOGLE_CLIENT_SECRET");
        let github_secret = v.opt("PM_OAUTH_GITHUB_CLIENT_SECRET");
        let (oauth_google_client_id, oauth_google_secret) =
            match (v.opt("PM_OAUTH_GOOGLE_CLIENT_ID"), google_secret) {
                (Some(id), Some(secret)) => (Some(id), Some(secret)),
                _ => (None, None),
            };
        let (oauth_github_client_id, oauth_github_secret) =
            match (v.opt("PM_OAUTH_GITHUB_CLIENT_ID"), github_secret) {
                (Some(id), Some(secret)) => (Some(id), Some(secret)),
                _ => (None, None),
            };
        let console = if console_on {
            if signup != SignupMode::Closed {
                for (name, value) in [
                    ("PM_TERMS_URL", &terms_url),
                    ("PM_PRIVACY_URL", &privacy_url),
                    ("PM_DPA_URL", &dpa_url),
                    ("PM_TERMS_VERSION", &terms_version),
                ] {
                    if value.is_none() {
                        return Err(invalid(name, "required when PM_SIGNUP is not closed"));
                    }
                }
            }
            Some(ConsoleConfig {
                signup,
                terms_url,
                privacy_url,
                dpa_url,
                terms_version,
                signup_blocked_domains: v.list("PM_SIGNUP_BLOCKED_DOMAINS", true),
                oauth_google_client_id,
                oauth_github_client_id,
                quarantine_key_release,
                agent_signup,
                agent_signup_daily_cap,
            })
        } else {
            None
        };

        // Billing.
        let billing_mode = v.one_of(
            "PM_BILLING",
            BillingMode::Off,
            &[("off", BillingMode::Off), ("stripe", BillingMode::Stripe)],
        )?;
        let plan_catalog = v.json("PM_PLAN_CATALOG")?;
        let grace_days = v.number("PM_BILLING_GRACE_DAYS", 0, 365)?.unwrap_or(7);
        let stripe = match (
            billing_mode,
            v.opt("PM_STRIPE_SECRET_KEY"),
            v.opt("PM_STRIPE_WEBHOOK_SECRET"),
        ) {
            (BillingMode::Stripe, Some(secret_key), Some(webhook_secret)) => Some(StripeSecrets {
                secret_key,
                webhook_secret,
            }),
            (BillingMode::Stripe, _, _) => {
                degraded.push(("billing", "stripe_secrets_missing"));
                None
            }
            (BillingMode::Off, _, _) => None,
        };
        let billing = BillingConfig {
            mode: billing_mode,
            started: billing_mode == BillingMode::Off || stripe.is_some(),
            plan_catalog,
            grace_days,
        };

        // Phone numbers.
        let phone_on = v.on_off("PM_PHONE", false)?;
        let telnyx_api_base = v.or("PM_TELNYX_API_BASE", TELNYX_API_BASES[0]);
        if !TELNYX_API_BASES.contains(&telnyx_api_base.as_str()) {
            return Err(invalid("PM_TELNYX_API_BASE", "not an allowed value"));
        }
        let catalogue = v.json("PM_PHONE_CATALOGUE")?;
        let rate_card = v.json("PM_PHONE_RATE_CARD")?;
        let kyc_retention_days = v
            .number("PM_PHONE_KYC_RETENTION_DAYS", 1, 3_650)?
            .unwrap_or(365);
        let telnyx_api_key = v.opt("PM_TELNYX_API_KEY");
        let phone = if phone_on {
            if telnyx_api_key.is_none() {
                return Err(invalid("PM_TELNYX_API_KEY", "required with PM_PHONE=on"));
            }
            Some(PhoneConfig {
                telnyx_api_base,
                telnyx_account_id: v
                    .opt("PM_TELNYX_ACCOUNT_ID")
                    .ok_or(invalid("PM_TELNYX_ACCOUNT_ID", "required with PM_PHONE=on"))?,
                telnyx_public_key: v.opt("PM_TELNYX_PUBLIC_KEY"),
                catalogue,
                rate_card,
                kyc_retention_days,
            })
        } else {
            None
        };

        let vault = VaultConfig {
            enabled: v.on_off("PM_VAULT", false)?,
            refused_domains: v.list("PM_VAULT_REFUSED_DOMAINS", true),
            clearance_cookies: v.list("PM_VAULT_CLEARANCE_COOKIES", false),
        };

        Ok(Config {
            platform_domain,
            console_host,
            api_host,
            jurisdiction,
            system_from,
            env,
            embed_model: v.or("PM_EMBED_MODEL", "@cf/baai/bge-m3"),
            rerank_model,
            embed_model_previous: v.opt("PM_EMBED_MODEL_PREVIOUS"),
            agent_model: v.or("PM_AGENT_MODEL", "@cf/qwen/qwen3.8-27b"),
            triage_model: v.or("PM_TRIAGE_MODEL", "@cf/openai/gpt-oss-20b"),
            screen_model: v.or("PM_SCREEN_MODEL", "@cf/openai/gpt-oss-20b"),
            ai_gateway: v.opt("PM_AI_GATEWAY"),
            docs_url,
            trusted_authserv_id: v.opt("PM_TRUSTED_AUTHSERV_ID"),
            doh_resolvers,
            ses_region,
            ses_sns_topic_arn,
            scanner_url: v.opt("PM_SCANNER_URL"),
            security_contact,
            log_level,
            default_policy,
            cf_account_id: v.opt("PM_CF_ACCOUNT_ID"),
            daily_send_quota,
            backup_bucket: v.opt("PM_BACKUP_BUCKET"),
            alert_email,
            heartbeat_key_id: v.opt("PM_HEARTBEAT_KEY_ID"),
            break_glass_key_id: v.opt("PM_BREAK_GLASS_KEY_ID"),
            mailbox_provider_domains: v.list("PM_MAILBOX_PROVIDER_DOMAINS", true),
            frozen,
            frozen_since: v.opt("PM_FREEZE_SINCE"),
            eval_rest,
            ses_inbound,
            ses_rule_set: v.or("PM_SES_RULE_SET", "pylota-identity"),
            cf_subdomain_setup,
            web_bot_auth,
            identity_key_overlap_days,
            notifications,
            console,
            billing,
            phone,
            vault,
            secrets: Secrets {
                master_key_a,
                master_key_b,
                master_key_active,
                key_pepper,
                hash_key,
                cf_api_token: v.opt("PM_CF_API_TOKEN"),
                ses,
                stripe,
                oauth_google_secret,
                oauth_github_secret,
                telnyx_api_key,
            },
            degraded,
            warnings,
            itest_fakes_url: if env == DeployEnv::Local {
                v.opt("PM_ITEST_FAKES_URL")
            } else {
                None
            },
            itest_token: if env == DeployEnv::Local {
                v.opt("PM_ITEST_TOKEN")
            } else {
                None
            },
        })
    }

    /// `/health`'s `status`: `degraded` while a feature is off for a missing setting.
    pub fn is_degraded(&self) -> bool {
        !self.degraded.is_empty()
    }

    /// The SES transport is on.
    pub fn ses_transport(&self) -> bool {
        self.secrets.ses.is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const KEY_A: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="; // bytes 0..32
    const KEY_B: &str = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA="; // bytes 1..33

    fn base() -> HashMap<&'static str, String> {
        HashMap::from([
            ("PM_PLATFORM_DOMAIN", "pylotamail.com".to_owned()),
            ("PM_API_HOST", "api.pylotamail.com".to_owned()),
            ("PM_MASTER_KEY", KEY_A.to_owned()),
            ("PM_KEY_PEPPER", KEY_A.to_owned()),
            ("PM_HASH_KEY", KEY_B.to_owned()),
        ])
    }

    fn read_with(
        vars: &HashMap<&'static str, String>,
        build: Build,
    ) -> Result<Config, ConfigInvalid> {
        Config::read(&|name| vars.get(name).cloned(), build)
    }

    fn read(vars: &HashMap<&'static str, String>) -> Result<Config, ConfigInvalid> {
        read_with(vars, Build::RELEASE)
    }

    fn with(pairs: &[(&'static str, &str)]) -> HashMap<&'static str, String> {
        let mut vars = base();
        for (k, v) in pairs {
            vars.insert(k, (*v).to_owned());
        }
        vars
    }

    fn fails(pairs: &[(&'static str, &str)]) -> &'static str {
        match read(&with(pairs)) {
            Err(e) => e.name,
            Ok(_) => panic!("{pairs:?} should be config_invalid"),
        }
    }

    /// Each row of the startup rules in Rust workspace §6.1.
    #[test]
    fn startup_rules() {
        // Defaults.
        let c = read(&base()).unwrap();
        assert_eq!(c.jurisdiction, Jurisdiction::Eu);
        assert_eq!(c.env, DeployEnv::Production);
        assert_eq!(c.console_host, "api.pylotamail.com");
        assert_eq!(c.system_from.address, "no-reply@pylotamail.com");
        assert_eq!(c.system_from.display_name, "Pylota Identity");
        assert_eq!(c.doh_resolvers, DEFAULT_DOH_RESOLVERS.map(str::to_owned));
        assert_eq!(c.docs_url, DEFAULT_DOCS_URL);
        assert!(c.degraded.is_empty() && c.warnings.is_empty() && !c.frozen);
        assert!(c.console.is_some() && c.notifications && c.billing.started);
        assert_eq!(c.secrets.active_master_key()[0], 0);

        // Row 1: a required variable or secret missing, or a secret of the wrong length.
        for name in [
            "PM_PLATFORM_DOMAIN",
            "PM_API_HOST",
            "PM_MASTER_KEY",
            "PM_KEY_PEPPER",
            "PM_HASH_KEY",
        ] {
            let mut vars = base();
            vars.remove(name);
            assert_eq!(read(&vars).unwrap_err().name, name, "missing {name}");
            vars.insert(name, String::new());
            assert_eq!(read(&vars).unwrap_err().name, name, "empty {name}");
        }
        assert_eq!(fails(&[("PM_HASH_KEY", "AAEC")]), "PM_HASH_KEY", "3 bytes");
        assert_eq!(fails(&[("PM_KEY_PEPPER", "not base64!")]), "PM_KEY_PEPPER");

        // Row 2: an optional variable set but malformed names it; never replaced by its default.
        for (name, bad) in [
            ("PM_JURISDICTION", "us"),
            ("PM_ENV", "prod"),
            ("PM_LOG_LEVEL", "trace"),
            ("PM_CONSOLE", "yes"),
            ("PM_NOTIFICATIONS", "true"),
            ("PM_FREEZE", "1"),
            ("PM_SIGNUP", "public"),
            ("PM_BILLING", "paddle"),
            ("PM_DAILY_SEND_QUOTA", "lots"),
            ("PM_IDENTITY_KEY_OVERLAP_DAYS", "-1"),
            ("PM_BILLING_GRACE_DAYS", "7d"),
            ("PM_SYSTEM_FROM", "Pylota <no-reply>"),
            ("PM_SYSTEM_FROM", "no reply@pylotamail.com"),
            ("PM_DOH_RESOLVERS", "https://cloudflare-dns.com/dns-query"),
            ("PM_DOH_RESOLVERS", "https://a.example/q,http://b.example/q"),
            (
                "PM_DOH_RESOLVERS",
                "https://a.example/q,https://b.example/q,https://c.example/q",
            ),
            ("PM_DEFAULT_POLICY", "{not json"),
            ("PM_DEFAULT_POLICY", "[1]"),
            ("PM_PLAN_CATALOG", "{\"plans\": "),
            ("PM_PHONE_CATALOGUE", "nope"),
            ("PM_TELNYX_API_BASE", "https://api.example.com/v2"),
            ("PM_VAULT", "enabled"),
        ] {
            assert_eq!(fails(&[(name, bad)]), name, "{name}={bad}");
        }
        let c = read(&with(&[
            (
                "PM_SYSTEM_FROM",
                "\"Pylota Mail\" <No-Reply@Pylotamail.com>",
            ),
            (
                "PM_DOH_RESOLVERS",
                "https://a.example/dns-query, https://b.example/resolve",
            ),
            ("PM_DEFAULT_POLICY", "{\"send\": {\"daily_cap\": 10}}"),
            ("PM_RERANK_MODEL", "none"),
            ("PM_SECURITY_CONTACT", "mailto:security@pylotamail.com"),
            (
                "PM_MAILBOX_PROVIDER_DOMAINS",
                " Example.org, ,mail.example ",
            ),
        ]))
        .unwrap();
        assert_eq!(c.system_from.display_name, "Pylota Mail");
        assert_eq!(c.system_from.address, "no-reply@pylotamail.com");
        assert_eq!(c.doh_resolvers[1], "https://b.example/resolve");
        assert_eq!(c.default_policy["send"]["daily_cap"], 10);
        assert_eq!(c.rerank_model, None);
        assert_eq!(c.alert_email.as_deref(), Some("security@pylotamail.com"));
        assert_eq!(c.mailbox_provider_domains, ["example.org", "mail.example"]);

        // Row 3: SES credentials and region without PM_SES_SNS_TOPIC_ARN start degraded, SES off.
        let ses = [
            ("PM_SES_REGION", "eu-west-1"),
            ("PM_SES_ACCESS_KEY_ID", "AKIAEXAMPLE"),
            ("PM_SES_SECRET_ACCESS_KEY", "secret"),
        ];
        let c = read(&with(&ses)).unwrap();
        assert!(!c.ses_transport());
        assert_eq!(c.degraded, [("ses", "sns_topic_missing")]);
        assert!(c.is_degraded());
        let mut full = ses.to_vec();
        full.push((
            "PM_SES_SNS_TOPIC_ARN",
            "arn:aws:sns:eu-west-1:123456789012:pm",
        ));
        let c = read(&with(&full)).unwrap();
        assert!(c.ses_transport() && c.degraded.is_empty());
        assert_eq!(
            fails(&[("PM_SES_ACCESS_KEY_ID", "AKIAEXAMPLE")]),
            "PM_SES_SECRET_ACCESS_KEY"
        );
        assert!(
            read(&with(&[("PM_SES_INBOUND_BUCKET", "b")]))
                .unwrap()
                .ses_inbound
                .is_none()
        );

        // Row 4: PM_BILLING=stripe without a Stripe secret starts with billing not started.
        let c = read(&with(&[
            ("PM_BILLING", "stripe"),
            ("PM_STRIPE_SECRET_KEY", "rk_x"),
        ]))
        .unwrap();
        assert!(!c.billing.started && c.secrets.stripe.is_none());
        assert_eq!(c.degraded, [("billing", "stripe_secrets_missing")]);
        let c = read(&with(&[
            ("PM_BILLING", "stripe"),
            ("PM_STRIPE_SECRET_KEY", "rk_x"),
            ("PM_STRIPE_WEBHOOK_SECRET", "whsec_x"),
        ]))
        .unwrap();
        assert!(c.billing.started && c.secrets.stripe.is_some() && c.degraded.is_empty());

        // Row 5: PM_SIGNUP not closed requires the terms, privacy and DPA URLs and the terms version.
        let terms = [
            ("PM_TERMS_URL", "https://pylotamail.com/terms"),
            ("PM_PRIVACY_URL", "https://pylotamail.com/privacy"),
            ("PM_DPA_URL", "https://pylotamail.com/dpa"),
            ("PM_TERMS_VERSION", "2026-10-01"),
        ];
        for missing in 0..terms.len() {
            let mut pairs: Vec<_> = terms.to_vec();
            let (name, _) = pairs.remove(missing);
            pairs.push(("PM_SIGNUP", "waitlist"));
            assert_eq!(fails(&pairs), name);
        }
        let mut pairs = terms.to_vec();
        pairs.push(("PM_SIGNUP", "open"));
        let console = read(&with(&pairs)).unwrap().console.unwrap();
        assert_eq!(console.signup, SignupMode::Open);
        assert!(read(&with(&[("PM_SIGNUP", "closed")])).is_ok());

        // Row 6: PM_WEB_BOT_AUTH=on in a release without signed requests.
        let vars = with(&[("PM_WEB_BOT_AUTH", "on")]);
        assert!(read(&vars).unwrap().web_bot_auth);
        let unsigned = Build {
            signed_requests: false,
        };
        assert_eq!(
            read_with(&vars, unsigned).unwrap_err().name,
            "PM_WEB_BOT_AUTH"
        );
        assert!(!read_with(&base(), unsigned).unwrap().web_bot_auth);

        // Row 7: the master-key slots.
        assert_eq!(
            fails(&[("PM_MASTER_KEY_ACTIVE", "b")]),
            "PM_MASTER_KEY_B",
            "empty slot b"
        );
        assert_eq!(
            fails(&[("PM_MASTER_KEY_ACTIVE", "c")]),
            "PM_MASTER_KEY_ACTIVE"
        );
        assert_eq!(
            fails(&[("PM_MASTER_KEY_B", KEY_A)]),
            "PM_MASTER_KEY_B",
            "same key ID"
        );
        assert_eq!(
            fails(&[("PM_MASTER_KEY_B", "AAEC")]),
            "PM_MASTER_KEY_B",
            "wrong length"
        );
        let c = read(&with(&[
            ("PM_MASTER_KEY_B", KEY_B),
            ("PM_MASTER_KEY_ACTIVE", "b"),
        ]))
        .unwrap();
        assert_eq!(c.secrets.master_key_active, MasterSlot::B);
        assert_eq!(c.secrets.active_master_key()[0], 1);

        // Row 8: PM_AGENT_SIGNUP not on or off, PM_AGENT_SIGNUP_DAILY_CAP outside 1–10,000.
        assert_eq!(fails(&[("PM_AGENT_SIGNUP", "yes")]), "PM_AGENT_SIGNUP");
        for cap in ["0", "10001", "x"] {
            assert_eq!(
                fails(&[("PM_AGENT_SIGNUP_DAILY_CAP", cap)]),
                "PM_AGENT_SIGNUP_DAILY_CAP"
            );
        }
        let c = read(&with(&[
            ("PM_AGENT_SIGNUP", "on"),
            ("PM_AGENT_SIGNUP_DAILY_CAP", "10000"),
        ]))
        .unwrap()
        .console
        .unwrap();
        assert!(c.agent_signup && c.agent_signup_daily_cap == 10_000);
        assert_eq!(
            read(&base())
                .unwrap()
                .console
                .unwrap()
                .agent_signup_daily_cap,
            200
        );

        // Row 9: an invalid PM_DOCS_URL starts with the default and logs docs_url_invalid.
        for bad in [
            "http://docs.example.com",
            "https://docs.example.com/?v=1",
            "https://docs.example.com/#top",
            &format!("https://docs.example.com/{}", "a".repeat(200)),
        ] {
            let c = read(&with(&[("PM_DOCS_URL", bad)])).unwrap();
            assert_eq!(c.docs_url, DEFAULT_DOCS_URL, "{bad}");
            assert_eq!(c.warnings, ["docs_url_invalid"]);
        }
        let c = read(&with(&[(
            "PM_DOCS_URL",
            "https://docs.example.com/identity/",
        )]))
        .unwrap();
        assert_eq!(c.docs_url, "https://docs.example.com/identity");
        assert!(c.warnings.is_empty());

        // Row 10: PM_FREEZE=on starts frozen.
        let c = read(&with(&[
            ("PM_FREEZE", "on"),
            ("PM_FREEZE_SINCE", "2026-10-28T10:00:00Z"),
        ]))
        .unwrap();
        assert!(c.frozen);
        assert_eq!(c.frozen_since.as_deref(), Some("2026-10-28T10:00:00Z"));

        // PM_CONSOLE_HOST, PM_SYSTEM_FROM and PM_NOTIFICATIONS are read with PM_CONSOLE=off; the console's
        // own variables are still validated.
        let c = read(&with(&[
            ("PM_CONSOLE", "off"),
            ("PM_CONSOLE_HOST", "console.pylotamail.com"),
            ("PM_SYSTEM_FROM", "Pylota <hello@pylotamail.com>"),
            ("PM_NOTIFICATIONS", "off"),
            ("PM_SIGNUP", "open"),
        ]))
        .unwrap();
        assert!(c.console.is_none());
        assert_eq!(c.console_host, "console.pylotamail.com");
        assert_eq!(c.system_from.address, "hello@pylotamail.com");
        assert!(!c.notifications);
        assert_eq!(
            fails(&[("PM_CONSOLE", "off"), ("PM_SIGNUP", "public")]),
            "PM_SIGNUP"
        );

        // PM_EVAL_REST is honoured only with PM_ENV=local.
        assert!(!read(&with(&[("PM_EVAL_REST", "on")])).unwrap().eval_rest);
        assert!(
            read(&with(&[("PM_EVAL_REST", "on"), ("PM_ENV", "local")]))
                .unwrap()
                .eval_rest
        );

        // Phone numbers need the Telnyx key and account with PM_PHONE=on.
        assert_eq!(fails(&[("PM_PHONE", "on")]), "PM_TELNYX_API_KEY");
        assert_eq!(
            fails(&[("PM_PHONE", "on"), ("PM_TELNYX_API_KEY", "k")]),
            "PM_TELNYX_ACCOUNT_ID"
        );
        let p = read(&with(&[
            ("PM_PHONE", "on"),
            ("PM_TELNYX_API_KEY", "k"),
            ("PM_TELNYX_ACCOUNT_ID", "org"),
            ("PM_TELNYX_API_BASE", "https://api.telnyx.eu/v2"),
        ]))
        .unwrap()
        .phone
        .unwrap();
        assert_eq!(p.telnyx_api_base, "https://api.telnyx.eu/v2");
        assert_eq!(p.kyc_retention_days, 365);
    }

    /// No secret value reaches `Debug` output or an error.
    #[test]
    fn secrets_never_print() {
        let c = read(&with(&[("PM_CF_API_TOKEN", "cf-token-value")])).unwrap();
        let text = format!("{:?}", c.secrets);
        assert!(text.contains("cf_api_token: true"));
        assert!(!text.contains("cf-token-value") && !text.contains("[0, 1, 2"));
        let e = read(&with(&[("PM_HASH_KEY", "c2VjcmV0")])).unwrap_err();
        assert!(!e.to_string().contains("c2VjcmV0"));
    }
}
