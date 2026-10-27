# S4 Wasm budget: result

**Budget: pass. Verdict parity: not run**, blocked on the `mail-auth` dependency (below). 2026-10-27.

The Worker `pi-spike-4` (this crate, through `platform::export_worker!`) links the crates inbound mail uses:
`mail-parser`, `mail-builder`, `ammonia`, `sha2`, `rsa` and `ed25519-dalek`. Each `POST /parse` parses the
message, walks every part, decodes every attachment, sanitises the HTML body with `ammonia`, hashes the whole
message with SHA-256 (standing in for the DKIM body hash, the dominant cost of verifying a large message) and
builds a short reply. CPU and wall time are the runtime's own (`wrangler tail`); memory is the wasm linear
memory after the request, which only grows, so it bounds the peak.

| Measure | Budget | Result |
|---|---|---|
| Compressed bundle | ≤ 10 MiB | 0.58 MiB gzip (1.55 MB wasm, 24 KB JS); Wrangler reports 608 KiB |
| Cold start | < 1 s | Startup 5–9 ms (Wrangler); first request after a deploy: 12 ms CPU, 12 ms wall |
| 25 MiB message (26,246,801 bytes, 8 parts, 5 base64 attachments) | < 128 MB, inside the CPU limit | 2,406 ms CPU, 2,992 ms wall; linear memory 48 MiB |
| 40 MB message (40,093,087 bytes, 10 parts, 7 attachments), first | same | 2,132 ms CPU, 3,390 ms wall; 71 MiB |
| 40 MB message, warm isolate | same | 442 ms CPU, 1,235 ms wall; 71 MiB |
| 4.8 MB HTML body through `ammonia` (to 3.2 MB) | – | 771 ms CPU |

All inside the Workers paid CPU limit (30 s by default) and under 128 MB.

## Findings

- **`mail-builder` 1.0.0 panics on wasm32 when it reads the clock**, as [Rust workspace §3](../../docs/src/project/design/rust-workspace.md#3-workspace-dependencies)
  predicted: a reply built without an explicit `Date` and `Message-ID` made every request fail, and the
  runtime reported the panic as `canceled … your Worker's code had hung`, not as a panic. With both set the
  build succeeds. The design's rule (always set `Date`, `Message-ID` and boundaries) is confirmed; a panic in
  wasm must be looked for under that message.
- **`mail-auth` 0.13.3 with `dns-doh` does not resolve**, so DKIM and DMARC verdict parity could not be run.
  `dns-doh` asks for `reqwest ^0.13` with the feature `webpki-roots`, which no `reqwest` 0.13 release has;
  0.14.0 and 0.14.1 are the same, and without a DNS feature the crate does not compile. [Rust workspace §3](../../docs/src/project/design/rust-workspace.md#3-workspace-dependencies)
  now records the fix M2 takes (a patched copy under `vendor/mail-auth`), and the parity check runs there
  against the corpus.
- **The `ammonia` pin was vulnerable.** Linking it here made `cargo deny` fail on 4.2.0 with RUSTSEC-2021-0157,
  RUSTSEC-2021-0158 and RUSTSEC-2026-0373 (CSS `url()` escaping, an SVG-animation XSS). The workspace now pins
  4.2.4, which fixes all three and has the same dependencies; [Rust workspace §3](../../docs/src/project/design/rust-workspace.md#3-workspace-dependencies)
  records why the two-week rule gives way.
