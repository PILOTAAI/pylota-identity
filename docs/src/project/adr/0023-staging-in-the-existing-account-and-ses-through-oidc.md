# 0023 Staging and the spikes in the existing Cloudflare account; SES through OIDC

| | |
|---|---|
| Status | Accepted |
| Date | 2026-10-27 |
| Deciders | The owner (TREFT LTD) |
| Related | [ADR 0010](0010-cloud-in-the-existing-cloudflare-account.md), [Build plan › Human prerequisites](../build-plan.md#human-prerequisites), [CLI and setup § 6](../design/cli.md#6-setup), [Domains on any DNS host § 4.2](../design/domain-connections.md#42-deployment-set-up-for-ses), [Security § 6](../design/security.md) |

## Context

The build plan asked for a **separate** Cloudflare account for staging and the M1 spikes, for two reasons:
setup used fixed resource names (`pylota-identity`, `pylota-identity-blobs`, `pm-*`) that would collide with
production's, and staging tokens should never reach production's zones. Pylota's existing account, where
Pylota Identity Cloud's production runs ([ADR 0010](0010-cloud-in-the-existing-cloudflare-account.md)), holds
the company's startup credits ($10,000, valid to 2027-09-10, covering every product except Registrar
purchases and AI Gateway, per the account's billing page on 2026-10-27). A second account would pay for
Workers Paid and usage itself.

The plan also gave staging and the spikes long-lived AWS access keys for SES (`STAGING_AWS_ACCESS_KEY_ID`,
`STAGING_AWS_SECRET_ACCESS_KEY`), to be rotated by hand.

## Decision

1. **Staging and the spikes run in Pylota's existing Cloudflare account** (owner, 2026-10-27), on Workers
   Paid, which the credits cover. Staging uses its own zones (a staging platform zone and a staging tenant
   zone), never `pylotamail.com`, `pylota.io` or any other zone in production use.
2. **A deployment has a name, and every resource name derives from it.** `idvy setup --name <name>` (default
   `pylota-identity`) sets the deployment name; the Worker, the D1 database and the R2 bucket are `{name}`,
   `{name}` and `{name}-blobs`, and the queues, the Vectorize index, the lifecycle rule IDs and the event
   subscription are prefixed `pm-` for the default name and `pm-{suffix}-` otherwise, where `{suffix}` is the
   name without `pylota-identity-`. Staging is `--name pylota-identity-staging` (queues `pm-staging-*`).
   Setup refuses a name whose Worker or database already exists with a different deployment's `PM_DEPLOYMENT_ID`
   ([CLI and setup § 6.1](../design/cli.md#61-flags)). Spikes use throwaway names `pi-spike-{n}-*` and delete
   what they create when they finish.
3. **Tokens are narrow and short-lived.**
   - The **spike token** has the first-run token's permissions plus AI Gateway · Edit and Account Analytics ·
     Read, zone permissions on the two staging zones only, and an expiry of at most 14 days. It is deleted
     when M1's gate is met.
   - Staging then uses its own first-run and deploy tokens, scoped to the staging Worker
     (`pylota-identity-staging`) and the staging zones, with the per-Worker Editor role, as production's are.
4. **SES access from GitHub Actions uses OIDC, not stored keys.** An IAM OIDC identity provider for
   `https://token.actions.githubusercontent.com` with audience `sts.amazonaws.com`, and one IAM role per GitHub
   environment, trusted only for `repo:PILOTAAI/pylota-identity:environment:staging` (and `…:environment:production`
   for production's setup jobs) through the `sub` condition, as GitHub's guide recommends (GitHub,
   "Configuring OpenID Connect in Amazon Web Services", read 2026-10-27). Workflows request `id-token: write`
   and assume the role with `aws-actions/configure-aws-credentials` pinned to a full commit SHA. Spikes S8 and
   S11 run as a `workflow_dispatch` job in environment `staging`, so M1 needs no AWS key anywhere. The
   Actions secrets `STAGING_AWS_ACCESS_KEY_ID` and `STAGING_AWS_SECRET_ACCESS_KEY` are replaced by the variable
   `STAGING_AWS_ROLE_ARN`.
5. **The running Worker keeps an access key, rotated on a schedule.** A Worker has no identity that AWS can
   federate: a search of Cloudflare's documentation index (`developers.cloudflare.com/llms.txt`) for "OIDC",
   "workload identity" and "federation" returned no Workers feature on 2026-10-27 (a negative result from a
   search; re-check before relying on it). So the Worker keeps the IAM user `pylota-identity-worker-{dep}` that
   `idvy setup ses` creates, with its policy limited to sending through the deployment's configuration set and
   reading the receiving bucket, and:
   - `idvy setup ses --rotate-key` creates a second access key, writes it to the Worker's secrets, waits for a
     successful SES call with it, then deletes the old key (an IAM user holds at most two keys);
   - a scheduled GitHub Actions job (`ses-key-rotate.yml`, OIDC, environment `production`) runs the rotation
     every 60 days, and the doctor's `ses` check warns when the key is older than 80 days, so a missed run is
     seen before the 90-day mark.

## Consequences

- No AWS secret is stored in GitHub, in this cloud environment or on a laptop for CI and the spikes; the only
  long-lived AWS secret is the Worker's own, and it rotates without a person.
- **Accepted risk, staging in the production account.** Several Cloudflare permissions are account-wide (D1,
  R2, Queues, Vectorize and Workers AI Edit, and Workers Admin at product scope for the first-run and spike
  tokens). Any of those tokens can change any database, bucket, queue or Worker in the account, Pylota's
  included. Mitigations: the deployment name keeps every resource separate; tokens expire within 14 days
  (spikes) or are deleted after use (first-run); zone permissions name only the staging zones; the owner
  reviews the account's audit log after M1 and after each staging setup. The owner accepted this on
  2026-10-27.
- Staging and production share the account's limits (Workers subrequests and CPU, the Email Sending daily
  quota, `PM_DAILY_SEND_QUOTA`); staging's live suite counts against the same quota.
- Every page that names a resource keeps the default names; this ADR and [CLI and setup § 6.1](../design/cli.md#61-flags)
  define how a named deployment derives its own.

## Alternatives considered

- **A separate staging account** (the earlier plan). Cleaner isolation, but it pays for Workers Paid and usage
  outside the credits. Not chosen.
- **Long-lived AWS keys for CI and the spikes, rotated by hand.** Simpler to set up, but a stored secret that
  can leak and that someone must remember to rotate. Not chosen.
