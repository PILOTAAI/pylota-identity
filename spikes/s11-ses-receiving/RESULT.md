# S11 SES receiving: result

**Pass**, 2026-10-27, in `eu-west-2`. `dns_records` and `smtp_relay` with `inbound: ses` stay in v1.0.

Run 7 of `spikes-ses.yml` (<https://github.com/PILOTAAI/pylota-identity/actions/runs/40596207014>), driven by
`spikes/s8-s11-ses/run.sh` with the staging role. `setup` created the SES identity
`pi-spike-11.pylotamail.com`, a private bucket, the topic `pi-spike-11-inbound` (`SignatureVersion` 2) with
the SQS backstop `pi-spike-11-backstop` subscribed, and the active rule set `pi-spike-11`; the DKIM CNAMEs and
the MX `10 inbound-smtp.eu-west-2.amazonaws.com` were published in Cloudflare; `exercise` sent from
`s8@pi-spike-11.pylotamail.com` through SES (S8), read the notifications and fetched the objects from the spike
Worker in workerd, then removed everything.

| Check | Result |
|---|---|
| Rule `pm-deliver` (no recipient condition, `ScanEnabled`, `TlsPolicy: Optional`, one S3 action with prefix `in/` and the topic) | Every message stored and notified, 42 s after the first send for the whole set |
| Notification shape | `notificationType: "Received"`; `receipt.action = { type: "S3", topicArn, bucketName, objectKeyPrefix: "in/", objectKey }`; `receipt.recipients`; `spamVerdict`, `virusVerdict`, `spfVerdict`, `dkimVerdict`, `dmarcVerdict`; `mail.commonHeaders`. No content in the notification |
| `objectKey` form | `in/` + `mail.messageId` (for example `in/iadgi2gtibvdli3fgmindupk33sm956q8t4clt01`), as [Inbound](../../docs/src/project/design/inbound.md) assumes |
| S3 `GetObject` with SigV4 from the Worker | `200`, the whole message; for the 39 MB message 39,003,877 bytes in 2.5 s |
| A 39 MB message ([N5](../../docs/src/project/edge-cases.md)) | Sent and received whole |
| `user+tag@` routing | `receipt.recipients = ["user+tag@pi-spike-11.pylotamail.com"]`: the rule without a recipient condition takes it, with the tag |
| The retired-address bounce: a rule for `retired@` with a Bounce action (`550`, `5.1.1`) and Stop, ahead of `pm-deliver` | The sender got a DSN, `Diagnostic-Code: smtp; 550 5.1.1 This address no longer accepts mail`, `Status: 5.1.1`; nothing was stored for the retired address |
| The backstop | The Worker read every notification from SQS (`ReceiveMessage`, then `DeleteMessage`) and each verified. No HTTPS subscription existed in this run, so every notification reached the Worker only through the backstop, the case of a push that never arrives |

## Findings

- **SES writes a test object when an S3 rule is created**: `in/AMAZON_SES_SETUP_NOTIFICATION`, notified like
  mail, to `recipient@example.com` from `no-reply-aws@amazon.com`. [Inbound](../../docs/src/project/design/inbound.md)
  now acknowledges and deletes it without ingesting anything.
- **A Bounce action needs a verified sender**: creating the retired-address rule before the domain verified
  failed with `Identity is not verified: mailer-daemon@…`. The design creates those rules from the Worker after
  setup has verified the platform identity (step 9), so its order already holds.
- **The staging role**: four runs failed before this one on the role's setup (the variable stored as a secret,
  then each inline policy missing in turn); each failure named the missing piece. `deploy/aws/README.md`'s steps
  stand; both inline policies must be attached.
- DMARC for the spike's own domain was `GRAY` (no DMARC record was published); it played no part.

## Cleanup

`run.sh exercise` ended with `cleanup: done`: rule set deactivated and deleted, `in/` emptied, bucket, queue,
topic and identity deleted. Its AWS calls run with errors ignored, so the owner may confirm in the console that
nothing named `pi-spike-11` is left. The four DNS records were deleted from `pylotamail.com`.
