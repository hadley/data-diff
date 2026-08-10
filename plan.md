---
title: Lazy paginated lookup of input values
---

# Todo

- [x] **Add a public `Value` type.** `src/value.rs`: a closed enum over the source types — `Null`, `Boolean`, `Int64`, `Double`, `String`, `Timestamp` (with unit and timezone), `Date32`, `Date64`, `Decimal128`, `Decimal256`, `Opaque` (canonical row-format bytes, the same encoding `compare` measures). `NaN` stays distinct from `Null` and from the empty string, and a type-changed column extracts each side in its own source type. It derives `PartialEq` but not `Eq`, a raw `f64` being what an honest display wants. `compare.rs`'s `CanonicalValue` stays internal. Nulls are logical: a valid dictionary key pointing at a null value extracts as `Null`, the comparison's own rule.
- [x] **Build the `Lookup` surface.** `src/lookup.rs`: `Lookup::new(old, new)` borrows the two `RecordBatch`es; `value(side, row, column)`, `values(side, &coords)` (the paginated form — request order, duplicates preserved, page size the caller's), and `row(side, row)`. Positions are one-based, matching the model; out-of-range is a checked `DiffError::RowOutOfRange` / `ColumnOutOfRange` naming the side and the table's extent.
- [x] **Borrow the batches, do not re-read the files.** Owner-approved (2026-08-10): retention lives in the caller-owned `Lookup`, the `Diff` invariant is untouched, and the CLI drops it immediately.
- [x] **Expose the coordinate accessors the callers need.** Surfaced by the work: `CellCoordinate` had no position accessor and `Coordinate::positions` was `pub(crate)`, and translating a diff into lookup positions is the caller's half of the contract. Both are now public (`CellCoordinate::positions() -> ([u32; 2], [u32; 2])`); the derived `Debug` is unchanged, so output identity holds.
- [x] **Cover it.** Inline unit tests per convention: extraction across every domain including the null/NaN/empty-string triple, the dictionary-hidden null, a type-changed column read from both sides, opaque encoding equality across interning; one-based access, request order with duplicates, full-row reads, the checked errors at and past the boundary, repeated lookups identical. Integration in `tests/lookup.rs`: a diff's changed-cell coordinates fed back through `Lookup` name values that differ while every unchanged matched cell agrees (the human format renders no values, so this semantic tie replaces the planned rendering comparison), added/dropped row positions read their rows, fanout cells resolve against both sides, and the whole diff-then-lookup path repeated is byte-identical.
- [x] **Complete the acceptance pass.** `cargo build --workspace --all-targets`, `cargo test --workspace --all-targets` (all ten suites), `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check` — all clean. No output changes, so `tests/readme.rs` passes unmodified.

# Goal

ui-design.md maps every view onto the existing `Diff` with exactly one new library surface: lazy, paginated lookup of cell and row values from the input tables. `Diff::cells` holds only `CellCoordinate`s and added/dropped rows are only positions, by invariant; the UI's cell view, row-view expansions, and column-view table all need the underlying values on demand. This step adds that surface and nothing else, so the UI step that follows builds against a reviewed, tested API.

# Scope

`src/value.rs` (the `Value` enum and extraction), `src/lookup.rs` (`Lookup`), `src/model.rs` (the two out-of-range error variants and the now-public position accessors), `src/lib.rs` (exports), tests. Explicitly deferred: the Tauri/Preact UI itself (planned in `ui-plan.md`, next queue item); any pagination state, caching, or prefetch policy (caller's concern); key-column-aware rendering (the UI composes `Lookup::row` with the key columns from `Diff::key`); and every other queue item.

# Definition of done

`Value` and `Lookup` are public, documented, and covered by isolated fixtures and integration tests tying them back to the diff's own coordinates; repeated runs are byte-identical; the full suite, strict Clippy, formatting, and diff checks pass; `plan-next.md` has the UI item first and fast paths renumbered behind it (done when the step was queued).
