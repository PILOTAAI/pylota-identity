# S13 Web Bot Auth format: result

**Pass**, 2026-10-27. Two runs, same answers.

`cargo run -p pylota-identity-spike-s13` signs a GET of `https://crawltest.com/cdn-cgi/web-bot-auth` with a fresh
Ed25519 key, through `core::httpsig::signature_base` and `core::jwk::thumbprint`. The headers follow
[Agent signing keys §5.2](../../docs/src/project/design/agent-keys.md#52-response):

- `Signature-Agent: "https://api.pylotamail.com"`, a structured-field string;
- `From`;
- `Signature-Input: sig1=("@authority" "signature-agent" "from");created;expires=created+60;keyid=<thumbprint>;alg="ed25519";nonce=<64 random bytes, base64>;tag="web-bot-auth"`;
- `Signature`.

| Request | Answer |
|---|---|
| Well-formed, unknown key (**the pass criterion**) | **401** |
| No signature headers (control) | 400 |
| `signature-agent` not in the covered components | 400 |
| No `tag` | 400 |
| `Signature-Agent` without the double quotes | 401 |
| `created` and `expires` in the past | 401 |
| Signature over a different base | 401 |

## What this does and does not prove

- The format the design specifies passes Cloudflare's format check. `PM_WEB_BOT_AUTH` may be turned on;
  the fallback is not needed.
- The endpoint answers 401 both for an unknown key and for a message that fails verification. It answers
  400 only for structural defects: a missing component or a missing tag. So 401 cannot show that the
  signature itself verifies. That is proven when a registered key gets 200, after the deployment's key
  directory is live (M25 on staging, M20).
- Cloudflare's page lists an unquoted `Signature-Agent` and an expired message as validation failures. Here
  both reached verification (401), so the format check alone does not catch them. `core::httpsig` and the
  §5.1 request rules must keep refusing them; they are not left to Cloudflare.

The RFC 9421 appendix B.2.6 vector and the RFC 8037 A.3 thumbprint vector pass as unit tests in
`crates/core` (`httpsig::tests::signature_base_rfc9421`, `jwk::tests::thumbprint_rfc8037_vector`), read
from rfc-editor.org on 2026-10-27.
