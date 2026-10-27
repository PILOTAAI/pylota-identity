# S12 SMTP from a Worker: result

**Pass**, 2026-10-27. `smtp_relay` ships in v1.0.

The Worker `pi-spike-12` (S1's spike Worker, routes `/s12/*`) opened TCP sockets with the runtime's
`connect()` and drove an SMTP client written for the spike: one connection per message, `EHLO`,
`STARTTLS` on 587, `AUTH PLAIN`, `MAIL FROM`, `RCPT TO`, `DATA` with dot-stuffing, the final `.`, `QUIT`;
10 s to connect and 30 s per command. Credentials came in the request and never appear in a transcript.
The streams are read through the runtime's `ReadableStream`/`WritableStream`, because `worker::Socket`
implements tokio's I/O traits, which `platform` must not depend on.

| Provider | 465 (TLS from the start) | 587 (`STARTTLS`) |
|---|---|---|
| A: Gmail (`smtp.gmail.com`, app password) | `235 2.7.0 Accepted`; a message to B accepted (`250 2.0.0 OK`) | `AUTH` offered only after `STARTTLS`; `235`; a message to B accepted |
| B: AgentMail (`smtp.agentmail.to`, API key) | `235 Authentication successful`; a message to A accepted (`250 Message queued as <…amazonses.com>`) | `235`; a message to A accepted |

| Check | Result |
|---|---|
| Wrong password (Gmail, 587) | `535-5.7.8 Username and Password not accepted` → `smtp_auth_failed` |
| Certificate for another host, 465: the alias `pi-spike-12-smtp.pylotamail.com` (CNAME to `smtp.gmail.com`, so a valid certificate for the wrong name) | Refused before any byte (`proxy request failed, cannot connect to the specified address`); the real name connects |
| The same alias on 587 | `STARTTLS` answered `220`; the upgraded socket's `opened` **resolved**, and the next read failed (`connection closed`) before `AUTH` was written. TLS is established only once the second `EHLO` is answered |
| Connect timeout | `www.google.com:81` (dropped): `timeout after 10000 ms` |
| Port 25 | `Connections to port 25 are prohibited` |
| Dot-stuffing | A body line starting `.` arrived at B with one dot |
| `Message-ID` through the relay | Gmail kept the composed `<s12-…@pylota-spike.example>`; AgentMail stored it but sent through SES, which replaces it |

## What this does not cover

- The **uncertain window** (the final `.` written and no reply within 60 s): neither provider can be made to
  withhold its reply. The timer is the same one that produced the connect timeout above; the classification
  (`Unknown`, never resent) is `core::smtp` logic, unit-tested in M23 against a scripted server.
- The six-connections-per-invocation limit was not exercised.
- A host-name mismatch, an unreachable host and a refused connection fail with the same runtime message, so
  the client cannot report which one happened.

## Cleanup

The Worker and the alias were removed on 2026-10-27. The AgentMail API key used here should now be revoked
(owner).
