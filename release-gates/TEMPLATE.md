# Release gate template

Copy this file to `v{version}.md` with the version bump that makes the final tag, and fill it in.
`cargo xtask release-gate --version {version}` refuses to promote the release until every item is
ticked (`- [x]`) with a link to its evidence, and until the facts it checks itself hold
([Security § 16](../docs/src/project/design/security.md#16-pre-release-checklist)).

candidate: v{version}-rc.N
fuzz-24h-commit: <the candidate's full commit hash>
live-manual: <YYYY-MM-DD of the cargo xtask live --manual run>

- [ ] This threat model reviewed against the code; every finding rated high fixed. ([evidence](<link>))
- [ ] An external penetration test completed before v1.0, findings rated high fixed and retested. ([evidence](<link>))
- [ ] `it::security::cross_tenant_matrix` and `it::security::route_table_complete` green (NFR-SEC-1 = 0). ([evidence](<link>))
- [ ] Every fuzz target clean for 24 cumulative hours on the candidate's commit. ([evidence](<link>))
- [ ] `cargo deny check`, `cargo audit` and CodeQL clean (no open high-severity finding). ([evidence](<link>))
- [ ] SBOMs of the Worker and the CLI (`cargo cyclonedx --format json`), signed `SHA256SUMS` with the trusted comment of the candidate's version, and attestations produced by the candidate's runs; a fresh-account `idvy deploy` rehearsal of the candidate verified them (PRD release criterion 6). The final tag's own run produces its artefacts again, and `release-gate` checks that they exist. ([evidence](<link>))
- [ ] The manual live run (`cargo xtask live --manual`) passed on staging with the candidate; its date is in the gate file. ([evidence](<link>))
- [ ] Secret rotation procedures in section 6.2 rehearsed on staging. ([evidence](<link>))
- [ ] `it::logs::i5_no_content_in_logs` green; the generated `wrangler.toml` has `invocation_logs = false` and traces disabled for production. ([evidence](<link>))
- [ ] Email preview disabled on every sending domain ([Privacy](privacy.md#3-jurisdiction-and-residency)). ([evidence](<link>))
- [ ] `security.txt` served on staging; `SECURITY.md` current. ([evidence](<link>))
- [ ] Signing-key rotation (`thread`, `link` and `cursor`) rehearsed on staging; old tokens verify until `verify_until`, and stop at once with `revoke_previous=true`. ([evidence](<link>))
- [ ] Identity-key rotation and revocation rehearsed on staging: a revoked key leaves the JWKS, and pausing an identity withdraws its JWKS. `PM_WEB_BOT_AUTH` is `on` only where spike S13 passed. ([evidence](<link>))
- [ ] With SES configured: both SNS topics have `SignatureVersion=2`, the inbound bucket blocks public access, and the IAM policy matches [Domains on any DNS host §4.2](domain-connections.md#42-deployment-set-up-for-ses). ([evidence](<link>))
- [ ] Release bundle checked for `/__test/` and the `itest-hooks` feature. ([evidence](<link>))
- [ ] The event feed's routes are in the cross-tenant matrix, and a revoked key's stream closes within 60 seconds (`it::stream::revocation_closes`). ([evidence](<link>))
- [ ] The npm tarball of this release is in the signed `SHA256SUMS`, the trusted publisher names `release-publish.yml` and environment `release`, and token publishing is disallowed on the package. ([evidence](<link>))
