---
title: Cache sampled counts and place changed cells without sorting
---

# Todo

- [x] **Cache sampled frequency counts per column.** `Projection` gains a `sampled_counts` piece beside `counts`: the value frequencies over the sampled rows, built once per (column, plan) on the first sampled measurement — with capacity for the sample, so the map never grows — and reused by every later pair. A sampled measurement becomes one zip over the two cached sampled projections for the agreeing count plus `expected()` over the two cached maps, instead of rebuilding two fresh maps per measurement: the 2026-08-06 profile of `full_rewrite` 100k×100 puts ~46% of the run in exactly that rebuilding, ~10k crossing measurements each re-inserting two 4096-entry maps. The `Agreement` values are identical by construction — same values, same partition, same commutative sum — and the memo and meter are untouched.
- [x] **Place changed cells by counting instead of sorting.** `CellChanges::changed_cells` stops materializing 10M coordinates and driftsorting them (~12% of the same profile): each column's cells are already ascending in `old_row`, and the sort key `(old_row, old_column, …)` is unique per cell, so a counting placement — per-row totals, prefix sums, then one pass over the columns in ascending `old` index writing each cell at its row's cursor — produces byte-for-byte the order `sort_by_key` produced, in two linear passes. A unit test pins the construction against the sorted one over a fixture with sparse, overlapping, and reordered columns.
- [x] **Gate on output identity against `5acf7c8`.** The worktree comparison over all eight scenarios at sampling and exhaustion sizes; identical `Diff` digests everywhere.
- [x] **Re-run the grid and record it.** `benches/README.md` gets the fresh table; the adversarial multipliers this targets — `full_rewrite`, `swapped`, and `rename_and_modify` at the map-rebuild-bound points — are called out with before/after, and the two measured halves of the acceptance rule are re-verified.
- [x] **Update the leads.** The `CellCoordinate` lead is rewritten to today's evidence: the sort is gone, the remaining cost is the vector itself and the retained invariant behind it, and shrinking the coordinate type stays a model change for its own step.
- [x] **Complete the acceptance pass.** `cargo build --workspace --all-targets`, `cargo test --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`, byte-identical repeated runs, and `cargo bench` compiling and running.

# Goal

Re-profiling the all-cells-changed case (2026-08-06, `full_rewrite` 100k×100 at ~1.7s) overturned the queued lead: the cost is not the coordinate vector but two habits around it. Sampled measurements rebuild their frequency maps from scratch per pair, though the counts are a fact about the column and the sample rather than the pair — the same amortization the full counts already have — and the changed-cell list is globally sorted though its order is derivable in linear time from what the per-column structure already guarantees. Both fixes are internal and output-identical: the same `Agreement` numbers from cached maps, the same cell order from counting placement, gated as always on bit-identical output against the committed baseline.

# Scope

## What changes

- `src/agreement.rs`: `Projection::sampled_counts`, built once and consulted by `Over::Sampled`.
- `src/cells.rs`: the counting placement in `changed_cells`.
- `benches/README.md`, `plan-next.md`, and the test suites.

## What stays and why

- **Every `Agreement` value, memo, and charge.** Cached counts are the same counts; a sampled measurement still costs its sample.
- **The changed-cell order and the complete cell set.** The placement reproduces the sort's exact order because its key is unique per cell; the retained invariant is untouched.
- **The model.** No coordinate type changes; the superseded lead is re-evidenced, not silently dropped.

# Verification

- A unit test pinning counting placement against the sorted construction; the existing sampled-measurement tests unchanged; determinism on repeated runs.
- The worktree output-identity comparison against `5acf7c8` over all eight scenarios.
- The grid re-run recorded in `benches/README.md`.

# Definition of done

Sampled counts build once per column; `changed_cells` runs in linear passes; output is verified bit-identical to `5acf7c8`; the grid and leads are updated; and the full test suite, strict Clippy, formatting, and diff checks pass.
