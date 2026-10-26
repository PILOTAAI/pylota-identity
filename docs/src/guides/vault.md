# Vault

This guide is for integrators and for agents. It covers keeping logins, API keys and browser sessions in the
vault, letting an agent use them without ever seeing them, approvals, consumers, one-time codes, sessions,
rotation and incidents. The binding design is [Agent credential vault](../project/design/vault.md).

The vault is off unless the deployment sets `PM_VAULT = "on"` and the platform sets the tenant's
`vault.allowed: true`. On Pylota Identity Cloud it needs a paid plan, or a partner that pays for its tenants.

## Who can see a value

Pylota Identity Cloud stores vault values encrypted with keys it holds. Values are decrypted only inside
the Pylota Identity Worker, only to fill, compute a code, sign or seal a packet for a consumer you
registered, and never returned to an agent, a model or an API key. Cloudflare, which runs the Worker, and
TREFT LTD, which operates Pylota Identity Cloud, are inside this trust boundary. The vault is not
zero-knowledge and not end-to-end encrypted. A partner can hold an extra unlock key so that a copy of the
database and the deployment's master key is not enough to decrypt its tenants' values.

| Who | Can get a value |
|---|---|
| An agent, a model, an MCP client, any API key | Never. No route, tool, event or log returns one |
| A consumer you registered | Only the fields of one phase of one ticket it holds, encrypted to its own key |
| A workspace owner or admin | In the console only, after signing in again, at most 10 times a day; never a TOTP seed or session cookies |
| The operator and Cloudflare | Yes, by running code that decrypts. With a partner unlock key, only while the partner presents it |

## How a value gets used

```text
agent ──vault_request_grant──▶ person approves the grant
orchestrator (your server) ──POST …/vault/uses──▶ person approves the use, if the item needs it
orchestrator ──ticket──▶ your consumer ──signed redeem──▶ packet sealed to the consumer's key
consumer fills at the exact site, reports, writes back a refreshed session
```

Three different credentials take part, and no one of them is enough: the agent's identity key asks, your
server's key starts the use, and your consumer's own key receives the packet.

| An agent can | A person decides | Your server (orchestrator) does |
|---|---|---|
| List the identity's items and their health; ask for a grant; read grants and uses; give up its own grant | Every grant; every use the item's approval level asks for; making an approval level looser | Create uses when its own code sees a sign-in form, a code field or a session start; mint tickets; pass each ticket to the consumer |

An agent's identity key can never hold `vault:use`, `vault:approve` or `vault:admin`, so it can never start a use
or approve anything, whatever permissions anyone tries to give it.

The MCP tools are `vault_list_items`, `vault_get_item`, `vault_request_grant`, `vault_list_grants`,
`vault_get_use` and (P1) `vault_sign`. None returns a value, and none approves.

## Before the first item: the service ledger

The vault uses a credential only at a service a person approved for that identity. Every login is tied to an
approved entry of the [service sign-up ledger](agents.md), and each of its sites must belong to the entry's
service domain (or one of up to five `login_domains`, for services that sign in on another domain).

The simplest path is to let the vault create the login when a person approves the account:

```bash
curl -X POST https://mail.example.com/v1/identities/idn_01J9Z3K8V4/accounts \
  -H "Authorization: Bearer $PYLOTA_IDENTITY_KEY" \
  -H "Content-Type: application/json" \
  -d '{ "service_domain": "example-insurer.co.uk",
        "account_identifier": "compliance.brightwell@pylotamail.com",
        "purpose": "Download policy documents for fleet renewals.",
        "vault_login": { "origins": ["https://portal.example-insurer.co.uk"],
                         "password": { "generate": {} }, "email_otp": true, "approval": "per_run" } }'
```

When a person approves the entry, the vault generates the password, creates the login and grants the agent
`fill` on those sites for 90 days. The agent never holds the password it signs up with.

## Add an item yourself

Values are write-only: you give them once and never read them back through the API.

```bash
idvy vault items create --identity brightwell-bookings --kind login \
  --title "TfL road user charging" --account sac_01JA… \
  --origin https://tfl.gov.uk --username compliance.brightwell@pylotamail.com \
  --password-stdin --totp-uri-file ./tfl-totp.txt --approval per_run < ./tfl-password.txt
```

| Kind | Holds | Used by |
|---|---|---|
| `login` | Username, password, optional TOTP seed and recovery codes | `fill` (a browser consumer, or an egress proxy by placeholder) |
| `api_key` | A secret and how to send it (`bearer`, `basic` or a named header) | `inject` (an executor or egress proxy), and P1 `sign` |
| `session` | Cookies and local storage for exact sites, with a time to live | `session_load`, `session_capture` |

Sites are exact `https` origins: no wildcards, no paths, and `www.example.com` is not `example.com`. The vault
refuses payment cards and bank details (even a card number stored as a "password"), passkeys, and
bot-challenge clearance cookies such as `cf_clearance`, which it drops from every session.

## Grants and approval levels

An agent asks for a grant; a person approves it with an expiry (1 hour to 90 days, 7 days by default):

```json
{ "item_id": "vit_01JAD4…", "operations": ["fill"], "origins": ["https://tfl.gov.uk"],
  "purpose": "Check PCN status each morning." }
```

Each item also has an approval level for each use:

| Level | A use needs a person |
|---|---|
| `grant` | Never: the grant is the approval. For low-risk, unattended reads |
| `per_run` | Once per run of your agent, then for 60 minutes in that run |
| `per_use` | Every time (the default) |

`rotate_fill` and `session_capture` always need a person. The tenant's `vault.min_approval` can raise the floor
for every item. Making a level looser is a decision for a person.

**Approval screens.** Show the person the item, the site, the operation, the consumer, the run and the purpose
as quoted text. Never turn anything an agent wrote into a link: an agent steered by what it read can put a
look-alike sign-in page in front of the person whose approval it needs.

## The orchestrator

Your server creates uses. Follow these four rules; they are the ones the vault cannot check for you:

1. Create a use only when your own code sees the trigger in the browser or tool it runs (a password field on
   an approved site, a code field, a session start), never because a model, a message, a page or an SMS asked.
2. Pass a ticket only to the consumer named in the use, over an authenticated internal channel. Never put it in
   a model's context, a prompt, a log or a tool result.
3. Pick the item from the grants of the identity whose run it is, never from text a model produced.
4. Show approvers the facts above, and no agent-written links.

```bash
curl -X POST https://mail.example.com/v1/identities/idn_01J9Z3K8V4/vault/uses \
  -H "Authorization: Bearer $PYLOTA_IDENTITY_SERVER_KEY" \
  -H "Idempotency-Key: run_8f2c41:tfl-login" \
  -H "Content-Type: application/json" \
  -d '{ "item_id": "vit_01JAD4…", "operation": "fill", "phases": ["credentials", "email_otp"],
        "consumer_id": "vcn_01JAC2…", "run_id": "run_8f2c41", "origin": "https://tfl.gov.uk",
        "trigger": "login_form", "purpose": "Check PCN status for WM12ABC" }'
```

If no person is needed, the answer carries the ticket (`vtk_…`), to be redeemed within 60 seconds. Otherwise the
use is `pending_approval`; wait for `vault.use.approved` and mint a ticket with
`POST /v1/vault/uses/{use_id}/ticket`. A replay with the same idempotency key never returns a ticket again:
mint a fresh one.

## Consumers

A consumer is the component that receives values: your browser supervisor (`browser`), your egress proxy
(`egress_proxy`) or a server process that calls APIs itself (`executor`). Its private keys stay in that
component, outside the browser, the agent and every model.

```bash
idvy vault consumer-keys generate --out ./consumer-keys     # Ed25519 to sign, X25519 to receive
idvy vault consumers register --tenant brightwell --kind browser --name "Browser fleet" \
  --hosting self_hosted --keys ./consumer-keys
idvy vault consumer-check --key ./consumer-keys            # proves keys and clock
idvy vault conformance --consumer ./consumer-keys          # the fixtures every consumer passes
```

A person (or the platform, for a hosted browser) activates it after review. Every request it makes is signed
(RFC 9421, Ed25519, with a nonce) and every packet is sealed to its own key.

What a consumer must do is the consumer contract `vault-consumer/1` (C1–C21, in the
[design](../project/design/vault.md#85-the-consumer-contract)). The parts integrators most often get wrong:

- **Fill only on the right page.** Check the top-level origin and the frame from the browser, never from page
  content. Never fill a frame from another origin. Turn off session recording, logging and CAPTCHA solving on a
  hosted browser (for Browserbase: `recordSession`, `logSession` and `solveCaptchas` all `false`).
- **Keep the model out of the fill.** No screenshot, DOM read or model step for that tab from the redemption
  until 30 seconds after the fill; mask password and code fields in every frame you record. Afterwards, search
  the page for the value and report `value_exposed` if it is there.
- **Proxy mode.** The browser types a placeholder (`pidv1-…`); the proxy swaps in the value only in the
  recorded field of one `POST` over `https` to the recorded path on the ticket's site, once. Anything else
  carrying a placeholder is dropped and reported `placeholder_misplaced`: another site, plain `http`, another
  path or field, a header, a URL, a WebSocket frame, a compressed body.
- **Secrets.** Put an `inject` secret only in the header its scheme names, only over `https`, only to the
  ticket's site. Never follow redirects on that request. Strip the secret from every response, including
  compressed bodies, values split across chunks, and headers such as `Location`, before a model or a log sees
  the response.
- **Notice closure.** If you hold a value across requests, check `GET /v1/vault/consumer/uses/{use_id}` at least
  every 30 seconds; when it says `closed`, drop the value and close every connection that carries it.

## One-time codes

- **TOTP.** The vault computes the code from the stored seed and puts it in the packet. Two runs never get the
  same code.
- **Email codes.** For a login with an email channel, verification mail from the service is hidden from every
  agent surface and every API key, and is delivered only into an open `email_otp` phase, once. Set
  `vault.codes_into_fills_only: true` to do this for every verification email the tenant receives.
- **SMS codes.** The same, for a login with a phone channel on one of the identity's
  [phone numbers](phone.md#verification-codes-by-sms), only into a use created in the last 10 minutes. SMS
  senders cannot be authenticated, so an unsolicited code is never used.

## Sessions

A session item holds cookies and local storage for its sites only, at most 1 MiB, with a time to live of 1
hour to 30 days. One run loads it at a time. At the end of the run the consumer writes back what it has; the
vault keeps only the new version. When a site signs the agent out, report `session_rejected`: the item becomes
`needs_auth` and appears in "Needs a person", and a person signs in again through your hand-off, approving a
`session_capture`.

Deleting a session item does not sign it out at the site. If you suspect a leak, use the site's own "sign out
other sessions".

## Rotation

- **Sessions** rotate on every write-back.
- **Logins**: change the password at the site, then `idvy vault items rotate`; or (P1) let `rotate_fill` change
  it at the site with a person's approval.
- **API keys**: rotate by hand with an overlap (`--overlap-hours`), or let your executor rotate after use where
  the service supports it and write the new secret back.
- A value reported exposed blocks the item until a new version is written.

## Incidents

| Suspected | Do |
|---|---|
| A value seen by a model or in a log | Report `value_exposed` (or mark the item `needs_rotation`), change the value at the site, then rotate it in the vault |
| A consumer's keys | Suspend the consumer, rotate its keys, treat what it redeemed since then as exposed |
| Your orchestrator's key | Revoke it; uses still need grants, approvals and a consumer |
| Everything | `idvy vault revoke-all --tenant T --shred-sessions`, then rotate each value at its service |

Every change is in the tenant's hash-chained audit (`idvy vault audit --verify`), and every event carries the
chain head, so a copy of your events lets you detect a rewritten history.

## Integrator checklist

- Give agents identity keys with `vault:read` and `vault:request`; keep `vault:use`, `vault:approve` and
  `vault:admin` on your server's keys.
- Tie every site to an approved ledger entry; use `vault_login` for new accounts.
- Follow the four orchestrator rules; never let a ticket reach a model.
- Register consumers with keys that live only in them; pass the conformance fixtures before activation and after
  each release.
- Show approvers quoted text, never agent-written links; pass `on_behalf_of` when your server approves.
- Handle `placeholder_misplaced`, `value_exposed` and `needs_auth` by routing them to a person.
- Treat every `title`, `purpose`, `note` and `health_reason` as untrusted text.
