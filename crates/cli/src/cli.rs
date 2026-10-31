//! The command tree ([CLI reference]): global flags and one subcommand per command of the M16 surface.
//! Later milestones add theirs (drafts, events, uploads, signing keys, `--schema`, `--query`, …).
//!
//! [CLI reference]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/reference/cli.md

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "idvy",
    version,
    about = "Pylota Identity: email for AI agents",
    max_term_width = 100
)]
pub struct Cli {
    #[command(flatten)]
    pub global: Global,
    #[command(subcommand)]
    pub command: Command,
}

/// Flags every command accepts.
#[derive(Args, Clone, Debug, Default)]
pub struct Global {
    /// API host (`https://…`).
    #[arg(long, global = true)]
    pub url: Option<String>,
    /// API key (visible in the process list; prefer PYLOTA_IDENTITY_KEY or a profile).
    #[arg(long, global = true)]
    pub key: Option<String>,
    /// Profile in the config file.
    #[arg(long, global = true)]
    pub profile: Option<String>,
    /// Cloudflare account ID.
    #[arg(long = "account-id", global = true)]
    pub account_id: Option<String>,
    /// Identity: an `idn_…` ID or one of its addresses.
    #[arg(long, global = true)]
    pub identity: Option<String>,
    /// Tenant: a `ten_…` ID or a slug.
    #[arg(long, global = true)]
    pub tenant: Option<String>,
    /// Print one JSON document.
    #[arg(long, global = true)]
    pub json: bool,
    /// Print only the essential value.
    #[arg(long, global = true, conflicts_with = "json")]
    pub quiet: bool,
    /// Answer yes to confirmations.
    #[arg(long, global = true)]
    pub yes: bool,
    /// Log each request's method, path, status and duration to stderr.
    #[arg(long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create every Cloudflare resource and deploy the Worker.
    Setup(SetupArgs),
    /// Deploy a release (or the CLI's own version).
    Deploy(DeployArgs),
    /// Deploy the CLI's version gradually, then run doctor.
    Upgrade(UpgradeArgs),
    /// Check the deployment.
    Doctor(DoctorArgs),
    /// Run the pinned Wrangler with these arguments.
    Wrangler(WranglerArgs),
    /// Save the API URL and key in a profile.
    Login(LoginArgs),
    #[command(subcommand)]
    Config(ConfigCmd),
    #[command(subcommand)]
    Mcp(McpCmd),
    #[command(subcommand)]
    Tenants(TenantsCmd),
    #[command(subcommand)]
    Partners(PartnersCmd),
    #[command(subcommand)]
    Identities(IdentitiesCmd),
    #[command(subcommand)]
    Addresses(AddressesCmd),
    #[command(subcommand)]
    Domains(DomainsCmd),
    #[command(subcommand, name = "identity-keys")]
    IdentityKeys(IdentityKeysCmd),
    #[command(subcommand)]
    Assertions(AssertionsCmd),
    /// Sign an HTTP request (Web Bot Auth) as an identity.
    #[command(name = "http-sign")]
    HttpSign(HttpSignArgs),
    /// Send a new email.
    Send(SendArgs),
    /// Reply to the sender of a message.
    Reply(ReplyArgs),
    /// Reply to the sender and every other recipient.
    #[command(name = "reply-all")]
    ReplyAll(ReplyArgs),
    /// Forward a message.
    Forward(ForwardArgs),
    /// Cancel a queued send.
    Cancel(MessageArg),
    /// Record the outcome of an uncertain send.
    Resolve(ResolveArgs),
    #[command(subcommand)]
    Threads(ThreadsCmd),
    #[command(subcommand)]
    Messages(MessagesCmd),
    /// Search mail.
    Search(SearchArgs),
    /// Ask a question; the answer cites messages.
    Ask(AskArgs),
    /// Wait for a new matching message.
    Wait(WaitArgs),
    #[command(subcommand)]
    Triage(TriageCmd),
    #[command(subcommand)]
    Quarantine(QuarantineCmd),
    #[command(subcommand)]
    Webhooks(WebhooksCmd),
    #[command(subcommand)]
    Keys(KeysCmd),
    #[command(subcommand)]
    Secrets(SecretsCmd),
    #[command(subcommand)]
    Dlq(DlqCmd),
    #[command(subcommand)]
    Ops(OpsCmd),
    #[command(subcommand)]
    Jobs(JobsCmd),
    #[command(subcommand)]
    Suppressions(SuppressionsCmd),
    #[command(subcommand)]
    Lists(ListsCmd),
    #[command(subcommand)]
    Erasure(ErasureCmd),
    #[command(subcommand)]
    Export(ExportCmd),
    /// Read the audit log.
    Audit(AuditArgs),
}

/// `--limit`, `--cursor` and `--all` of a list command.
#[derive(Args, Clone, Debug, Default)]
pub struct Paging {
    #[arg(long)]
    pub limit: Option<u32>,
    #[arg(long)]
    pub cursor: Option<String>,
    /// Follow every page into one document.
    #[arg(long)]
    pub all: bool,
}

#[derive(Args, Debug)]
pub struct DirArg {
    /// Deployment directory.
    #[arg(long, default_value = "deploy")]
    pub dir: String,
}

// ---- Deployment -------------------------------------------------------------------------------

#[derive(Args, Debug)]
pub struct SetupArgs {
    /// The API host (`PM_API_HOST`).
    #[arg(long)]
    pub domain: String,
    /// The platform mail domain, a zone apex (`PM_PLATFORM_DOMAIN`).
    #[arg(long = "mail-domain")]
    pub mail_domain: Option<String>,
    #[arg(long, default_value = "eu", value_parser = ["eu", "default"])]
    pub jurisdiction: String,
    #[arg(long = "console-host")]
    pub console_host: Option<String>,
    #[arg(long = "owner-email")]
    pub owner_email: Option<String>,
    #[arg(long = "owner-name")]
    pub owner_name: Option<String>,
    #[arg(long = "tenant-name", default_value = "Default")]
    pub tenant_name: String,
    #[arg(long = "no-console")]
    pub no_console: bool,
    /// Delete existing MX records at the mail domain; mail to its current provider stops.
    #[arg(long = "replace-mx")]
    pub replace_mx: bool,
    #[arg(long = "daily-send-quota")]
    pub daily_send_quota: Option<u64>,
    #[arg(long = "backup-bucket")]
    pub backup_bucket: Option<String>,
    /// Worker release to deploy.
    #[arg(long)]
    pub version: Option<String>,
    #[arg(long = "from-source")]
    pub from_source: bool,
    #[arg(long = "source-dir", default_value = ".")]
    pub source_dir: String,
    #[arg(long = "print-secrets")]
    pub print_secrets: bool,
    /// Break-glass: replace PM_KEY_PEPPER; every existing API key stops working.
    #[arg(long = "rotate-pepper")]
    pub rotate_pepper: bool,
    /// The deployment name.
    #[arg(long, default_value = "pylota-identity")]
    pub name: String,
    #[command(flatten)]
    pub dir: DirArg,
}

#[derive(Args, Debug)]
pub struct DeployArgs {
    /// The release to deploy (default: the CLI's version).
    #[arg(long)]
    pub version: Option<String>,
    #[arg(long = "from-source")]
    pub from_source: bool,
    #[arg(long = "source-dir", default_value = ".")]
    pub source_dir: String,
    /// Deploy anyway when nothing changed.
    #[arg(long)]
    pub force: bool,
    #[command(flatten)]
    pub dir: DirArg,
}

#[derive(Args, Debug)]
pub struct UpgradeArgs {
    #[command(flatten)]
    pub dir: DirArg,
}

#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Send a test message to the platform domain and wait for it.
    #[arg(long = "mail-test")]
    pub mail_test: bool,
    /// Run only these checks.
    #[arg(long = "check")]
    pub checks: Vec<String>,
    #[command(flatten)]
    pub dir: DirArg,
}

#[derive(Args, Debug)]
pub struct WranglerArgs {
    #[command(flatten)]
    pub dir: DirArg,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

// ---- Dead letters and jobs --------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum DlqCmd {
    /// List dead-letter items (never their bodies).
    List {
        #[arg(long)]
        queue: Option<String>,
        #[arg(long, value_parser = ["open", "redriven"])]
        status: Option<String>,
        #[command(flatten)]
        paging: Paging,
    },
    /// Publish items back to their source queue.
    Redrive {
        ids: Vec<String>,
        /// Every open item of this queue (with --tenant, of that tenant only).
        #[arg(long)]
        queue: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum JobsCmd {
    /// Start a maintenance job over one tenant (--tenant is required).
    Start {
        #[arg(value_parser = ["reparse", "reembed", "reindex", "event_log_rebuild"])]
        kind: String,
        /// Only these identities (comma-separated IDs or addresses).
        #[arg(long = "identity-ids", value_delimiter = ',')]
        identities: Vec<String>,
        #[arg(long)]
        after: Option<String>,
        #[arg(long)]
        before: Option<String>,
    },
    Get {
        job: String,
    },
}

// ---- Operations -------------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum OpsCmd {
    /// The freeze flag, switches, firing alerts, master key and capacity.
    Status,
    /// Turn a deployment-wide switch on or off.
    Switch {
        #[arg(value_parser = ["read_only", "free_sending", "emergency_prune", "agent_signup"])]
        name: String,
        #[arg(value_parser = ["on", "off"])]
        state: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Freeze the deployment for a restore (PM_FREEZE = "on", then a code deploy).
    Freeze {
        #[command(flatten)]
        dir: DirArg,
    },
    /// End a restore freeze.
    Unfreeze {
        #[command(flatten)]
        dir: DirArg,
    },
    #[command(subcommand)]
    Restore(RestoreCmd),
}

#[derive(Subcommand, Debug)]
pub enum RestoreCmd {
    /// Restore D1 to a point in time and replay what came after it.
    D1 {
        #[arg(long)]
        at: Option<String>,
        /// A table or table.column the restore is meant to undo (repeatable).
        #[arg(long)]
        exclude: Vec<String>,
        /// A row the restore is meant to undo, as table:key (repeatable).
        #[arg(long = "exclude-row")]
        exclude_row: Vec<String>,
        /// Recompute and apply the replay from an earlier restore's working directory.
        #[arg(long = "replay-only")]
        replay_only: Option<String>,
        #[command(flatten)]
        dir: DirArg,
    },
    /// Restore one identity's mailbox (only while frozen).
    Mailbox {
        identity: String,
        #[arg(long)]
        at: Option<String>,
        #[arg(long)]
        bookmark: Option<String>,
    },
    /// Re-queue and re-insert what came after the restore point, and re-apply erasures.
    Reconcile {
        #[arg(long = "identity-ids", value_delimiter = ',')]
        identities: Vec<String>,
        #[arg(long)]
        after: String,
    },
    /// Delete the restore working directories (they hold every D1 row).
    Cleanup {
        #[command(flatten)]
        dir: DirArg,
    },
}

// ---- Secrets ----------------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum SecretsCmd {
    /// Rotate PM_MASTER_KEY into the other slot and wait for the re-seal.
    #[command(name = "rotate-master")]
    RotateMaster(RotateMasterArgs),
}

#[derive(Args, Debug)]
pub struct RotateMasterArgs {
    /// Poll an unfinished rotation instead of starting one.
    #[arg(long)]
    pub resume: bool,
    /// Overwrite the previous key within 30 days of its re-seal (after a suspected leak).
    #[arg(long = "discard-previous")]
    pub discard_previous: bool,
    #[command(flatten)]
    pub dir: DirArg,
}

// ---- Profiles ---------------------------------------------------------------------------------

#[derive(Args, Debug)]
pub struct LoginArgs {
    /// Store this environment variable's name instead of the key.
    #[arg(long = "key-env", conflicts_with = "key_command")]
    pub key_env: Option<String>,
    /// Store this command, whose output is the key, instead of the key.
    #[arg(long = "key-command")]
    pub key_command: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// Print the resolved settings and where each came from.
    Show,
    /// Set url, identity, tenant, account_id, key_env, key_command or default_profile.
    Set { setting: String, value: String },
}

#[derive(Subcommand, Debug)]
pub enum McpCmd {
    /// Print MCP client configuration for the current profile's URL.
    Config {
        #[arg(long, default_value = "generic", value_parser = ["generic"])]
        client: String,
        #[arg(long, default_value = "pylota-identity")]
        name: String,
    },
}

// ---- Tenants and partners -----------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum TenantsCmd {
    Create {
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,
        #[arg(long, value_parser = ["live", "test"])]
        mode: Option<String>,
        #[arg(long)]
        timezone: Option<String>,
        #[arg(long = "address-suffix")]
        address_suffix: Option<String>,
        #[arg(long = "policy-file")]
        policy_file: Option<String>,
        #[arg(long = "owner-email")]
        owner_email: Option<String>,
        #[arg(long = "owner-name")]
        owner_name: Option<String>,
        #[arg(long = "billing-mode", value_parser = ["metered", "exempt", "disabled"])]
        billing_mode: Option<String>,
    },
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        partner: Option<String>,
        #[command(flatten)]
        paging: Paging,
    },
    Get {
        tenant: String,
    },
    Update {
        tenant: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        timezone: Option<String>,
        #[arg(long = "policy-file", conflicts_with = "policy")]
        policy_file: Option<String>,
        #[arg(long)]
        policy: Option<String>,
    },
    /// Show the tenant's policy with what your key may change, or change it.
    Policy {
        tenant: String,
        #[arg(long, conflicts_with = "set_file")]
        set: Option<String>,
        #[arg(long = "set-file")]
        set_file: Option<String>,
    },
    Suspend {
        tenant: String,
    },
    Resume {
        tenant: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum PartnersCmd {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long = "default-billing-mode", value_parser = ["exempt", "metered"])]
        default_billing_mode: Option<String>,
        #[arg(long = "max-tenants")]
        max_tenants: Option<u32>,
        #[arg(long = "ramp-exempt")]
        ramp_exempt: Option<bool>,
    },
    List {
        #[arg(long)]
        status: Option<String>,
        #[command(flatten)]
        paging: Paging,
    },
    Get {
        partner: String,
    },
    Update {
        partner: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_parser = ["active", "suspended"])]
        status: Option<String>,
        #[arg(long = "default-billing-mode", value_parser = ["exempt", "metered"])]
        default_billing_mode: Option<String>,
        #[arg(long = "max-tenants")]
        max_tenants: Option<u32>,
        #[arg(long = "ramp-exempt")]
        ramp_exempt: Option<bool>,
    },
    Delete {
        partner: String,
    },
}

// ---- Identities, addresses and domains ------------------------------------------------------------

#[derive(Args, Debug, Default)]
pub struct IdentityFields {
    #[arg(long = "display-name")]
    pub display_name: Option<String>,
    #[arg(long)]
    pub purpose: Option<String>,
    #[arg(long = "owner-name")]
    pub owner_name: Option<String>,
    #[arg(long = "owner-email")]
    pub owner_email: Option<String>,
    #[arg(long = "signature-text")]
    pub signature_text: Option<String>,
    /// `key=value`; repeatable.
    #[arg(long)]
    pub metadata: Vec<String>,
    /// Erase each message this many hours after it is stored (1–8760).
    #[arg(long = "message-hours", conflicts_with = "keep_messages")]
    pub message_hours: Option<u32>,
    #[arg(long = "keep-messages")]
    pub keep_messages: bool,
}

#[derive(Subcommand, Debug)]
pub enum IdentitiesCmd {
    Create {
        #[arg(long)]
        username: String,
        #[command(flatten)]
        fields: IdentityFields,
        /// Create the primary address on this tenant domain.
        #[arg(long)]
        domain: Option<String>,
        #[arg(long = "client-id")]
        client_id: Option<String>,
    },
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        purpose: Option<String>,
        #[arg(long = "client-id")]
        client_id: Option<String>,
        #[command(flatten)]
        paging: Paging,
    },
    Get {
        identity: String,
    },
    Update {
        identity: String,
        #[command(flatten)]
        fields: IdentityFields,
        #[arg(long = "daily-cap")]
        daily_cap: Option<u32>,
        #[arg(long = "auto-reply", value_parser = ["allowed", "denied"])]
        auto_reply: Option<String>,
        #[arg(long = "require-known-recipient")]
        require_known_recipient: Option<bool>,
    },
    Pause {
        identity: String,
    },
    Resume {
        identity: String,
    },
    /// Erase the whole mailbox.
    Delete {
        identity: String,
        /// The identity's primary address, typed to confirm.
        #[arg(long)]
        confirm: Option<String>,
    },
    Lookup {
        address: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum IdentityKeysCmd {
    List {
        #[arg(long, value_parser = ["active", "retiring", "retired"])]
        status: Option<String>,
        #[command(flatten)]
        paging: Paging,
    },
    Create,
    Rotate,
    Revoke {
        kid: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum AssertionsCmd {
    /// Mint an agent assertion.
    Create {
        #[arg(long)]
        audience: String,
        #[arg(long = "expires-in", default_value_t = 300)]
        expires_in: i64,
        #[arg(long)]
        nonce: Option<String>,
        /// A JSON object placed under the `ext` claim.
        #[arg(long, conflicts_with = "ext_file")]
        ext: Option<String>,
        #[arg(long = "ext-file")]
        ext_file: Option<String>,
    },
    /// Check an assertion offline, as a verifier does (`-` reads the token from stdin).
    Verify {
        token: String,
        #[arg(long)]
        audience: String,
        #[arg(long)]
        issuer: Option<String>,
        #[arg(long)]
        now: Option<i64>,
    },
}

#[derive(Args, Debug)]
pub struct HttpSignArgs {
    #[arg(long)]
    pub url: String,
    #[arg(long)]
    pub method: Option<String>,
    #[arg(long = "expires-in", default_value_t = 60)]
    pub expires_in: i64,
    /// Also sign `@method`, `@path` or `@query` (repeatable).
    #[arg(long = "component")]
    pub components: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum AddressesCmd {
    List,
    Add {
        #[arg(long = "local-part")]
        local_part: String,
        #[arg(long)]
        domain: String,
    },
    Promote {
        address: String,
        #[arg(long = "retire-previous-after-days")]
        retire_previous_after_days: Option<u32>,
    },
    Retire {
        address: String,
        #[arg(long = "after-days")]
        after_days: Option<u32>,
    },
    Delete {
        address: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum DomainsCmd {
    Add {
        name: String,
        #[arg(long, value_parser = ["cloudflare_zone", "nameservers", "dns_records", "send_only", "smtp_relay", "delegated_subdomain"])]
        method: String,
        #[arg(long = "no-receiving")]
        no_receiving: bool,
        #[arg(long = "no-sending")]
        no_sending: bool,
        #[arg(long = "replace-mx")]
        replace_mx: bool,
        #[arg(long = "confirm-dedicated")]
        confirm_dedicated: bool,
        #[arg(long)]
        claim: bool,
        #[arg(long, value_parser = ["forward", "ses"])]
        inbound: Option<String>,
        #[arg(long = "smtp-host")]
        smtp_host: Option<String>,
        #[arg(long = "smtp-port")]
        smtp_port: Option<u16>,
        #[arg(long = "smtp-username")]
        smtp_username: Option<String>,
        #[arg(long = "smtp-password-stdin")]
        smtp_password_stdin: bool,
        #[arg(long = "probe-from")]
        probe_from: Option<String>,
    },
    List {
        #[command(flatten)]
        paging: Paging,
    },
    Get {
        domain: String,
    },
    Records {
        domain: String,
        #[arg(long, default_value = "json", value_parser = ["json", "bind"])]
        format: String,
    },
    Verify {
        domain: String,
    },
    Health {
        domain: String,
    },
    Reprove {
        domain: String,
    },
    Probe {
        domain: String,
    },
    /// Create a `manual` domain's Email Sending event subscription with your local token.
    Subscribe {
        domain: String,
        #[command(flatten)]
        dir: DirArg,
    },
    Remove {
        domain: String,
    },
}

// ---- Sending ---------------------------------------------------------------------------------------

/// The body flags of a send, reply or forward.
#[derive(Args, Debug, Default)]
pub struct BodyArgs {
    #[arg(long, conflicts_with = "text_file")]
    pub text: Option<String>,
    #[arg(long = "text-file")]
    pub text_file: Option<String>,
    #[arg(long, conflicts_with = "html_file")]
    pub html: Option<String>,
    #[arg(long = "html-file")]
    pub html_file: Option<String>,
    /// A file to attach; repeatable.
    #[arg(long)]
    pub attach: Vec<String>,
    #[arg(long = "idempotency-key")]
    pub idempotency_key: Option<String>,
}

#[derive(Args, Debug)]
pub struct SendArgs {
    /// `addr` or `Name <addr>`; repeat or separate with commas.
    #[arg(long, required = true)]
    pub to: Vec<String>,
    #[arg(long)]
    pub cc: Vec<String>,
    #[arg(long)]
    pub bcc: Vec<String>,
    #[arg(long)]
    pub subject: String,
    #[command(flatten)]
    pub body: BodyArgs,
    #[arg(long, value_parser = ["transactional", "marketing", "auto_reply"])]
    pub kind: Option<String>,
    #[arg(long)]
    pub thread: Option<String>,
    #[arg(long = "from-address")]
    pub from_address: Option<String>,
    #[arg(long)]
    pub label: Vec<String>,
    /// `X-Name: value`; repeatable.
    #[arg(long)]
    pub header: Vec<String>,
    #[arg(long)]
    pub metadata: Vec<String>,
    #[arg(long = "unsubscribe-url")]
    pub unsubscribe_url: Option<String>,
    #[arg(long = "unsubscribe-mailto")]
    pub unsubscribe_mailto: Option<String>,
    #[arg(long = "consent-basis", requires = "consent_recorded_at")]
    pub consent_basis: Option<String>,
    #[arg(long = "consent-recorded-at")]
    pub consent_recorded_at: Option<String>,
    /// Check without sending.
    #[arg(long = "dry-run")]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct ReplyArgs {
    pub message: String,
    #[command(flatten)]
    pub body: BodyArgs,
    #[arg(long, value_parser = ["transactional", "auto_reply"])]
    pub kind: Option<String>,
    #[arg(long = "dry-run")]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct ForwardArgs {
    pub message: String,
    #[arg(long, required = true)]
    pub to: Vec<String>,
    #[arg(long)]
    pub cc: Vec<String>,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long = "no-attachments")]
    pub no_attachments: bool,
    #[arg(long = "idempotency-key")]
    pub idempotency_key: Option<String>,
    #[arg(long = "dry-run")]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct MessageArg {
    pub message: String,
}

#[derive(Args, Debug)]
pub struct ResolveArgs {
    pub message: String,
    #[arg(long, value_parser = ["sent", "not_sent"])]
    pub outcome: String,
}

// ---- Threads, messages, search ------------------------------------------------------------------

#[derive(Args, Debug, Default)]
pub struct LabelArgs {
    #[arg(long)]
    pub add: Vec<String>,
    #[arg(long)]
    pub remove: Vec<String>,
    #[arg(long, conflicts_with = "unread")]
    pub read: bool,
    #[arg(long)]
    pub unread: bool,
    #[arg(long, conflicts_with = "unarchive")]
    pub archive: bool,
    #[arg(long)]
    pub unarchive: bool,
}

#[derive(Subcommand, Debug)]
pub enum ThreadsCmd {
    List {
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long = "needs-reply-gte")]
        needs_reply_gte: Option<f64>,
        #[arg(long)]
        unread: bool,
        #[arg(long)]
        direction: Option<String>,
        #[arg(long)]
        after: Option<String>,
        #[arg(long)]
        before: Option<String>,
        #[arg(long)]
        archived: bool,
        #[command(flatten)]
        paging: Paging,
    },
    Get {
        thread: String,
        #[arg(long = "messages-limit")]
        messages_limit: Option<u32>,
        #[arg(long)]
        include: Option<String>,
    },
    Label {
        thread: String,
        #[command(flatten)]
        labels: LabelArgs,
    },
    /// Put a legal hold on a thread.
    Hold {
        thread: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        until: Option<String>,
    },
    Unhold {
        thread: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum MessagesCmd {
    List {
        #[arg(long)]
        thread: Option<String>,
        #[arg(long)]
        direction: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        after: Option<String>,
        #[arg(long)]
        before: Option<String>,
        #[command(flatten)]
        paging: Paging,
    },
    Get {
        message: String,
        #[arg(long)]
        include: Option<String>,
    },
    /// Save the original MIME message.
    Raw {
        message: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Save an attachment's bytes.
    Attachment {
        message: String,
        attachment: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Print an attachment's extracted text.
    #[command(name = "attachment-text")]
    AttachmentText {
        message: String,
        attachment: String,
        #[arg(long)]
        pages: Option<String>,
    },
    Label {
        message: String,
        #[command(flatten)]
        labels: LabelArgs,
    },
    /// Erase one message.
    Delete { message: String },
    /// Messages related to one message.
    Related {
        message: String,
        #[arg(long)]
        limit: Option<u32>,
    },
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    pub query: String,
    #[arg(long, value_parser = ["keyword", "semantic", "hybrid", "agentic"])]
    pub mode: Option<String>,
    #[arg(long = "identity-ids")]
    pub identity_ids: Option<String>,
    #[arg(long = "group-by", value_parser = ["message", "thread"])]
    pub group_by: Option<String>,
    #[arg(long)]
    pub limit: Option<u32>,
    #[arg(long = "snippet-chars")]
    pub snippet_chars: Option<u32>,
    #[arg(long)]
    pub direction: Option<String>,
    #[arg(long)]
    pub label: Vec<String>,
    #[arg(long)]
    pub after: Option<String>,
    #[arg(long)]
    pub before: Option<String>,
    #[arg(long = "no-facets")]
    pub no_facets: bool,
    #[arg(long = "include-quarantined")]
    pub include_quarantined: bool,
    #[arg(long = "require-mode")]
    pub require_mode: bool,
    #[arg(long)]
    pub cursor: Option<String>,
}

#[derive(Args, Debug)]
pub struct AskArgs {
    pub question: String,
    #[arg(long = "max-steps")]
    pub max_steps: Option<u32>,
    #[arg(long = "max-seconds")]
    pub max_seconds: Option<u32>,
    #[arg(long = "include-quarantined")]
    pub include_quarantined: bool,
    #[arg(long = "no-stream")]
    pub no_stream: bool,
    #[arg(long = "show-trace")]
    pub show_trace: bool,
    /// With --json: one JSON line per event.
    #[arg(long)]
    pub stream: bool,
}

#[derive(Args, Debug)]
pub struct WaitArgs {
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long = "subject-contains")]
    pub subject_contains: Option<String>,
    #[arg(long)]
    pub thread: Option<String>,
    #[arg(long, value_parser = ["any", "reply", "verification"])]
    pub kind: Option<String>,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub timeout: Option<u32>,
}

#[derive(Subcommand, Debug)]
pub enum TriageCmd {
    List {
        #[arg(long)]
        category: Option<String>,
        #[arg(long = "needs-reply-gte")]
        needs_reply_gte: Option<f64>,
        #[arg(long)]
        limit: Option<u32>,
    },
    Rerun {
        message: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum QuarantineCmd {
    List {
        #[command(flatten)]
        paging: Paging,
    },
    Release {
        message: String,
        #[arg(long)]
        reason: String,
    },
}

// ---- Webhooks and keys ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum WebhooksCmd {
    Create {
        #[arg(long)]
        url: String,
        #[arg(long)]
        events: String,
        #[arg(long = "identity-ids")]
        identity_ids: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long, conflicts_with = "partner")]
        platform: bool,
        #[arg(long)]
        partner: bool,
    },
    List {
        #[command(flatten)]
        paging: Paging,
    },
    Get {
        webhook: String,
    },
    Update {
        webhook: String,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        events: Option<String>,
        #[arg(long = "identity-ids")]
        identity_ids: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        #[arg(long)]
        disable: bool,
    },
    Delete {
        webhook: String,
    },
    /// Issue a new signing secret.
    Rotate {
        webhook: String,
        #[arg(long = "overlap-hours")]
        overlap_hours: Option<u32>,
    },
    Test {
        webhook: String,
    },
    Deliveries {
        webhook: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long = "event-type")]
        event_type: Option<String>,
        #[arg(long)]
        after: Option<String>,
        #[command(flatten)]
        paging: Paging,
    },
    Replay {
        webhook: String,
        #[arg(long = "event-id")]
        event_ids: Vec<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        until: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    /// Check a captured delivery's signature offline.
    Verify {
        #[arg(long = "secret-env", conflicts_with = "secret")]
        secret_env: Option<String>,
        #[arg(long)]
        secret: Option<String>,
        #[arg(long, conflicts_with_all = ["id", "timestamp", "signature"])]
        headers: Option<String>,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        timestamp: Option<String>,
        #[arg(long)]
        signature: Option<String>,
        #[arg(long)]
        body: Option<String>,
        #[arg(long, default_value_t = 300)]
        tolerance: i64,
        #[arg(long)]
        now: Option<i64>,
    },
}

#[derive(Subcommand, Debug)]
pub enum KeysCmd {
    Create {
        #[arg(long, value_parser = ["platform", "partner", "tenant", "identity"])]
        level: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        partner: Option<String>,
        #[arg(long)]
        permissions: Option<String>,
        #[arg(long = "expires-at", conflicts_with = "expires_in")]
        expires_at: Option<String>,
        /// For example `90d`, `12h`.
        #[arg(long = "expires-in")]
        expires_in: Option<String>,
        #[arg(long = "mcp-tools")]
        mcp_tools: Option<String>,
        #[arg(long = "save-profile")]
        save_profile: Option<String>,
    },
    List {
        #[command(flatten)]
        paging: Paging,
    },
    Get {
        key: String,
    },
    Revoke {
        key: String,
    },
    /// Rotate an API key (`key_…`), or a signing key (`thread`, `link`, `cursor`, `web_bot_auth`).
    Rotate {
        key: String,
        #[arg(long = "overlap-hours")]
        overlap_hours: Option<u32>,
        #[arg(long = "revoke-previous")]
        revoke_previous: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum SuppressionsCmd {
    List {
        #[arg(long)]
        address: Option<String>,
        #[arg(long)]
        reason: Option<String>,
        #[command(flatten)]
        paging: Paging,
    },
    Add {
        address: String,
        #[arg(long, default_value = "manual")]
        reason: String,
        #[arg(long)]
        note: Option<String>,
    },
    Remove {
        address: String,
        #[arg(long = "confirm-complaint-removal")]
        confirm_complaint_removal: bool,
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ListsCmd {
    List {
        #[arg(value_parser = ["receive", "send"])]
        direction: String,
        #[arg(value_parser = ["allow", "block"])]
        kind: String,
        #[command(flatten)]
        paging: Paging,
    },
    Add {
        #[arg(value_parser = ["receive", "send"])]
        direction: String,
        #[arg(value_parser = ["allow", "block"])]
        kind: String,
        entry: String,
    },
    Remove {
        #[arg(value_parser = ["receive", "send"])]
        direction: String,
        #[arg(value_parser = ["allow", "block"])]
        kind: String,
        entry: String,
    },
}

// ---- Privacy and operations ----------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum ErasureCmd {
    Create {
        #[arg(long, value_parser = ["message", "thread", "counterparty", "identity", "tenant"])]
        scope: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        message: Option<String>,
        #[arg(long)]
        thread: Option<String>,
        #[arg(long)]
        address: Option<String>,
        /// Poll until the request ends and print the receipt.
        #[arg(long)]
        wait: bool,
    },
    Get {
        erasure: String,
    },
    List {
        #[arg(long)]
        status: Option<String>,
        #[command(flatten)]
        paging: Paging,
    },
    /// Submit a failed request's scope and target again, which resumes it.
    Retry {
        erasure: String,
        /// The counterparty's address (counterparty scope; only its hash is stored).
        #[arg(long)]
        address: Option<String>,
    },
    /// Re-apply a completed erasure after a restore (platform key).
    Reapply {
        erasure: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ExportCmd {
    Create {
        #[arg(long, value_parser = ["counterparty", "identity"])]
        scope: String,
        #[arg(long)]
        address: Option<String>,
    },
    Get {
        export: String,
        /// Save the ZIP from its signed link.
        #[arg(long)]
        download: Option<String>,
    },
    Delete {
        export: String,
    },
}

#[derive(Args, Debug)]
pub struct AuditArgs {
    #[arg(long)]
    pub action: Option<String>,
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long)]
    pub after: Option<String>,
    #[command(flatten)]
    pub paging: Paging,
}
