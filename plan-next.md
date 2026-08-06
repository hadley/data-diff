---
title: data-diff next steps
---

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`. Implement only that plan, leave the result uncommitted for owner review, and do not start the following item until the owner has reviewed and committed the current work.

1. Shrink `CellCoordinate` to `u32` coordinates. After the sampled-counts cache and the counting placement (2026-08-06), the all-cells-changed scenarios' remaining cost really is the changed-cell vector itself: four `usize`s per cell where four `u32`s would halve the memory and bandwidth of the retained cell-level invariant. It is a public model change with a real ceiling — 4 billion rows or columns — so the plan should state the ceiling where the model documents the invariant, decide what an input beyond it does (arrow batches are `i32`-indexed in practice, so a checked conversion with a clear error is likely enough), and ride the same output-identity gate as the performance steps. Settle the ceiling's acceptability with the owner before implementing.
2. Cross-representation fast paths: extend the native equivalence arguments one bijection at a time — a seconds column against a milliseconds one, decimals of equal value across scales, `Utf8` against `LargeUtf8` — wherever the pair's canonical verdict is still a pure function of the raw values under an inline conversion. Same shape as the same-type steps: each widening argued at the dispatch, everything else falling back, output verified bit-identical. Dictionary hydration belongs here too if a clean logical-value argument exists; otherwise it stays a fallback.

Whoever opens either step should re-profile first: four performance steps (2026-08-05 to 2026-08-06) took the identical 1M×10 floor from 3.59 s to 198 ms and the worst grid point from ~6.9 s to ~0.5 s, and the profile reshapes each time the totals shrink.
