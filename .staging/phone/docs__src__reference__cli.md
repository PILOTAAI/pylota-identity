Where this goes: `docs/src/reference/cli.md`. (1) A new subsection of "## Deployment" after "### `setup ses`"; (2) a new section after "## Agent sign-up" (before "## Commands at a glance"); (3) a row in "## Commands at a glance". Source: [Phone numbers § 28](../project/design/phone-numbers.md#28-mcp-and-cli).

## 1. In "## Deployment", after "### `setup ses`"

### `setup phone`

Connects the deployment to its Telnyx account once. Reads the Telnyx API key from stdin, checks the account
level (a Verified-only read answered with 10038 means not Verified) and records it through
`POST /v1/platform/phone/probe`, reads the webhook public key you paste from the Telnyx portal, uploads
`PM_TELNYX_API_KEY` and `PM_TELNYX_PUBLIC_KEY` as Worker secrets, creates the account's notification profile
and a webhook channel to `https://{PM_API_HOST}/hooks/telnyx` for number orders, requirement groups, porting
orders and port-outs, and prints what remains manual (data locality, the reseller annex, the account level).
It writes nothing else to the account: tenant resources are created per tenant. `idvy doctor` re-checks every
item and the signature of a Telnyx test webhook.

```text
idvy setup phone [--account-id <telnyx-org-id>] [--api-base us|eu] [--profile <name>]
```

```bash
idvy setup phone --api-base eu < telnyx-key.txt
```

## 2. New section, after "## Agent sign-up"

## Phone numbers

Behind `PM_PHONE`. An agent's key buys inside the tenant's standing budget or asks; people decide the rest
([Phone numbers](../guides/phone.md)). Commands that can cost money take `--idempotency-key`; without it the CLI
derives one from the command's arguments and reuses it on a retry, as `send` does.

### `phone countries`

```text
idvy phone countries [--tenant <tenant>]
```

The catalogue rows with the tenant's standing (`eligible`, or the reason).

### `phone setup status` and `phone setup draft`

```text
idvy phone setup status [--tenant <tenant>] [--country <cc>]
idvy phone setup draft --country <cc> --file <draft.json> [--tenant <tenant>]
idvy phone setup draft --country <cc> --delete [--tenant <tenant>]
```

`status` prints each country's status, steps, fees, waits and next step. `draft` writes the agent's draft of a
registration's text (what the business does, 2–5 sample messages with the brand name, the opt-in flow, the
website, privacy and terms URLs); a person submits it in the console.

```bash
idvy phone setup status --tenant brightwell --country US
```

### `phone budget get` and `phone budget set`

```text
idvy phone budget get [--tenant <tenant>]
idvy phone budget set [--tenant <tenant>] [--enabled true|false] [--countries <cc,cc>] [--types <t,t>]
                       [--per-identity <n>] [--per-month <n>] [--max-setup <pounds>] [--max-monthly <pounds>]
                       [--expiring-only true|false] [--on-behalf-of <user>]
```

`set` reads the current budget, shows the change, and asks for confirmation when it loosens any field (pass
`--yes` in scripts). Loosening needs a platform key or the tenant's partner key where `phone.key_approval` is
`true`, with `--on-behalf-of`; otherwise `403 permission_denied` (`person_required`, exit 4) and a person saves
it in the console. Tightening needs `phone:manage`.

### `phone buy`

```text
idvy phone buy --country <cc> [--identity <identity>] [--type mobile|local|toll_free] [--area-code <nnn>]
                [--number <e164> --quote-token <token>] [--expires-at <rfc3339>] [--primary-for sms,voice]
                [--acknowledge-not-available] [--idempotency-key <key>]
```

Prints the number and its capability facets (exit 0) when the purchase was covered, or the request and who must
act (exit 0, with `status` and `next_step`) when it waits. `409 phone_number_taken` prints the alternatives
(exit 6).

```bash
idvy phone buy --country GB --identity bookings@brightwell
idvy phone buy --country US --area-code 512 --expires-at 2026-11-02T00:00:00Z
```

### `phone search`

```text
idvy phone search --country <cc> --type <type> [--contains <digits>] [--starts-with <digits>]
                   [--ends-with <digits>] [--locality <name>] [--region <name>] [--limit <1-20>] [--identity <identity>]
```

### `phone request` and `phone requests`

```text
idvy phone request number|port-in|release|route|emergency|a2p|toll-free|compliance [--identity <identity>] ...
idvy phone requests list|get|approve|reject|cancel|resolve [<request_id>] [--tenant <tenant>] [--status <s>]
                    [--digest <digest>] [--note <text>] [--on-behalf-of <user>] [--reason <text>]
```

`requests approve` without `--digest` reads the request, prints the quote (with `follow_on`) and asks for
confirmation; non-interactive use must pass `--digest`. `requests resolve` is a platform-key command.

### `phone numbers`

```text
idvy phone numbers list|get|update|move|lookup|adopt [<number>] [--identity <identity>] [--tenant <tenant>]
                   [--primary-for sms,voice] [--expires-at <rfc3339>|none] [--to-identity <identity>]
```

`adopt` is a platform-key command.

### `phone compliance`, `phone documents` and `phone export`

```text
idvy phone compliance get|update [--tenant <tenant>] [--legal-name ...] [--entity-type ...]
                      [--registration-country <cc>] [--company-number ...] [--company-registry <cc[:region]:name>]
                      [--use-case ...] [--representative-name ...] [--representative-role ...]
idvy phone documents upload <file> --kind <kind> [--valid-until <date>] [--tenant <tenant>]
idvy phone documents list|delete [<document_id>] [--tenant <tenant>]
idvy phone export <profile_id> --reason <text>
```

`documents upload` computes the size and SHA-256 locally and `PUT`s the file. `export` is a platform-key command.

### `phone registrations`

```text
idvy phone registrations [--tenant <tenant>]
idvy phone registrations otp <registration_id> --pin <code>   # a person, for a US sole-proprietor brand
idvy phone registrations otp-resend <registration_id>
```

### `phone connectors` and `phone route`

```text
idvy phone connectors create --kind vapi --region us|eu [--tenant <tenant>]   # reads the Vapi key from stdin
idvy phone connectors list|rotate|delete [<connector_id>] [--tenant <tenant>]
idvy phone route get <number>
```

### `phone sms`, `phone calls`, `phone consent`, `phone port-outs`

```text
idvy phone sms send --to <e164> --text <text> [--from <number>] [--kind transactional|marketing|verification]
                    [--dry-run] [--idempotency-key <key>] [--identity <identity>]
idvy phone sms list|get|wait|resolve|release [<sms_id>] [--identity <identity>] [--from <sender>] [--kind any|verification]
idvy phone calls create --to <e164> --purpose <text> [--assistant <id>] [--idempotency-key <key>]
idvy phone calls list|get [<call_id>]
idvy phone consent get|set --number <e164> [--state opted_out|opted_in] [--purpose marketing] [--evidence <text>]
idvy phone port-outs list|authorize|reject [<port_out_id>] [--reason-code <code>] [--reason <text>]
```

### `phone usage`, `phone charges`, `phone statement`, `phone reconcile`

```text
idvy phone usage [--identity <identity>] [--month YYYY-MM]
idvy phone charges [--tenant <tenant>] [--month YYYY-MM]
idvy phone statement --partner <partner_id> --month YYYY-MM
idvy phone reconcile [--tenant <tenant> | --platform]
```

## 3. Row for "## Commands at a glance" (after the `Usage` row)

```text
Phone        setup phone · phone countries · phone setup status|draft · phone budget get|set · phone buy · phone search
             phone request · phone requests list|get|approve|reject|cancel|resolve · phone numbers list|get|update|move|lookup|adopt
             phone compliance get|update · phone documents upload|list|delete · phone export · phone registrations
             phone connectors create|list|rotate|delete · phone route get · phone sms send|list|get|wait|resolve|release
             phone calls create|list|get · phone consent get|set · phone port-outs list|authorize|reject
             phone usage · phone charges · phone statement · phone reconcile
```
