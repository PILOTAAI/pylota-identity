# S8 SES in wasm: result

**Pass**, 2026-10-27. SES stays in v1.0.

Code: `core::ses::sigv4` and `core::sns`, called from the spike Worker `spikes/s8-s11-ses` running in workerd
(`wrangler dev --local`) on a GitHub Actions runner with the staging role's temporary credentials (OIDC,
[ADR 0023](../../docs/src/project/adr/0023-staging-in-the-existing-account-and-ses-through-oidc.md)), by the
workflow `spikes-ses.yml` (run 7, <https://github.com/PILOTAAI/pylota-identity/actions/runs/40596207014>).
The same run is S11's. The spike Worker, its `run.sh` and the `spikes-ses.yml` workflow were removed at
build plan M4, once the platform crate replaced the spike helpers they used; they are in the history at
commit `84a76b2`.

| Check | Result |
|---|---|
| SigV4 against AWS's published test suite (`get-vanilla`, `get-vanilla-with-session-token`, `post-x-www-form-urlencoded`) | Unit test `core::ses::tests::sigv4_vectors` passes |
| A real send: SES v2 `POST /v2/email/outbound-emails` with `Content.Raw`, signed with `x-amz-security-token` (temporary credentials), from wasm in `eu-west-2` | `200` with a `MessageId` (`010b01a17f858bb4-…`); the message arrived (S11) |
| A 39,000,480-byte raw message the same way | `200`; it arrived whole (S11) |
| SNS `SignatureVersion` 2 verification of real notifications, the certificate fetched from `SigningCertURL` (`https://sns.eu-west-2.amazonaws.com/SimpleNotificationService-1e59c4574facfe41babdb2d652f8ebef.pem`) | All four verified (`Ok`) |
| The same notification with one byte of `Message` changed | Refused: `BadSignature` |
| The same notification claiming `SignatureVersion` 1 | Refused: `Version("1")` |
| A wrong certificate host, a wrong topic and a stale timestamp | Refused in `core::sns::precheck` (unit tests) |

The S3 `GetObject` and SQS `ReceiveMessage`/`DeleteMessage` calls of S11 signed with the same `sigv4`
(services `s3` and `sqs`), so one signer serves every AWS call.

Not exercised: where SES puts its error type (`x-amzn-ErrorType` or the body) on a failed send, because no
send failed; the transport reads both, as [Outbound › Amazon SES](../../docs/src/project/design/outbound.md#amazon-ses)
says, and its first real error is checked in M9. The real notification used here was not kept as a test
fixture; `core::sns::verify_v2_vectors` gets one captured from staging in M23.
