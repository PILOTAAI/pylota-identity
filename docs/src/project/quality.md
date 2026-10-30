# Quality baselines

The accepted scores of the quality gates (build plan M18, [Testing § 9](design/testing.md#9-search-and-triage-evaluation)).
Each evaluation scores the median of its runs; CI fails when a median drops more than 0.01 below the
figure here, or under the gate. A baseline is the median of five runs on the commit that sets it, and
changing it is a reviewed change with the score deltas in the pull request.

| Figure | Baseline | Gate | Measured |
|---|---|---|---|
| `it.keyword_recall_at_10` | 0.4934 | no drop | 2026-10-30, `it::search::golden_keyword_recall` with the fake AI: MRR 0.4846, nDCG@10 0.4844, zero-result rate 0.40 (paraphrase, multilingual and typo queries, which keyword search does not answer), every exact reference found. Keyword search is deterministic, so one run is its median |
| `search.hybrid_recall_at_10` | pending | ≥ 0.90 (NFR-QUAL-1) | – |
| `agentic.citation_precision` | pending | ≥ 0.98 (NFR-QUAL-2) | – |
| `triage.category_accuracy` | pending | ≥ 0.85 (NFR-QUAL-3) | – |

The golden mailbox quarantines 766 messages as `otp_unsolicited` (Inbound › Verification codes and
unsolicited OTP): the trigger words `code` and `confirm` with a reference or promotion code close
after them match ordinary mail, such as "Use code FLEET23 at checkout" or "Can you confirm the
cancellation? Booking reference: BK-2291". Quarantined mail is hidden from search, which these figures
include.
