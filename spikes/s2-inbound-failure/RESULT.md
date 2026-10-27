# S2 Inbound failure semantics: result

**Pass**, 2026-10-27. The design stands: a thrown handler is a temporary failure and the sender retries.

The Worker `pi-spike-2` ran S1's spike Worker (`spikes/s1-bindings`), whose email export accepts, calls
`setReject("spike s1 reject")` for addresses starting `reject`, and rejects its promise (`TempFail`, a thrown
handler) for addresses starting `tempfail`. `pi-spike-2.pylotamail.com` was enabled for Email Routing with
literal rules to the Worker. The sender was an AgentMail inbox, sending through its API; AgentMail delivers
through Amazon SES, whose bounce reports and delivery metrics show what the sending MTA saw. (This container
has no outbound SMTP, so no MTA could be driven from here directly.)

| Case | Runtime (trace) | What the sending MTA saw |
|---|---|---|
| Accept | outcome `ok` | Delivered (SES `Delivery`) |
| `setReject("spike s1 reject")` | outcome `ok` | Permanent failure, bounced at once: `Diagnostic-Code: smtp; 555 5.7.1 spike s1 reject. kuR4qmWuYjDu`, `Status: 5.7.1`. Cloudflare uses code 555 (not 550), adds a full stop and appends its own ID |
| Thrown handler (rejected promise) | outcome `exception` (`temporary failure`) | Temporary failure: SES retried, and the Worker received the message again 60.5 s later |
| Address with no rule yet (rule created about 3 minutes earlier) | no event | `550 5.1.1 Address does not exist.` and a permanent bounce |

`Authentication-Results`: Cloudflare stamps `Authentication-Results`, `ARC-Authentication-Results` (`i=1`) and
`Received-SPF`, each with the authserv-id **`mx.cloudflare.net`**, for example
`mx.cloudflare.net; dkim=pass header.d=agentmail.to …; dkim=pass header.d=amazonses.com …; dmarc=pass
header.from=agentmail.to policy.dmarc=reject; spf=pass (…)`. Setup's mail test writes this value to
`PM_TRUSTED_AUTHSERV_ID`.

## Findings

- **A new literal rule is not live at once, and mail sent before it is lost for good.** About three minutes
  after creation, mail to the rule's address was refused `550 5.1.1 Address does not exist.` (a permanent
  failure; the sender's provider then suppressed those addresses), and S1's first message, a minute after
  enabling routing, never arrived. Five minutes after creation, delivery worked. The design adds literal
  rules only on subdomain domains ([Identities and domains](../../docs/src/project/design/identity-domains.md));
  an address on such a domain must not be reported usable until its rule is live, and S9 measures how long
  that takes.
- The local `wrangler dev` endpoint answers `400` for `setReject` and `500` for a throw (S1); the deployed
  MX answers `555 5.7.1` and a temporary failure. Tests that use the local endpoint assert the local
  statuses only.

## Cleanup

The Worker, its rules, the subdomain's routing and the zone's routing were removed on 2026-10-27.
