---
title: Shrink the cell coordinates behind an input ceiling
---

# Todo

- [x] **Shrink `CellCoordinate`'s repr to `u32`.** `CellCoordinateRepr` holds `[u32; 2]` positions instead of `[usize; 2]`, halving the diff's largest allocation — 40 bytes to 20 per changed cell, 400 MB to 200 MB at the 10⁷-cell grid cap. `from_zero_based` keeps its `usize` signature so no caller changes, converts with a documented panic that input validation makes unreachable, and the derived `Debug` prints the same numerals, so output identity holds by construction.
- [x] **Enforce the ceiling at input validation.** `validate_table` rejects a side whose row or column count exceeds `u32::MAX` with a new `DiffError::TableTooLarge` naming the side, the counts, and the ceiling — the owner-settled decision (2026-08-06): a checked error at the door rather than a panic in the middle, the ceiling being theoretical for arrow batches that are `i32`-indexed in practice. The model documents the ceiling where `Diff::cells` documents the retained invariant.
- [x] **Cover it.** The ceiling error triggered through `rows_without_columns` with a row count past `u32::MAX` (a column count that large cannot be constructed, so the symmetric check rides the same test's assertions on the error's shape); a boundary conversion at the largest valid position; a `should_panic` pin on the documented panic; a size assertion that the repr actually shrank.
- [x] **Gate on output identity against `12d7c78` and record the grid.** The worktree comparison over all eight scenarios; the grid re-run with the all-cells-changed points called out in `benches/README.md`; item 1 leaves `plan-next.md` and the rest renumbers.
- [x] **Complete the acceptance pass.** `cargo build --workspace --all-targets`, `cargo test --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`, byte-identical repeated runs, and `cargo bench` compiling and running.

# Goal

The all-cells-changed scenarios' remaining cost is the changed-cell vector itself, and half of it is width: four word-sized positions per cell where four `u32`s hold every table arrow can realistically deliver. The owner accepted the ceiling with validation at input (2026-08-06), so the conversion below is infallible-by-validation, the public read API is unchanged — the repr is private and `Debug` prints the same numerals — and the gate stays what it has been: output bit-identical to the committed baseline on every scenario.

# Scope

`src/model.rs` (repr, error variant, docs), `src/input.rs` (the ceiling check), `benches/README.md`, `plan-next.md`, tests. Everything else — accessors, human format, summarization, the invariant itself — reads through unchanged interfaces. Deferred: shrinking `Coordinate` (row/column events are few; no evidence it matters) and every other queue item.

# Definition of done

The repr is `u32` behind an input-validated ceiling with its documented error; output is verified bit-identical to `12d7c78`; the grid and queue are updated; and the full test suite, strict Clippy, formatting, and diff checks pass.
