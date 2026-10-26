# Staging: text waiting to be pasted into shared files

This folder is temporary and is deleted before the pull request merges.

Three designs are written in parallel: phone numbers (ADR 0020), the agent vault (ADR 0021) and the SDK
languages (ADR 0022). Several shared files need text from each of them: the PRD, the edge-case register,
the build plan, `openapi.yaml`, the `reference/` pages, the data model, architecture, security, privacy,
billing, `SUMMARY.md` and the indexes. To keep three writers out of one another's way, each design puts its
part of those files here, one file per target, instead of editing the shared file directly.

## Layout

`.staging/<area>/<target path with / replaced by __>.md`. The first line of each file says exactly where
the text goes (after which heading or row). `<area>` is `phone`, `vault` or `sdk`.

Files whose name starts with `_` are notes between designs, not text to paste: `phone/_to-vault.md` records
the vault interfaces the phone design uses (the port-out PIN as a managed secret, SMS codes handed to the
vault) and two points to settle.

## Allocations

| Area | ADR | Requirements | Edge cases | Milestones |
|---|---|---|---|---|
| Phone numbers | 0020 | FR-PHN-1 to FR-PHN-42 | `P1`–`P107` | M38–M41 |
| Vault | 0021 | FR-VLT-1 to FR-VLT-25 | `U1`–`U72` | M42–M45 |
| SDK languages | 0022 | FR-SDK-25 onward | `Y72` onward | M46–M48 |

The section letters `P` and `U` sit close to the priorities `P0`–`P2` and the selling points `U1`–`U10`;
choose other letters at integration if that reads better, and change every reference at once.

## Integration, once all three designs are written

1. Paste every staged block into its target, in milestone order, and renumber milestones contiguously.
2. Add the new pages to `docs/src/SUMMARY.md`, `design/index.md` and `adr/index.md`. Until then mdBook
   does not render them and the link check does not see them.
3. Build the docs and run the link check; every anchor the new pages link to must exist.
4. Delete this folder.
