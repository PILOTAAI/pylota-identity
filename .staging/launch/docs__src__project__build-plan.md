Where this goes: `docs/src/project/build-plan.md`. Five insertions, each introduced by where it goes. The launch milestone is numbered M49, after the SDK milestones M46–M48; renumber with the others at integration and keep the anchor `#m49--launch` used by [Cloud commissioning](cloud-commissioning.md) and [Launch readiness](launch-readiness.md) in step.

## 1. In "## Dependency graph", in the second `text` block, replace the line "M20 staging + live proof ─▶ M28 Cloud production commissioning (phases A, B, C)" with

```text
M20 staging + live proof ─▶ M28 Cloud production commissioning (phases A, B, C; gates P, V, S as their milestones land)
M28, M41, M45, M48 ─▶ M49 launch (every row of Launch readiness done or n/a for the gates opened)
```

## 2. In "## M19 · Site, docs and release pipeline …", add to **Acceptance** after the last bullet

- The site serves `/terms`, `/privacy`, `/dpa`, `/subprocessors` and `/trademarks` from pages counsel and the
  owner supplied ([Launch readiness §4](launch-readiness.md#4-legal-pages-and-processors)); the landing page
  links each from its footer; a link check covers them.
- `xtask::release_artefacts_carry_licence`: every archive and package the release produces (the Worker
  bundle, the CLI archives, each `.crate`) contains `LICENSE.md` and a `THIRD_PARTY_LICENSES` file generated
  from its SBOM; `cargo deny check licenses` is green.
- The release notes and `CHANGELOG.md` state each version's release date, from which its Apache-2.0 date
  (the second anniversary) is computed and printed beside it.
- A `dco` CI job refuses a pull request whose commits lack a `Signed-off-by` line matching the author
  (`xtask::dco_refuses_unsigned`).
- `xtask::docs_forbidden_claims` scans `site/public/index.html` copy as well as `docs/` and refuses "open
  source" as a description of Pylota Identity (an `id` attribute is not copy).

## 3. In "## M20 · Staging deploy and live proof", add after step 16

17. Phone numbers and the vault on staging, with the live tests their milestones name: a GB number bought,
    used for SMS both ways and released; a US number whose first send is refused before its 10DLC campaign
    is provisioned; a vault fill by a conformant browser consumer with a TOTP phase and a session
    write-back; each product's kill switches turned off and on again
    (`live::ops::phone_kill_switches`, `live::ops::vault_kill_switches`).

## 4. In "## M28 · Cloud production commissioning (after M20)", add to **Acceptance** after the gate C bullet

- Gate P ([Cloud commissioning §10](cloud-commissioning.md#10-go-live-gates)), stage by stage: checks
  CC20–CC27; the Telnyx steps of [§11](cloud-commissioning.md#11-phone-numbers-the-telnyx-account) for the
  countries offered; the phone rows of [Launch readiness §3.3](launch-readiness.md#33-phone-numbers) `done`.
  `PM_PHONE=on`; `phone.allowed` on for Pylota's first operator, then every Pylota operator, then others.
- Gate V: checks CC28–CC32; Pylota's consumers activated after their conformance fixtures; the vault rows of
  [Launch readiness §3.4](launch-readiness.md#34-vault) `done`. `PM_VAULT=on`; `vault.allowed` on for
  Pylota's tenants, then others.
- Gate S: check CC33 on the first full Python and Go SDK releases.

## 5. New milestone, after "## M28 · Cloud production commissioning (after M20)"

---

## M49 · Launch (after M28, M41, M45 and M48)

**Files:** `docs/src/project/launch-readiness.md` (exists; statuses updated as rows close),
`xtask/src/launch.rs` (parses the tracker's tables), `release-gates/v1.0.0.md`, and in the private operations
repository the gate notes, signed legal documents and review reports the tracker names.

| File (owner milestone) | Hook |
|---|---|
| `xtask/src/main.rs`, the `release-gate` command (M19) | Reads `docs/src/project/launch-readiness.md` and refuses `v1.0.0` while a row whose **Blocks** names `v1.0` is neither `done` nor `n/a` |
| `crates/cli/src/commands/doctor.rs` (M16) | A `launch` section: for each flag that is on (`PM_SIGNUP` phase, `PM_PHONE`, `PM_VAULT`), warns when a row that blocks its gate is not `done`, reading the tracker published with the release |

**Implements:** FR-OPS-6 and NFR-OPS-4.

**Acceptance:**

- `xtask::launch_readiness_refuses_open_blockers`: a tracker with a `v1.0` row `open` makes `release-gate`
  fail with the row's ID; the same tracker with it `done` and an evidence link passes; a row without an
  owner, evidence or status is a parse error.
- `cli::doctor::launch_readiness_gates`: with `PM_PHONE=on` and a `P1` row `open`, `idvy doctor` prints a
  `warn` naming the row.
- Every row of [Launch readiness](launch-readiness.md) is `done` or `n/a` for the gates opened, each with
  evidence a later reader can check: the legal pages and sub-processor list live (CC20), counsel's answers
  recorded for the questions that block an opened gate, the tax decisions recorded, the licence rows done,
  the security sign-off rows done, the operations rows done, and each opened gate's first-week note written
  ([Launch readiness §11](launch-readiness.md#11-launch-day-and-the-first-week)).
- A fresh agent that never saw the deployment reads [Launch readiness](launch-readiness.md) and
  [Cloud commissioning](cloud-commissioning.md) alone and reports every row it cannot verify from the
  evidence named; the owner fixes each before the milestone is accepted.
