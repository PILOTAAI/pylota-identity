# AWS policies for `idvy setup ses`

These are the IAM documents for the AWS account a deployment uses for SES. Replace `ACCOUNT_ID` with the
12-digit account ID before pasting them. The region is `eu-west-2`; change every ARN and
`aws:RequestedRegion` value together for another region. The resource types were read from the AWS Service
Authorization Reference (SES v1 and v2, S3, SNS, SQS, IAM) on 2026-10-27. The trust policy follows GitHub's
"OpenID Connect reference" page, read the same day.

| File | Attach to | What it allows |
|---|---|---|
| `github-oidc-trust-policy.json` | The role's trust relationship | Only GitHub Actions jobs in this repository's `staging` environment. For production, change `environment:staging` to `environment:production` |
| `setup-ses-policy.json` | The role (staging and production) | `idvy setup ses`, `setup ses --rotate-key`, `destroy --include-ses` and the doctor's `ses` check, on the default resource names only |
| `spikes-policy.json` | The staging role only | Spikes S8 and S11, on `pi-spike-*` resources only. Remove it when M1's gate is met |
| `worker-permissions-boundary.json` | The managed policy `pylota-identity-worker-boundary` | The most the Worker's IAM user can ever do. Setup creates that user only with this boundary |

## Why the boundary

The setup role can create an IAM user and give it an inline policy. Without a limit, a stolen job token
could create a user with any permission and keep its key. The setup policy allows `iam:CreateUser` and
`iam:PutUserPolicy` only when the user carries the boundary `pylota-identity-worker-boundary`. The role
cannot remove, replace or edit the boundary. So the Worker's user can never do more than the boundary
allows, whatever inline policy it is given.

## What stays on `*`

The SES v1 receipt-rule actions (`DescribeActiveReceiptRuleSet`, `DescribeReceiptRuleSet`,
`DescribeReceiptRule`, `CreateReceiptRuleSet`, `SetActiveReceiptRuleSet`, `CreateReceiptRule`,
`UpdateReceiptRule`, `DeleteReceiptRule` and `DeleteReceiptRuleSet`) list no resource type, and neither
do `ses:GetAccount` and `ses:ListEmailIdentities`. They are granted on `*`, limited to `eu-west-2`. A
holder of the role can therefore change any receipt rule in that region. This is one reason each
deployment gets its own AWS account (see [Domains on any DNS host §4.2](../../docs/src/project/design/domain-connections.md#42-deployment-set-up-for-ses)).

SES identities are granted on `identity/*`, because tenant domains have no common prefix. The account
holds no identities but the deployment's own.

## Console steps (once per account)

1. **IAM → Identity providers → Add provider.** Choose OpenID Connect, with provider URL
   `https://token.actions.githubusercontent.com` and audience `sts.amazonaws.com`.
2. **IAM → Policies → Create policy → JSON.** Paste `worker-permissions-boundary.json` and name the policy
   `pylota-identity-worker-boundary`.
3. **IAM → Roles → Create role → Custom trust policy.** Paste `github-oidc-trust-policy.json`. Do not
   attach a managed policy. Name the role `pylota-identity-github-staging`, or `…-production` for
   production.
4. **On the role, Add permissions → Create inline policy → JSON.** Paste `setup-ses-policy.json` and name
   it `setup-ses`. On the staging role only, add `spikes-policy.json` as a second inline policy named
   `spikes`.
5. **GitHub → Settings → Environments → `staging` → Variables.** Add `STAGING_AWS_ROLE_ARN` with the role's
   ARN. For the production role, add `PROD_AWS_ROLE_ARN` in the `production` environment.

The `sub` value names the organisation and repository with their numeric IDs
(`PILOTAAI@281272467/pylota-identity@1412185299`). GitHub uses this immutable format for every
repository created after 15 July 2026, and this one was created on 2026-10-09. A trust policy that names
only `PILOTAAI/pylota-identity` would never match.
