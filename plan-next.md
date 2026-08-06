---
title: data-diff next steps
---

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`. Implement only that plan, leave the result uncommitted for owner review, and do not start the following item until the owner has reviewed and committed the current work.

The queue is currently empty.

# Performance leads

Starting points for whoever opens the next performance step. The same-type fast-path step (in flight, 2026-08-06) absorbed the leads it could ride with — `KeyIndex::new`'s pre-sizing and double lookup, `minimal_moves`' missing early-out, `RowSample::select`'s full sort, and the canonicalization materialization in cell comparison and digest discovery. What remains is an observation, not a commitment — re-profile first, since relative shares move as the total shrinks.

- On all-cells-changed inputs, materializing `Vec<CellCoordinate>` dominates after inference (~30% of `swapped` 100k×100, 2026-08-05): the vector is never reserved and the coordinate type is two `usize`s where two `u32`s would halve the bandwidth — but shrinking it is a model change, so it wants its own look.
- Exact-rename verification and informativeness still materialize both columns and build cloned-value frequency maps, which is now the dominant cost on string tables: `renamed_strings` 1M×10 spends ~4 of its ~4.6s there (2026-08-06), the digest join's native streaming having removed only the discovery pass. A native verification (comparing raw arrays over matched rows) is the same equivalence argument the fast path already wrote; informativeness needs frequency counts, where borrowing string keys instead of cloning them is the smaller, safer half. Sharing one canonicalization between cell comparison and `Aligned` for the pairs that still materialize — cross-type and dictionary columns — remains unshared and matters less.
- Cross-representation fast paths (same family, different unit or scale) extend the same-type arguments one bijection at a time; deferred from the fast-path step so each widening gets its own review.
- Parallelism remains deliberately unexplored — a different risk class than any of the above; nothing in the current structure forecloses it, and the per-column independence of canonicalization, projection, and cell comparison is the natural seam.

Defer decisions about hint syntax, UI presentation, and threshold changes until the prerequisite behavior exists and can be benchmarked. Preserve the central invariants: deterministic reconciliation, no inferred event without underlying evidence, and a result model that retains the complete cell-level diff.
