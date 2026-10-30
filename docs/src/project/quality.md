# Quality baselines

The accepted scores of the quality gates (build plan M18, [Testing § 9](design/testing.md#9-search-and-triage-evaluation)).
Each evaluation scores the median of its runs; CI fails when a median drops more than 0.01 below the
figure here, or under the gate. A baseline is the median of five runs on the commit that sets it, and
changing it is a reviewed change with the score deltas in the pull request.

| Figure | Baseline | Gate | Measured |
|---|---|---|---|
| `it.keyword_recall_at_10` | pending | no drop | – |
| `search.hybrid_recall_at_10` | pending | ≥ 0.90 (NFR-QUAL-1) | – |
| `agentic.citation_precision` | pending | ≥ 0.98 (NFR-QUAL-2) | – |
| `triage.category_accuracy` | pending | ≥ 0.85 (NFR-QUAL-3) | – |
