Where this goes: `docs/src/project/design/billing.md`, "## What the Worker meters", a paragraph at the end. Source: [Vault § 19](vault.md#19-billing-and-limits).

**The vault** is not metered in v1. It is available on paid plans, and to `exempt` tenants of a partner; on a
deployment with `PM_BILLING=stripe` a Free-plan workspace gets `403 policy_denied` (`plan_required`) on every
vault route. Its rows and objects do not count toward `storage_gb`; the item limits bound them. No `QuotaRequest`
variant is added.
