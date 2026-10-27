# mail-auth 0.13.3, patched

A copy of `mail-auth` 0.13.3 from crates.io (`mail-auth-0.13.3.crate`, SHA-256
`8505122ba86e1f4adeb664196c1e787c3f29bb6e7c128e4a366d47d209911440`, the checksum in the crates.io index;
published 2026-09-18; Apache-2.0 OR MIT, licence texts in `LICENSES/`), used through `[patch.crates-io]`
in the workspace `Cargo.toml` (Rust workspace § 3).

## The change

One line of `Cargo.toml`: the non-wasm32 `reqwest` dependency of the `dns-doh` feature no longer asks for
the feature `webpki-roots`, which no `reqwest` 0.13 release has (0.13.0 to 0.13.5 offer `rustls`,
`rustls-no-provider` and private `__rustls*` features; crates.io sparse index, read 2026-10-27). Cargo
resolves every target's dependencies, so with that line any workspace naming `mail-auth` with `dns-doh`
fails to lock, on wasm32 too (spike S4). `rustls` stays. The wasm32 dependency table is unchanged.

```diff
 [target.'cfg(not(target_arch = "wasm32"))'.dependencies.reqwest]
 version = "0.13"
 features = [
     "rustls",
-    "webpki-roots",
     "http2",
```

The Worker never calls the DoH client: DNS answers are pre-filled into the caches (Inbound ›
Authentication verdict, step 2), so the change touches no code path that runs.

## Removed from the copy

`Cargo.lock`, `Cargo.toml.orig`, `examples/`, `tests/` and `resources/` (test fixtures, including a
sample RSA private key, that only the crate's own tests read). `src/` is byte for byte the published
source.

## Upstream

No issue about this was found in the stalwartlabs/mail-auth tracker or changelog (web search,
2026-10-27). Drop this copy once a release fixes the feature list, or move to it.
