---
title: data-diff next steps
---

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`. Implement only that plan, leave the result uncommitted for owner review, and do not start the following item until the owner has reviewed and committed the current work.

1. Cross-representation fast paths: extend the native equivalence arguments one bijection at a time — a seconds column against a milliseconds one, decimals of equal value across scales, `Utf8` against `LargeUtf8` — wherever the pair's canonical verdict is still a pure function of the raw values under an inline conversion. Same shape as the same-type steps: each widening argued at the dispatch, everything else falling back, output verified bit-identical. Dictionary hydration belongs here too if a clean logical-value argument exists; otherwise it stays a fallback.

Whoever opens the step should re-profile first: four performance steps (2026-08-05 to 2026-08-06) took the identical 1M×10 floor from 3.59 s to 198 ms and the worst grid point from ~6.9 s to ~0.5 s, and the profile reshapes each time the totals shrink.
