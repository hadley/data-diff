---
title: data-diff next steps
---

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`. Implement only that plan, leave the result uncommitted for owner review, and do not start the following item until the owner has reviewed and committed the current work.

1. Cut the pipeline's constant factor on large inputs, guided by the profile taken while tuning the budgets (2026-08-04): on an identical 1M×10 pair (~3.6s), roughly 40% of samples are allocator traffic — a fresh `Vec<u8>` per value in `stable_hash`, one heap `Vec` per row in `ResolvedKey`'s key values, a per-row `collect` in `match_rows` — and another ~30% is `Aligned::fill` eagerly canonicalizing, digesting, and building full-column frequency maps for columns whose only question was a 4096-row sampled measurement, while the truly necessary linear work (canonicalization, xxh3, the key index, the LCS) is a small minority. In impact order: make projections lazy in `Aligned` so a sampled measurement projects only the sampled rows and digests and counts are built only when the exact stage asks; stream values into the hasher instead of allocating a byte buffer per value; flatten the per-row key representation and the per-row bucket collects. Re-run `benches/pipeline.rs` before and after; the acceptance rule stays the tuned budgets', and byte-identical output is the regression gate, none of this being semantic.

Defer decisions about hint syntax, UI presentation, and threshold changes until the prerequisite behavior exists and can be benchmarked. Preserve the central invariants: deterministic reconciliation, no inferred event without underlying evidence, and a result model that retains the complete cell-level diff.
