Where this goes: `docs/src/reference/cli.md`. A new top-level section "## Vault" after the phone section (or after "## Identities" … the last resource section before "## Platform operations"), and a row for `vault_redeem` in "### `ops switch`". Source: [Vault § 18.2](../project/design/vault.md#182-cli).

## Vault

Every command is a client of the [vault routes](api.md#vault). Values are read only from standard input or a
file, never from an argument, so they never reach shell history; no command prints a value, and an unlock key
or a ticket is printed once, to standard output only. There is no `reveal` command.

### `vault status`

```bash
idvy vault status --tenant brightwell
```

Custody, the tenant key's `kid`, counts of items, grants and pending uses, and `retiring_versions`.

### `vault items list`, `get`, `create`, `update`, `archive`, `restore`, `delete`

```bash
idvy vault items create --identity bookings.brightwell@pylotamail.com --kind login \
  --title "TfL road user charging" --account sac_01JA… --origin https://tfl.gov.uk \
  --username compliance.brightwell@pylotamail.com --password-stdin --approval per_run < tfl-password.txt
idvy vault items create --identity … --kind api_key --title "Supplier API" --origin https://api.supplier.example \
  --inject bearer --secret-stdin < key.txt
idvy vault items create --identity … --kind session --title "Supplier portal" --origin https://portal.supplier.example \
  --session-file storage-state.json --ttl-days 7
idvy vault items delete vit_01JAD4…        # asks for the item's title to be typed back unless --yes
```

`--password-stdin`, `--secret-stdin`, `--totp-uri-file`, `--recovery-codes-file`, `--session-file` and
`--generate [--length N]` supply values. `update` changes metadata only.

### `vault items rotate`

```bash
idvy vault items rotate vit_01JAD4… --password-stdin --overlap-hours 24 < new-password.txt
```

### `vault grants list`, `request`, `approve`, `deny`, `revoke`

```bash
idvy vault grants request --identity … --item vit_01JAD4… --operation fill --purpose "Check PCN status each morning."
idvy vault grants approve vgr_01JAD5… --expires-in 7d --on-behalf-of usr_8812
```

`approve` shows the grant's facts (item, sites, operations, identity, purpose as quoted text) and asks for
confirmation unless `--yes`.

### `vault uses list`, `get`, `approve`, `deny`, `cancel`

```bash
idvy vault uses approve vus_01JAD6… --digest c41a…      # prompts with the digest shown when --digest is omitted
```

### `vault consumers list`, `register`, `activate`, `suspend`, `resume`, `rotate-keys`, `revoke`

```bash
idvy vault consumers register --tenant brightwell --kind browser --name "Browser fleet" \
  --hosting self_hosted --modes cdp --keys ./consumer-keys
idvy vault consumers activate vcn_01JAC2… --note "Reviewed settings and conformance report 2026-11-02"
```

### `vault consumer-keys generate`

```bash
idvy vault consumer-keys generate --out ./consumer-keys
```

Local only: an Ed25519 signing key and an X25519 encryption key, written with mode `0600`; prints the two public
JWKs.

### `vault consumer-check`

Signs `GET /v1/vault/consumer/self` with the keys in a directory and prints the answer, proving the keys and the
clock before activation.

### `vault conformance`

```bash
idvy vault conformance --consumer ./my-consumer.toml [--fixture proxy_second_form]
```

Runs the fixture server and the consumer under test against it, and prints a report. Exit 0 only when every
fixture passes.

### `vault custody enrol`, `release`

```bash
idvy vault custody enrol --tenant brightwell --slots 2      # partner key; prints two unlock keys once
idvy vault custody release --tenant brightwell --unlock-stdin < unlock.txt
```

### `vault rotate-key`, `vault revoke-all`, `vault audit`

```bash
idvy vault rotate-key --tenant brightwell
idvy vault revoke-all --tenant brightwell --shred-sessions     # asks for the tenant's slug to be typed back
idvy vault audit --tenant brightwell --verify                  # exit 1 on a broken chain
```

## Row for "### `ops switch`"

| `vault_redeem` | `on` (default) or `off`: with `off`, vault redemption, write-back and ticket minting refuse; reads and approvals continue ([Vault § 15](../project/design/vault.md#15-kill-switches-and-incident-response)) |
