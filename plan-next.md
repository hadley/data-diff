---
title: data-diff next steps
---

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`. Implement only that plan, leave the result uncommitted for owner review, and do not start the following item until the owner has reviewed and committed the current work.

1. A same-type fast path for comparison, gated on output identity — an optimization, never a mode. First add a string-heavy scenario to `test-support`'s generators, since the integer grid understates canonicalization: every string value clones its bytes into `CanonicalValue`, and the constant-factor profile (2026-08-05) measured cell-comparison canonicalization at only ~3% on integer fixtures. Then re-profile after the constant-factor step lands. The path itself has two tiers: cell comparison over same-type columns compares raw arrow values directly instead of materializing two `Vec<CanonicalValue>` per column, sound wherever the canonical equality verdict is a pure function of the raw values (integers trivially; string-vs-string keeps its bytes; doubles compare by bit pattern; same-unit timestamps scale bijectively) — no hashing is involved, so digests and sampling are untouched; and digest passes stream the canonical byte encoding straight from the arrow array without building `CanonicalValue`s, bit-identical because the encoding is a pure function of the value. Each type's equivalence argument is reviewed on its own (NaN payloads, `-0.0`, dictionary arrays, timestamp units, decimal scales), and any type without a clean argument keeps the canonicalizing path.

# Performance leads

Starting points for whoever opens the next performance step, from the post-constant-factor profile of `identical` 1M×10 (2026-08-05, ~3500 in-pipeline samples); none is worth a step alone, but several could ride along with item 1, the same-type fast path. Each is an observation, not a commitment — re-profile first, since relative shares move as the total shrinks.

- `KeyIndex::new` is now the largest single block (~1160 samples plus ~460 of `HashMap` growth): the digest map is never pre-sized (`with_capacity` from the row count would cut the rehashing), and construction looks each row's digest up twice — once counting, once placing — where remembering bucket ids from the first pass would halve the lookups.
- `minimal_moves` runs its full Fenwick-tree LIS even when the matching is already in order (~820 samples on *identical* tables, where the answer is trivially empty): an `is_sorted` early-out on the new positions is O(n), exact, and hits the commonest case of all.
- `RowSample::select` sorts every matched row's `(digest, position)` pair to keep the smallest 4096 (~360 samples of quicksort at 1M rows): `select_nth_unstable` at the cap followed by sorting only the kept prefix selects the identical set — the comparator's total order is unique because positions are distinct — in O(n + k log k).
- Canonicalization materializing `Vec<CanonicalValue>` per column (~920 samples) and `compare_cells` (~420) are item 1's territory — the same-type fast path — as is sharing one canonicalization between cell comparison and `Aligned`, which today each canonicalize the same column independently.
- On all-cells-changed inputs, materializing `Vec<CellCoordinate>` dominates after inference (~1050 of ~3500 samples on `swapped` 100k×100): the vector is never reserved and the coordinate type is two `usize`s where two `u32`s would halve the bandwidth — but shrinking it is a model change, so it wants its own look.
- Parallelism remains deliberately unexplored — a different risk class than any of the above; nothing in the current structure forecloses it, and the per-column independence of canonicalization, projection, and cell comparison is the natural seam.

Defer decisions about hint syntax, UI presentation, and threshold changes until the prerequisite behavior exists and can be benchmarked. Preserve the central invariants: deterministic reconciliation, no inferred event without underlying evidence, and a result model that retains the complete cell-level diff.
