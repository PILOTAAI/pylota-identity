# 0019 Pylota Mail becomes Pylota Identity

| | |
|---|---|
| Status | Accepted |
| Date | 2026-10-24 |
| Deciders | The owner (TREFT LTD) |
| Related | [PRD](../prd.md), [Architecture](../architecture.md), [ADR 0010](0010-cloud-in-the-existing-cloudflare-account.md), [ADR 0018](0018-typescript-sdk-in-its-own-repository.md) |

## Context

The product began as email and identity for AI agents. The owner wants it to give an agent the other
identifiers and secrets an agent needs to act in the world: phone numbers that can be obtained in the
United Kingdom, Europe and the United States, and a credential vault built for an agentic OS, in the way
1Password serves people. The name "Pylota Mail" describes one of those capabilities. Those two capabilities
are designed separately (their research and design records follow this one); this record covers the name.

Nothing is deployed except the landing page: Pylota Identity Cloud is not commissioned, no crate or package is
published, and the only code is the M0 skeleton (PR #7). Renaming now costs only text.

## Decision

1. **Product and repository.** The product is **Pylota Identity**; the hosted service is **Pylota Identity
   Cloud**; the repository is `PILOTAAI/pylota-identity` (renamed by the owner on GitHub, which keeps
   redirects from `PILOTAAI/pylota-mail`).
2. **Code and packages.** The crates are `pylota-identity-core`, `-platform`, `-api-types`, `-worker`,
   `-conformance`, `-cli` and the Rust SDK `pylota-identity`; the TypeScript SDK is `@pylotahq/identity`
   in `PILOTAAI/pylota-identity-typescript`; every name was unregistered on crates.io, npm and PyPI on
   2026-10-24. Planned Cloudflare resources that do not exist yet take the new name (`pylota-identity`,
   `pylota-identity-blobs`, …), and so do contract strings that nothing has implemented (the WebSocket
   subprotocol `pylota-identity.events.v1`, the DNS label `_pylota-identity`, the `User-Agent`
   `PylotaIdentity/1.0`, the configuration directory `~/.config/pylota-identity`).
3. **The command-line tool is `idvy`** ("id" plus "v" for vault). On 2026-10-24 it was unregistered on
   crates.io, npm, PyPI, Homebrew (formulae and casks), Debian sources and the Arch AUR; no installed command
   on macOS uses it; GitHub has one personal repository with that name and no stars; a web search found no
   software called "idvy". The names first proposed (`pident`, `pylota-id`) and `pmail` were rejected
   because other tools already use them. `lotid` and `lyid` were equally free and are the fallbacks.
4. **Kept as they are**, because each names something that exists or that renaming would break:
   - the domain `pylotamail.com` and its hostnames (`api.`, `app.`, `www.`) and addresses: the landing page
     is live there and it stays the default mailbox domain until the owner chooses another;
   - the landing page Worker `pylota-mail-site`: it is deployed, its routes are bound to it, and the site
     token is scoped to that Worker by name ([ADR 0010](0010-cloud-in-the-existing-cloudflare-account.md));
   - the `PM_` prefix of configuration variables and the `pm-` prefix of resource names: they are internal
     identifiers, not the product's name, and renaming every one would touch the whole design for no user
     benefit;
   - the path of Pylota's own integration spec, `docs/plans/pylota-mail-integration.md` in the Pylota
     repository, which that repository renames on its own schedule.
5. **The local folder** `~/Desktop/pylota-mail` keeps its name for now: the desktop app's trust and the
   agents' worktrees are bound to that path.

## Consequences

- Every document, test name and manifest in this repository says Pylota Identity and `idvy`. A reader who
  meets `pylota-mail` in an old link reaches the right repository through GitHub's redirect.
- The landing page shows the new name on the existing domain. A new domain, when the owner buys one, is a
  separate change of hostnames, DNS and the site Worker, with its own record.
- Build waves stay paused until the phone and vault designs are written and accepted (owner decision,
  2026-10-24).

## Alternatives considered

- **Rename the repository only.** It leaves the product, packages and CLI named after one capability.
- **Move to a new domain now.** There is no new domain yet; renaming hostnames to a placeholder would break
  the live site and every example.
