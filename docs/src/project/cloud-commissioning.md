# Cloud commissioning

How the owner, as the operator of Pylota Identity Cloud, sets up the production deployment at
`pylotamail.com`: its configuration, the Pylota partner and Pylota's own tenants, the checks, and the gates
that open it first to Pylota alone, then to the waitlist, then to everyone. [M20](build-plan.md#m20--staging-deploy-and-live-proof)
proves the software on staging; [M28](build-plan.md#m28--cloud-production-commissioning-after-m20) runs this
page against production (FR-OPS-5). Phone numbers ([§11](#11-phone-numbers-the-telnyx-account)) and the vault
([§12](#12-the-vault)) are commissioned after mail, each behind its own flag and gate, and the SDKs are
published from the accounts of [§13](#13-sdk-publishing). [M49](build-plan.md#m49--launch-after-m28-m41-m45-and-m48) closes the launch;
[Launch readiness](launch-readiness.md) tracks every gate on one page.

The owner is a solo developer and the only operator. Nothing here relies on a second person. The controls
are mechanical: a configuration that fails closed, scoped and short-lived credentials, the checks in
[§9](#9-checks), a fresh-agent dry run of this page, and an independent adversarial agent review before
each gate ([§10](#10-go-live-gates)). Pylota's own servers never hold a Pylota Identity platform key: Pylota
holds a partner key only.

| | |
|---|---|
| Requirement | FR-OPS-5 ([PRD](prd.md#613-operations)) |
| Built in | [M28](build-plan.md#m28--cloud-production-commissioning-after-m20), after M20 |
| Related | [Cloud sign-up §2](design/cloud-signup.md#2-hostnames) (hostnames), [Configuration](../reference/configuration.md), [Deploy to Cloudflare](../self-hosting.md), [Billing](design/billing.md#stripe-integration), [REST API › Partners](../reference/api.md#partners), [Phone numbers](design/phone-numbers.md), [Agent credential vault](design/vault.md), [SDKs, skills and agent tooling](design/sdk-and-skills.md), [Launch readiness](launch-readiness.md) |
| External facts read on 2026-10-26 | `https://pylotamail.com/terms` answered `404` (the legal pages of [§4](#4-production-configuration) are not yet published). PyPI `pylota-identity` and `pylota_identity` answered `404` (`pypi.org/pypi/<name>/json`), npm `@pylotahq/identity` answered `404`, and the Go module proxy had no versions of `github.com/PILOTAAI/pylota-identity-go`: none of the four is published yet. Telnyx's documentation was not reachable from the session that wrote §11; its Telnyx facts are those of [Phone numbers](design/phone-numbers.md), read 2026-10-24, and are re-read before gate P |
| External facts read on 2026-10-10 | Cloudflare [Workers roles and permissions](https://developers.cloudflare.com/workers/authorization/workers/) (last updated 2026-09-15) and [Durable Objects roles and permissions](https://developers.cloudflare.com/workers/authorization/durable-objects/): per-Worker roles for account-owned API tokens; creating a Worker needs product-level Admin, because per-Worker roles apply only to Workers that exist; Custom Domains do not support per-Worker roles yet; Durable Objects take the permissions of their Worker. Not verified: that a per-Worker `Editor` token can apply the first Durable Object migration; check CC3 proves it on the first deploy |

## 1. What runs where

| Piece | Production value | Owner decision |
|---|---|---|
| Cloudflare account | Pylota's existing account, shared with Pylota's own Workers, "shared, tightened" | D10, reaffirmed 2026-10-10 |
| Zones | `pylotamail.com` (Cloud) and `pylota.io` (Pylota's own; only its `notify.` and `reminders.` subdomains are used here) | 2026-10-09, 2026-10-10 |
| Worker `pylota-identity` | The service, on `api.pylotamail.com` (`PM_API_HOST`) and `app.pylotamail.com` (`PM_CONSOLE_HOST`) as Custom Domains | [Cloud sign-up §2](design/cloud-signup.md#2-hostnames) |
| Worker `pylota-mail-site` | The landing page and docs, on `pylotamail.com` and `www.pylotamail.com` (`site/wrangler.jsonc`) | – |
| Platform mail domain | `pylotamail.com` apex (`PM_PLATFORM_DOMAIN`) | 2026-10-09 |
| Amazon SES | `eu-west-2` (London), production access, for `dns_records`, `send_only`, `smtp_relay` with `inbound: ses`, the failover, and marketing mail | 2026-10-09 |
| Stripe | Live mode, GBP prices: Free £0, Developer £10, Team £49.50 a month, top-ups £1 | 2026-10-10 |
| Google and GitHub sign-in | One production OAuth client each, redirecting to `app.pylotamail.com` | – |
| Telnyx | TREFT LTD's existing Telnyx account (UK ordering set up; US pending), the one provider account of [Phone numbers §2.1](design/phone-numbers.md#21-one-account-and-provider_account_id-everywhere) | UK set up, stated by the owner 2026-10-26; the account choice is [Phone numbers §39](design/phone-numbers.md#39-open-questions-for-the-owner) question 3 |
| Vapi | No deployment account: each tenant connects its own Vapi organisation for the `vapi` voice route ([Phone numbers §14.2](design/phone-numbers.md#142-vapi)) | – |
| Package registries | crates.io (owner's machine), npm organisation `pylotahq`, PyPI, the Go module proxy through GitHub tags ([§13](#13-sdk-publishing)) | – |
| Licensor | TREFT LTD | – |

Cloudflare's daily Email Sending quota is per account, so Cloud shares it with Pylota's own Workers
(decided 2026-10-10). `PM_DAILY_SEND_QUOTA` holds the account's figure, so the alert at 80% covers both.

## 2. Release order

Pylota's integration needs Cloud before the public does, so Cloud opens in three phases, and agent
self-sign-up comes after them as a fourth step. Each phase is entered only through its gate
([§10](#10-go-live-gates)); going back is always one variable away.

| Phase | Who can get in | `PM_SIGNUP` | What changes from the previous phase |
|---|---|---|---|
| **A. Partner-only** | Pylota's partner key and the workspaces it creates; the owner's default tenant | `waitlist` (the landing page collects the waitlist; nobody is invited) | Everything in [§4](#4-production-configuration) and [§7](#7-bootstrap-the-partner-and-pylotas-tenants) |
| **B. Waitlist** | Plus people the owner invites in batches with `idvy waitlist invite` | `waitlist` | Invitations start; paid plans can be bought |
| **C. Open (public v1.0 on Cloud)** | Anyone | `open` | `PM_SIGNUP=open` |
| **D. Agent self-sign-up** | Plus AI agents that sign themselves up and name the person accountable for them ([Agent self-sign-up](design/agent-signup.md)) | `open` | `PM_AGENT_SIGNUP=on`, behind gate D |

**Rolling back a phase.** `PM_SIGNUP=closed` stops all sign-ups and waitlist confirmations at the next
deploy and leaves every account working; `PATCH /v1/partners/{partner_id}` with `status: suspended` contains
Pylota at once ([J13](edge-cases.md)); `npx --yes wrangler@4.139.0 rollback` returns the Worker to the
previous version ([Security §10](design/security.md#10-rate-limiting-and-abuse)). `PM_AGENT_SIGNUP=off`, or
`idvy ops switch agent_signup off` without a deploy, stops new agent sign-ups and their mail while every
provisional workspace keeps its lifecycle.

## 3. The Cloudflare account: shared, tightened

The account also runs Pylota's own Workers, so no credential that Cloud uses may reach them.

- **Workers are created by the owner in the dashboard**, signed in as a person: `pylota-identity` and
  `pylota-mail-site`, each with the dashboard's starter script. Creating a Worker needs product-level Admin,
  which an API token would then hold for every Worker of the account; a dashboard session is not a stored
  credential.
- **Custom Domains are attached by the owner in the dashboard**: `api.pylotamail.com` and
  `app.pylotamail.com` to `pylota-identity`, `pylotamail.com` and `www.pylotamail.com` to `pylota-mail-site`.
  Custom Domains do not support per-Worker roles yet; once attached, later deploys need only `Editor` on the
  Worker, as long as they do not add or remove a domain. `idvy setup` finds them present and creates
  nothing.
- **Every API token is account-owned, scoped to named zones and to single Workers** (per-Worker roles),
  never to all Workers of the account:

| Token | Held by | Workers | Zones | Account permissions | Lifetime |
|---|---|---|---|---|---|
| Setup token (`CLOUDFLARE_API_TOKEN`) | The owner's shell, for `idvy setup`, `setup ses`, `deploy`, `upgrade`, `doctor` and `secrets rotate-master` | `Editor` on `pylota-identity` only | `pylotamail.com`: Zone Read, DNS Edit, Zone Settings Edit, Email Routing Rules Edit | D1 Edit, Workers R2 Storage Edit, Queues Edit, Vectorize Edit, Workers AI Read and Edit, Email Sending Edit, Account Settings Read, Account Analytics Read | Created for one working session with an expiry 24 hours ahead, deleted at the end of it |
| Worker token (`PM_CF_API_TOKEN`, a Worker secret) | The Worker | None | `pylotamail.com` and `pylota.io` only: Zone Read, DNS Edit, Zone Settings Edit, Email Routing Rules Edit. No Zone Edit: `domains.allow_create_zone` is off on Cloud, so the Worker never creates a zone | Email Sending Edit, Queues Edit | Standing; rotated every 90 days and after any suspected leak |
| Site token (`CLOUDFLARE_API_TOKEN` of the GitHub environment `production`) | `site.yml`, which deploys `pylota-mail-site` on every push to `main` that changes the site | `Editor` on `pylota-mail-site` only | `pylotamail.com`: Workers Routes Edit | none | Standing, because `site.yml` runs unattended; rotated every 90 days and after any suspected leak. It reaches no other Worker and no DNS |
| Production deploy token (`PROD_CLOUDFLARE_API_TOKEN` of the GitHub environment `production`) | `cloud-deploy.yml`, which only the owner starts, for `idvy deploy --gradual` | `Editor` on `pylota-identity` only | none: a deploy changes no DNS and adds no Custom Domain | D1 Edit (migrations) and Queues Edit (Wrangler attaches the queue consumers on deploy); nothing else | Created for one rollout with an expiry 24 hours ahead and set as the environment secret just before the owner starts the workflow; an expired secret makes the next rollout fail before it changes anything. The D1 and Queues permissions are account-wide, which is why it is never standing |

The setup token's D1, R2, Queues and Vectorize permissions, and the production deploy token's D1 and
Queues permissions, are account-wide for those products, because those products have no per-resource
roles for this use; that is why both tokens live for one session or one rollout only. Whether a deploy
needs Queues Edit, or only D1 Edit, is checked on staging at M20 with a token that lacks it; the
permission is dropped if the deploy succeeds without it. Staging lives in a separate account, so
`STAGING_CLOUDFLARE_API_TOKEN` (for `release-publish.yml`'s `staging` job) is standing and holds the
setup token's permissions on that account only.
No token holds token management, billing, members or Email Routing destination addresses
([Deploy › step 2](../self-hosting.md#2-create-a-cloudflare-api-token) lists what each permission is for).

**What Cloud offers tenants as a result.** `nameservers` is not offered (`422 transport_unavailable`,
`zone_creation_not_allowed`); `delegated_subdomain` is off (`PM_CF_SUBDOMAIN_SETUP=off`); `cloudflare_zone`
works only on names a platform key lists in a tenant's `domains.cloudflare_zones`, which on Cloud means only
the exact names `=notify.pylota.io`, `=reminders.pylota.io` and `=account.pylota.io` for Pylota's own three
tenants, never the `pylota.io` zone itself. Customers bring their own domains with `dns_records`,
`send_only` and `smtp_relay` ([Domains on any DNS host](design/domain-connections.md)), each shipping only if
its spike passed (S8, S11, S12).

## 4. Production configuration

`idvy setup` writes `deploy/wrangler.toml` with self-hosting defaults (`PM_SIGNUP = "closed"`,
`PM_BILLING = "off"`). The owner then sets the `[vars]` below in that file and runs `idvy deploy`. Secrets
go only through `npx --yes wrangler@4.139.0 secret put <NAME>` (the value on stdin), or are generated and
uploaded by `idvy setup` and `idvy setup ses`; none is written to a file. The rendered file is kept in the
owner's private operations repository, never in `PILOTAAI/pylota-identity`.

**Variables:**

| Variable | Production value | Why |
|---|---|---|
| `PM_ENV` | `production` | Shown in `/health` |
| `PM_PLATFORM_DOMAIN` | `pylotamail.com` | The shared mail domain, a zone apex |
| `PM_API_HOST` | `api.pylotamail.com` | REST, MCP, hooks, the Stripe webhook; the assertion issuer |
| `PM_CONSOLE_HOST` | `app.pylotamail.com` | Keeps session cookies off the API host |
| `PM_JURISDICTION` | `eu` | D1, R2 and Durable Objects in the EU; fixed at setup |
| `PM_CF_ACCOUNT_ID` | Pylota's account ID (written by setup) | – |
| `PM_SIGNUP` | `waitlist` in phases A and B, `open` in phase C | [§2](#2-release-order) |
| `PM_CONSOLE` | `on` | – |
| `PM_SYSTEM_FROM` | `Pylota Identity <no-reply@pylotamail.com>` | Sign-in, invitation and notification mail |
| `PM_NOTIFICATIONS` | `on` | – |
| `PM_QUARANTINE_KEY_RELEASE` | `off` | Only people release quarantined mail, except where a partner opted a tenant in (FR-CON-6) |
| `PM_DEFAULT_POLICY` | The JSON of [§5](#5-the-cloud-default-policy) | – |
| `PM_BILLING` | `stripe` | – |
| `PM_PLAN_CATALOG` | The live catalog of [§6](#6-stripe-live-mode) | – |
| `PM_BILLING_GRACE_DAYS` | `7` | FR-BILL-10 |
| `PM_TERMS_URL`, `PM_PRIVACY_URL`, `PM_DPA_URL` | `https://pylotamail.com/terms`, `https://pylotamail.com/privacy`, `https://pylotamail.com/dpa` | Required when `PM_SIGNUP` is not `closed`; the site must serve all three before phase A |
| `PM_TERMS_VERSION` | The date of the published terms, `YYYY-MM-DD` | Stored on each user at sign-up |
| `PM_SIGNUP_BLOCKED_DOMAINS` | unset (the built-in disposable-domain list applies) | Add domains only after abuse is seen |
| `PM_OAUTH_GOOGLE_CLIENT_ID`, `PM_OAUTH_GITHUB_CLIENT_ID` | The production clients of [§8](#8-oauth-applications) | – |
| `PM_SES_REGION` | `eu-west-2` | Written by `idvy setup ses` |
| `PM_SES_SNS_TOPIC_ARN`, `PM_SES_INBOUND_BUCKET`, `PM_SES_INBOUND_TOPIC_ARN`, `PM_SES_INBOUND_QUEUE_URL`, `PM_SES_RULE_SET` | As `idvy setup ses` writes them | – |
| `PM_CF_SUBDOMAIN_SETUP` | `off` | No Enterprise child zones on Cloud |
| `PM_WEB_BOT_AUTH` | `off` | Turned on only after spike S13 passed and the owner decides, by a later change to this page |
| `PM_AGENT_SIGNUP` | `off` | Until gate D ([§10](#10-go-live-gates)) |
| `PM_AGENT_SIGNUP_DAILY_CAP` | `200` | Agent sign-ups per UTC day for the deployment ([Cloud sign-up § 10.2](design/cloud-signup.md#102-system-mail-budgets)) |
| `PM_DAILY_SEND_QUOTA` | The account's Email Sending daily quota from the dashboard (shared with Pylota) | Alert at 80% |
| `PM_BACKUP_BUCKET` | `pylota-identity-backup` | Paying customers' mail gets the nightly copy (NFR-OPS-2) |
| `PM_SECURITY_CONTACT` | A mailbox outside `pylotamail.com` that the owner reads, for example `security@pylota.io` | Mail to role names on the platform domain routes to it, so it must not be on that domain |
| `PM_TRUSTED_AUTHSERV_ID` | Written by setup's mail test | – |
| `PM_LOG_LEVEL` | `info` | – |
| `PM_AI_GATEWAY`, `PM_SCANNER_URL` | unset | – |
| `PM_DOCS_URL` | unset | The default, `https://pylotamail.com/docs`, is where Cloud's docs are published |
| `PM_EMBED_MODEL`, `PM_RERANK_MODEL`, `PM_AGENT_MODEL`, `PM_TRIAGE_MODEL`, `PM_SCREEN_MODEL`, `PM_DOH_RESOLVERS`, `PM_IDENTITY_KEY_OVERLAP_DAYS` | Defaults | – |
| `PM_PHONE` | `off` until gate P ([§10](#10-go-live-gates)) | [Phone numbers §30](design/phone-numbers.md#30-rollout-telemetry-and-rollback) |
| `PM_TELNYX_API_BASE` | The Telnyx API base that matches the data locality chosen in [§11](#11-phone-numbers-the-telnyx-account) step 3 (`https://api.telnyx.com/v2` or `https://api.telnyx.eu/v2`) | Checked at isolate start |
| `PM_TELNYX_ACCOUNT_ID` | The Telnyx organisation ID of the account | Checked against `GET /v2/balance` at isolate start |
| `PM_PHONE_CATALOGUE`, `PM_PHONE_RATE_CARD` | unset: the built-in catalogue and rate card of the release | A change to either is a release, so its `verified_at` and the margin are reviewed |
| `PM_PHONE_KYC_RETENTION_DAYS` | `365` until counsel sets periods per country | [Phone numbers §4.6](design/phone-numbers.md#46-retention-and-erasure) |
| `PM_VAULT` | `off` until gate V | [Agent credential vault §15](design/vault.md#15-kill-switches-and-incident-response) |
| `PM_VAULT_REFUSED_DOMAINS`, `PM_VAULT_CLEARANCE_COOKIES` | unset (the built-in lists apply) | Added to only after counsel or an incident |

**Secrets:**

| Secret | Source |
|---|---|
| `PM_MASTER_KEY`, `PM_KEY_PEPPER`, `PM_HASH_KEY` | Generated and uploaded by `idvy setup` (no `--print-secrets`) |
| `PM_CF_API_TOKEN` | The Worker token of [§3](#3-the-cloudflare-account-shared-tightened) |
| `PM_SES_ACCESS_KEY_ID`, `PM_SES_SECRET_ACCESS_KEY` | Created and uploaded by `idvy setup ses` |
| `PM_STRIPE_SECRET_KEY` | A live restricted key (`rk_live_…`) with exactly the permissions of [Billing › Stripe integration](design/billing.md#stripe-integration) |
| `PM_STRIPE_WEBHOOK_SECRET` | The signing secret of the live webhook endpoint ([§6](#6-stripe-live-mode)) |
| `PM_OAUTH_GOOGLE_CLIENT_SECRET`, `PM_OAUTH_GITHUB_CLIENT_SECRET` | The production OAuth clients |
| `PM_TELNYX_API_KEY`, `PM_TELNYX_PUBLIC_KEY` | Set by `idvy setup phone` ([§11](#11-phone-numbers-the-telnyx-account) step 5): a Telnyx API key made for Pylota Identity Cloud alone (a Worker secret), and the account's webhook public key (a variable in `deploy/wrangler.toml`: it is public). Set only when gate P is being prepared; absent, every phone request fails `not_configured` |

`idvy setup` for production:

```bash
export CLOUDFLARE_API_TOKEN=…      # the setup token of §3, created for this session
idvy setup --account-id <pylota-account-id> --domain api.pylotamail.com \
  --console-host app.pylotamail.com --mail-domain pylotamail.com --jurisdiction eu \
  --owner-email <owner address> --owner-name "<owner name>" --tenant-name "Pylota Identity" \
  --daily-send-quota <account quota> --backup-bucket pylota-identity-backup --profile cloud-prod
idvy setup ses --region eu-west-2
```

The default tenant ("Pylota Identity", billing `disabled`) holds only the system identity and the owner's
console account; it is never a customer workspace.

## 5. The Cloud default policy

```json
{
  "domains": { "allow_create_zone": false },
  "accounts": { "require_approval": true },
  "phone": { "spend": { "tenant_monthly_cap_pence": 2000, "identity_monthly_cap_pence": 1000 } }
}
```

- `domains.allow_create_zone: false` is already the built-in default; stating it records the
  shared-account decision: the Worker token cannot create zones, and no Cloud tenant may use `nameservers`.
- `accounts.require_approval: true` turns on the [service sign-up ledger](design/service-accounts.md) gate
  for every Cloud tenant. It is lower-only with `false` as the looser value, so neither a workspace nor a
  partner can turn it off; only a platform key can, tenant by tenant
  ([Workspace policy §2](design/workspace-policy.md#2-classes-and-ceilings)).
- Every other field keeps its built-in default. `quarantine.key_release` stays `false`,
  `drafts.key_approval` stays `false` and `web_bot_auth.allowed` stays `false`. Pylota's partner key sets
  `drafts.key_approval` on its own operators' workspaces when it creates them
  ([Drafts § 19](design/drafts.md#19-pylota-agentic-os)); no other workspace's keys approve drafts.
- `phone.spend.*` are the Cloud defaults of [Phone numbers §18.3](design/phone-numbers.md#183-spend-caps) (£20 a
  workspace and £10 an identity a month), lower-only. The built-in defaults have no cap, so Cloud states them
  here. The amounts are owner question 2 of [Phone numbers §39](design/phone-numbers.md#39-open-questions-for-the-owner);
  any cap on one-off registration fees that the phone design adds is set beside them in the same change.
- `phone.allowed` and `vault.allowed` keep their built-in default, `false`. They are turned on tenant by tenant
  in [§11](#11-phone-numbers-the-telnyx-account) and [§12](#12-the-vault); `vault.allowed` only by a platform key.

## 6. Stripe live mode

In the Stripe Dashboard, live mode, for TREFT LTD:

1. **Products and prices**, all monthly, recurring, GBP, tax behaviour as Stripe Tax requires: Developer
   £10, Team £49.50, and three top-up prices at £1 per unit (inboxes, sends, triage).
2. **Stripe Tax** on, with TREFT LTD's registrations; the Customer Portal configured as
   [Billing › Customer Portal](design/billing.md#customer-portal) describes.
3. **Webhook endpoint** `https://api.pylotamail.com/billing/stripe/webhook`, API version
   `2025-03-31.basil`, with exactly the events of [Billing › Events handled](design/billing.md#events-handled);
   its signing secret becomes `PM_STRIPE_WEBHOOK_SECRET`.
4. **Restricted key** with exactly the permissions of [Billing › Stripe integration](design/billing.md#stripe-integration);
   it becomes `PM_STRIPE_SECRET_KEY`.
5. **`PM_PLAN_CATALOG`**: the built-in catalog of [Billing › Plan catalog](design/billing.md#plan-catalog)
   with the live price IDs filled in (`plans[].stripe_price_id` for Developer and Team,
   `topup.stripe_price_ids` for the three top-ups). `idvy deploy` refuses an invalid catalog. The staging
   catalog with small allowances (M20 step 11) is never used here.

## 7. Bootstrap the partner and Pylota's tenants

The owner runs these commands from their own machine. Platform keys are short-lived: setup's bootstrap key
expires after 24 hours, and every later session uses a new platform key with `--expires-in 1d`, read by the
CLI through `key_command` from the owner's password manager, never stored in a file
([Configuration › CLI configuration](../reference/configuration.md#cli-configuration)). One long-lived
break-glass key, kept only in the break-glass record and paging on any use, recovers a gap of more than a
day without rotating the pepper ([J36](edge-cases.md)). No platform key is
ever given to Pylota's servers.

```bash
# 1. A platform key for this session (the profile reads it with key_command).
idvy --profile cloud-prod keys create --level platform --name ops-2026-10-12 --expires-in 1d \
  --permissions tenants:manage,partners:manage,platform:ops,keys:manage,domains:read,domains:write,\
identities:read,policy:write,audit:read,usage:read

# 1a. The break-glass platform key: the same permissions, 400 days. Its secret goes only into the
#     pylota-identity-break-glass vault and the sealed printed copy (Observability 5.7), never into a profile.
#     Then set PM_BREAK_GLASS_KEY_ID to its key_ ID and redeploy, so any use of it pages.
idvy --profile cloud-prod keys create --level platform --name break-glass --expires-in 400d \
  --permissions tenants:manage,partners:manage,platform:ops,keys:manage,domains:read,domains:write,\
identities:read,policy:write,audit:read,usage:read

# 2. The Pylota partner: exempt billing, more tenants than the default 25, no send ramp.
idvy --profile cloud-prod partners create --name Pylota --default-billing-mode exempt \
  --max-tenants 200 --ramp-exempt true

# 3. Pylota's partner key, 90 days. The secret goes straight into Pylota's backend secret store.
#    erasure:manage and tenants:erase let Pylota delete an operator's workspace (and, later, the
#    partner); suppressions:manage keeps do-not-contact lists; audit:read reads its tenants' audit log.
#    No policy:write: every key Pylota mints must be a subset of this one, so an agent's tenant key
#    could otherwise be given policy:write. Pylota sets tenant policy through tenants:manage.
idvy --profile cloud-prod keys create --level partner --partner <ptn_id> --name pylota-backend \
  --expires-in 90d --permissions tenants:manage,tenants:erase,erasure:manage,keys:manage,webhooks:manage,\
webhooks:read,quarantine:review,suppressions:manage,audit:read,usage:read,identities:read,identities:write,\
domains:read,domains:write,messages:read,messages:send,messages:write,attachments:read,search:read,\
members:manage,accounts:approve
```

`max_tenants` 200 bounds Pylota at 200 × 5,000 sends a day in policy terms; the shared Email Sending quota
is the tighter bound in practice, and its 80% alert fires first. Raising it later is a platform-key
`PATCH /v1/partners/{partner_id}`.

**Pylota's own three tenants**, created with Pylota's partner key (a profile `pylota-partner` that holds it),
so they are Pylota's, billed `exempt` and outside the ramp. The auth tenant keeps Pylota's sign-in,
invitation and waitlist mail apart, with a short retention, so codes and links are not kept for the life of
the platform tenant's mail:

```bash
# 4. Pylota's platform tenant (transactional mail from notify.pylota.io), marketing tenant
#    (reminders, win-back and expired quotes from reminders.pylota.io, with consent) and auth tenant
#    (sign-in, invitation and waitlist mail from account.pylota.io; raw MIME 1 day, messages 2 days).
#    The suffix is .pylota-platform, not .pylota: `pylota` is a reserved suffix.
idvy --profile pylota-partner tenants create --slug pylota-platform --name "Pylota platform"
idvy --profile pylota-partner tenants create --slug pylota-marketing --name "Pylota marketing"
idvy --profile pylota-partner tenants create --slug pylota-auth --name "Pylota sign-in" \
  --policy-file pylota-auth-policy.json   # {"retention":{"raw_days":1,"message_days":2}}

# 5. Only a platform key grants the three names, each by exact name (domains.cloudflare_zones is
#    platform-only; a whole-zone entry would hand every pylota.io name to a stolen key, H18).
idvy --profile cloud-prod tenants update pylota-platform  --policy '{"domains":{"cloudflare_zones":["=notify.pylota.io"]}}'
idvy --profile cloud-prod tenants update pylota-marketing --policy '{"domains":{"cloudflare_zones":["=reminders.pylota.io"]}}'
idvy --profile cloud-prod tenants update pylota-auth      --policy '{"domains":{"cloudflare_zones":["=account.pylota.io"]}}'

# 6. Pylota adds the three subdomains with its partner key (cloudflare_zone, exactly the granted name;
#    nothing else in pylota.io is reachable).
idvy --profile pylota-partner domains add notify.pylota.io --tenant pylota-platform --method cloudflare_zone
idvy --profile pylota-partner domains add reminders.pylota.io --tenant pylota-marketing --method cloudflare_zone
idvy --profile pylota-partner domains add account.pylota.io --tenant pylota-auth --method cloudflare_zone

# 7. Marketing mail needs SES: Cloudflare Email Service is for transactional mail only. The transport
#    change is platform-only. With SES configured, onboarding already created the SES identity and its
#    DKIM CNAMEs, so the switch needs no new DNS.
idvy --profile cloud-prod domains update reminders.pylota.io --transport ses
```

Each operator workspace is then created by Pylota's backend with its partner key
(`POST /v1/tenants`, with `policy.quarantine.key_release: true`), never by the owner. Marketing sends from
`reminders.pylota.io` use `kind: marketing` with `unsubscribe` and `consent` (FR-OUT-8); a marketing send
from a `cloudflare` transport domain is refused with `422 transport_unavailable` (`marketing_needs_ses`).

## 8. OAuth applications

| Provider | Setting | Value |
|---|---|---|
| Google | OAuth client type | Web application, in a Google Cloud project owned by TREFT LTD |
| Google | Authorised redirect URI | `https://app.pylotamail.com/console/oauth/google/callback` |
| Google | Scopes | `openid email profile` only; consent screen published, with the terms and privacy URLs of [§4](#4-production-configuration) |
| GitHub | OAuth app callback URL | `https://app.pylotamail.com/console/oauth/github/callback` |
| GitHub | Scopes requested by the Worker | `read:user user:email` |

Provider endpoints and claim names follow [Cloud sign-up §4](design/cloud-signup.md#4-google-and-github),
re-read when M24 was built; re-read both providers' current documentation again before phase B, and record
the date on this page.

## 9. Checks

Run against production by the owner, each with its command and expected result. A check that fails stops
the phase; nothing is fixed by hand in the dashboard without the change being written back into this page.

| ID | Check | How | Pass |
|---|---|---|---|
| CC1 | Health and version | `curl https://api.pylotamail.com/health` | `200`, `env: production`, `version` = the release tag verified by `idvy deploy` |
| CC2 | Doctor | `idvy --profile cloud-prod doctor --mail-test` | Exit 0: DNS, routing, sending, event subscriptions, bindings, secrets and quota all pass |
| CC3 | Tokens are scoped | Each token's policies in the dashboard (Manage Account › Account API Tokens), and the first `idvy setup` and `idvy deploy` with the setup token | No standing token has a product-level Workers role; zones are exactly those of [§3](#3-the-cloudflare-account-shared-tightened); the first deploy, including the Durable Object migration, succeeded with the per-Worker `Editor` token; the setup token is deleted at the end of the session |
| CC4 | Configuration | Compare `deploy/wrangler.toml` `[vars]` with [§4](#4-production-configuration), and the Worker's secret list (Wrangler `secret list`, pinned as everywhere) with its secrets table | Equal, line by line; no other variable or secret |
| CC5 | Host split | `curl -i https://api.pylotamail.com/console`; `curl -i https://app.pylotamail.com/v1/me` | Both `404`; no `Set-Cookie` from the API host |
| CC6 | System mail | Sign in at `app.pylotamail.com` with an email link | The mail comes from `no-reply@pylotamail.com` with DKIM `pass` aligned to `pylotamail.com` |
| CC7 | Zone creation off | With a test tenant key, `POST /v1/tenants/{tenant_id}/domains` with `method: nameservers` | `422 transport_unavailable`, `details.reason = "zone_creation_not_allowed"` |
| CC8 | Key release off | With a key holding `quarantine:review` on a workspace without `quarantine.key_release`, release a quarantined message | `403 permission_denied` |
| CC9 | Partner containment | Pylota's partner key reads the owner's default tenant, and creates a test tenant and reads it | `404 tenant_not_found`, then `201` and `200`; the test tenant has `partner_id` and billing `exempt`; it is erased after CC10 |
| CC10 | Ledger gate | On Pylota's test tenant, `wait(kind=verification, from=@service.example)` for an identity with no approved entry | `403 policy_denied`, `details.reason = "account_not_approved"` |
| CC11 | SES | `idvy setup ses` exit 0; both SNS topics have `SignatureVersion=2`; a marketing send from `reminders.pylota.io` to the owner's mailbox | Delivered with DKIM aligned to `reminders.pylota.io` and the unsubscribe headers |
| CC12 | Pylota's domains | `idvy domains health notify.pylota.io` and `reminders.pylota.io` | `healthy`; adding the `pylota.io` apex with Pylota's key gets `403 scope_denied` (`zone_not_allowed`) |
| CC13 | Send quota alert | `PM_DAILY_SEND_QUOTA` set; the alert rule is listed by `idvy doctor` | Present |
| CC14 | Backups | The day after setup, list `pylota-identity-backup` | The nightly job copied the day's `t/` objects |
| CC15 | Stripe | A real Developer Checkout by the owner on a throwaway workspace, a top-up, a Portal plan switch, then cancellation and a refund in the Dashboard | The plan changes only through the webhook; `billing_events` has no `error:` outcome; the workspace returns to Free |
| CC16 | Google and GitHub | Sign up with each on `app.pylotamail.com`, with a waitlist invite | Each creates one account with the verified address |
| CC17 | Waitlist | Join the waitlist with an address the owner controls, confirm it, invite it with `idvy waitlist invite --count 1`, sign up | One `waitlist` row, then one account and workspace on Free |
| CC18 | DMARC | `dig TXT _dmarc.pylotamail.com` | `p=none` with `rua` at phase A; the ramp to `quarantine` and `reject` follows [Deploy › The DMARC ramp](../self-hosting.md#dns-authentication-and-the-dmarc-ramp) |
| CC19 | Security contact | `curl https://api.pylotamail.com/.well-known/security.txt` | Served, with `PM_SECURITY_CONTACT` |
| CC20 | Legal pages | `curl -I` each of `PM_TERMS_URL`, `PM_PRIVACY_URL`, `PM_DPA_URL` and `https://pylotamail.com/subprocessors` | `200`; the sub-processor page lists every processor of [Launch readiness §4](launch-readiness.md#4-legal-pages-and-processors) for the features turned on |
| CC21 | Telnyx account | `idvy --profile cloud-prod doctor` with `PM_TELNYX_*` set; the Telnyx portal's account page | `phone` section passes: account level recorded (Verified for US, [§11](#11-phone-numbers-the-telnyx-account) step 1), webhook channel present, signature of a test event verifies, `PM_TELNYX_ACCOUNT_ID` matches `GET /v2/balance`, data locality as decided |
| CC22 | Phone off by default | With `PM_PHONE=on` and a test tenant whose `phone.allowed` is `false`, `GET /v1/phone/countries` | `403 policy_denied`; `/hooks/telnyx` with `PM_PHONE=off` answers `503` |
| CC23 | GB number end to end | On Pylota's test tenant with an approved GB profile: search, request, approve, number `active`; SMS to and from the owner's phone; release approved and done | Each step's event arrives; the release leaves no number at Telnyx (complete listing) and no charge after the month |
| CC24 | US number end to end | As CC23 with a US local number, after the tenant's 10DLC campaign is `MNO_PROVISIONED`; one send before provisioning | The early send gets `403 a2p_not_registered`; after provisioning the send is delivered; no outbound call before `enable_emergency` |
| CC25 | Phone spend limits | The account's per-product spend limits and each test tenant's messaging and outbound voice profile `daily_spend_limit` | Set as [Phone numbers §18.4](design/phone-numbers.md#184-provider-side-backstops-and-balance) says; `phone_provider_balance_low` fires in a staging rehearsal with a lowered threshold |
| CC26 | Port-out guard | On the test number: `external_pin` set and its vault managed secret present; a port-out request simulated on staging | `port_pin: "set"`; the guard raises its alert and waits for a person |
| CC27 | Phone kill switches | `idvy ops switch phone paused_purchases`, then `paused`, then `on`; `PM_PHONE=off` and on again | Purchases refused, then sends refused, then normal; with the flag back on the reconciler runs first and reports no drift |
| CC28 | Vault off by default | With `PM_VAULT=on` and a tenant whose `vault.allowed` is `false`, `GET /v1/tenants/{tenant_id}/vault` | `422 vault_disabled` |
| CC29 | Vault end to end | On Pylota's test tenant: an item, a grant approved by a person, a use, a ticket redeemed by Pylota's activated browser consumer, a report, a session write-back | Every step writes its audit row; `GET …/vault/audit` verifies from the first row; the item's test value appears in no API response, event, webhook delivery or log line (searched in the responses saved by the run and in `wrangler tail` output captured during it) |
| CC30 | Vault kill switches | `idvy ops switch vault_redeem off`, a redemption, `on`; `POST …/vault/revoke-all` on the test tenant | `422 vault_disabled`, then normal; every grant revoked and every open use closed |
| CC31 | Master-key rotation with vault rows | `idvy secrets rotate-master` on production with vault rows present, then `--resume` until `master_key.remaining` is 0 | Every sealed column, `vault_keys.private_sealed` included, re-sealed; a use mints and redeems afterwards |
| CC32 | Trust statement | `grep` of the site and the published docs for the forbidden claims of [Agent credential vault §1.2](design/vault.md#12-the-trust-model) | None found; the statement appears word for word on the landing page and in the vault guide |
| CC33 | SDK packages | `cargo search pylota-identity`, `npm view @pylotahq/identity`, `pip download pylota-identity==<v> --no-deps`, `go list -m github.com/PILOTAAI/pylota-identity-go@<v>` | Each resolves to the release's version; each artefact's checksum is in that repository's signed `SHA256SUMS` |

## 10. Go-live gates

| Gate | Enters | Every item must hold |
|---|---|---|
| **A** | Phase A, partner-only | M20 accepted on staging and the release tagged; CC1–CC14, CC18 and CC19 pass; the terms, privacy and DPA pages are live; a fresh agent that never saw this deployment rebuilds [§4](#4-production-configuration) and [§5](#5-the-cloud-default-policy) from the docs alone and finds no difference from the deployed configuration; an independent adversarial agent review of the deployed configuration, the token scopes and [§7](#7-bootstrap-the-partner-and-pylotas-tenants) finds nothing rated high; `PM_SIGNUP=waitlist` with no invitation sent |
| **B** | Phase B, waitlist invitations | Gate A; Pylota's integration has run on Cloud for 14 days with no service-owned edge row failing in production and no state alert left open; CC15, CC16 and CC17 pass; the OAuth documentation re-read is recorded; the DMARC record is at `p=quarantine` |
| **C** | Phase C, open sign-up | Gate B; the [pre-release checklist](design/security.md#16-pre-release-checklist) is complete, the external penetration test included; 30 days of phase B with every Free workspace's complaint and bounce rates under the auto-pause thresholds and no `signup_ramp_review` alert unanswered; the DMARC record is at `p=reject`; a second fresh-agent dry run and adversarial review pass on the phase-C configuration (`PM_SIGNUP=open`) |
| **D** | Agent self-sign-up (`PM_AGENT_SIGNUP=on`) | Gate C; phase C has run for at least 30 days with no `containment:*` page, no `shared_domain_breaker` at stage 2 and no unresolved `signup_ramp_review`; DMARC on the platform domain is at `p=reject`; the two `live::signup` tests (`live::signup::x40_refuse_one_click_headers` and `live::signup::end_to_end`) pass on production with the variable on for one hour, then it is turned off again until the owner decides ([Agent self-sign-up › Milestone](design/agent-signup.md#26-milestone)) |

| **P** | Phone numbers (`PM_PHONE=on`), in three stages: P1 Pylota's first operator, P2 every Pylota operator, P3 waitlist and open workspaces | Gate A; M38–M41 accepted on staging; checks CC20–CC27 pass; [§11](#11-phone-numbers-the-telnyx-account) steps 1–9 done for the countries offered (US numbers only after steps 1 and 7); counsel questions 1 and 7 of [Phone numbers §32](design/phone-numbers.md#32-questions-for-counsel) answered for GB, and 2, 10 and 11 answered before US is offered; the sub-processor page names Telnyx (and Vapi for the `vapi` route); a fresh-agent dry run of §11 and an adversarial review of the phone configuration find nothing rated high. Each later stage waits for the seven days of telemetry of [Phone numbers §30](design/phone-numbers.md#30-rollout-telemetry-and-rollback) |
| **V** | The vault (`PM_VAULT=on`), in two stages: V1 Pylota's tenants with Pylota's consumers, V2 other tenants | Gate A; M42–M45 accepted on staging; checks CC28–CC32 pass; Pylota's consumers activated by a platform key after the conformance fixtures pass ([§12](#12-the-vault)); the kill-switch rehearsal recorded; the vault's counsel questions answered; an independent adversarial review of the vault and of each consumer finds nothing rated high. V2 waits for 30 days of V1 with no `vault_value_exposed_total`, no `vault_audit_append_failures_total` and no consumer suspension |
| **S** | The Python and Go SDKs' first full release | M46–M48 accepted; CC33 passes on the release candidates; the trusted publishers of [§13](#13-sdk-publishing) configured and token publishing disallowed |

Gates P, V and S are independent of gates B to D: phone and the vault may open to Pylota's tenants in phase A
or B, and to others only once gate C has opened sign-up.

The owner records each gate in a dated note in the private operations repository: the release tag, the
check results, and the dry-run and review reports.

## 11. Phone numbers: the Telnyx account

**Status on 2026-10-26** (owner): ordering GB numbers works on TREFT LTD's Telnyx account; US is pending.
The steps below are done once, in this order, and each is recorded with its date in the private operations
repository. Telnyx facts are those of [Phone numbers](design/phone-numbers.md), read 2026-10-24; the owner
re-reads each named page before gate P and records the date here.

1. **Account level.** The account must be at the **Verified** level for US numbers, 10DLC and toll-free
   verification; the Paid level orders only local numbers in the account's own country
   ([Phone numbers §2.1](design/phone-numbers.md#21-one-account-and-provider_account_id-everywhere)). Record the
   level the account holds now. If it is not Verified, apply (KYC of TREFT LTD and Telnyx's AI agent
   evaluation). This is the first step on the US path and blocks nothing in GB that already works.
2. **Reseller requirements annex.** TREFT accepts it as the reseller of record. It obliges TREFT to keep end
   users' KYC records "for the locally mandated retention period" and give them to Telnyx or authorities on
   request ([Phone numbers §4.6](design/phone-numbers.md#46-retention-and-erasure)).
3. **Data locality.** Set once in the Telnyx portal and irreversible; the recommendation is EU
   ([Phone numbers §39](design/phone-numbers.md#39-open-questions-for-the-owner) question 4). Set
   `PM_TELNYX_API_BASE` to match. Decide before the first tenant number is bought through Pylota Identity.
4. **Which account.** Using the existing account lets Pylota's numbers be adopted without porting
   ([Phone numbers §7.6](design/phone-numbers.md#76-adoption-for-pylotas-migration)); a new account means
   porting. Owner question 3.
5. **`idvy setup phone`.** A Telnyx API key made for Pylota Identity Cloud alone, read from stdin, never the
   key another integration uses; the webhook public key from the portal; the notification profile, the webhook
   channel to `https://api.pylotamail.com/hooks/telnyx` and the notification settings for number orders,
   requirement groups, porting and port-outs ([Phone numbers §2.4](design/phone-numbers.md#24-deployment-set-up)).
   Rotation: every 90 days and after any suspected leak, by a new key, `wrangler secret put`, then deleting the
   old key in the portal.
6. **Balance and spend limits.** The account is prepaid: keep at least 30 days of expected spend, and set the
   account's per-product spend limits to 1.5 times the sum of tenant caps
   ([Phone numbers §18.4](design/phone-numbers.md#184-provider-side-backstops-and-balance)). `phone_provider_balance_low`
   pages at 7 days of average spend and pauses purchases at 2.
7. **US: 10DLC as an ISV.** One brand and one campaign per tenant, with brand and campaign webhooks to the hooks
   URL and every path under `/v2`; `live::phone::a2p_paths` passes on staging; each fee step is a request a
   person approves ([Phone numbers §13](design/phone-numbers.md#13-us-a2p-10dlc-and-toll-free-verification)).
   Before a UK-registered tenant may submit a US brand, one GB-registered brand is taken through vetting on
   staging and counsel question 11 is answered.
8. **US: emergency calling.** A US number that places calls needs a registered E911 address
   (`enable_emergency`, $1.50 a month); Telnyx charges $100 for an unregistered emergency call. Whether a tenant
   with no US location can register one is a question to Telnyx (owner question 6); until it is answered, such
   a tenant's US numbers place no calls ([Phone numbers §14.8](design/phone-numbers.md#148-emergency-calling)).
9. **Catalogue, rate card and policy.** The release's catalogue rows carry `verified_at` under 180 days; the
   owner has set the rate card's margin and buffer (owner question 1); `phone.allowed` and, for Pylota's
   operators, `phone.key_approval` are set by Pylota's partner key, which is re-issued with the `phone:*`
   permissions it needs ([§7](#7-bootstrap-the-partner-and-pylotas-tenants)), so Pylota approves from its own
   app with `on_behalf_of`.
10. **Migration.** Pylota's existing numbers are adopted (`POST /v1/platform/phone-numbers/adopt`) one operator
    at a time in stage P1 and P2, and re-papered under each operator's own profile within the period the
    phone design sets. Pylota's own Telnyx credentials are retired once every number is adopted or ported.

## 12. The vault

1. **Flag and tenants.** `PM_VAULT=on` at gate V; `vault.allowed: true` per tenant by a platform key
   ([Agent credential vault §15](design/vault.md#15-kill-switches-and-incident-response)), first for Pylota's
   tenants. Pylota's partner key is re-issued with the `vault:*` permissions its server needs; `vault:approve`
   is used only under `vault.key_approval` with `on_behalf_of`.
2. **Consumers.** Pylota registers its browser fleet, egress proxy and executor with its partner key. Each
   consumer generates its own key pairs on its own host (`idvy vault consumer-keys generate`); no private key
   leaves it. A platform key activates each one only after it passes the conformance fixtures of
   [Agent credential vault](design/vault.md) and its registration declares recording, logging and CAPTCHA
   solving off; a third-party hosted browser is activated only by a platform key, with a note naming the
   provider's settings read that day.
3. **Master key.** The vault's tenant keys are sealed under `PM_MASTER_KEY`. Before gate V,
   `idvy secrets rotate-master` is rehearsed on staging with vault rows present and run once on production
   (CC31); the break-glass record says that losing the master key makes every operator-custody vault value
   unrecoverable.
4. **Partner custody.** Optional, per tenant, by the tenant's own partner key. If Pylota enrols, both unlock
   keys go straight into Pylota's secret store and an offline backup it holds; Pylota Identity never keeps a
   copy, and a lost pair makes those items unrecoverable by design.
5. **Kill switches.** `PM_VAULT=off`, the `vault_redeem` ops switch, `revoke-all`, consumer suspension and
   partner suspension are rehearsed on staging and recorded (CC30), with the "Vault incident" runbook.
6. **Trust statement.** The statement of [Agent credential vault §1.2](design/vault.md#12-the-trust-model)
   appears word for word on the landing page and in the vault guide; no forbidden claim appears (CC32).

## 13. SDK publishing

No workflow holds a publishing token. Each registry is configured once, by the owner:

| Package | Registry | How it is published | Before the first release |
|---|---|---|---|
| `pylota-identity`, `pylota-identity-api-types`, `pylota-identity-cli` | crates.io | `cargo xtask release crates` from the owner's machine, with the token from the password manager ([Rust workspace](design/rust-workspace.md#9-xtask)) | The crate names reserved by TREFT LTD's crates.io account |
| `@pylotahq/identity` | npm | Trusted publishing from the SDK repository's `release-publish.yml` in environment `release` ([SDKs §2.16](design/sdk-and-skills.md#216-versioning-and-release)) | TREFT LTD owns the npm organisation `pylotahq`; token publishing disallowed on the package |
| `pylota-identity` (import `pylota_identity`) | PyPI | Trusted publishing (OIDC) from the Python SDK repository's release workflow in environment `release` | The project name registered by TREFT LTD's PyPI account, which has two-factor authentication; a pending trusted publisher configured before the first upload |
| `github.com/PILOTAAI/pylota-identity-go` | Go module proxy | A signed tag on the Go SDK repository; the proxy fetches it. A bad version is withdrawn with a `retract` directive in the next version, never by deleting the tag | Immutable releases and tag protection on the repository |

The names and supported language versions are those of the SDK languages decision (ADR 0022), checked
against the registries on the day of the first release.
