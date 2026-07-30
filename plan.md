---
title: How much changed
---

# Todo

- [x] **Give a column edit a count instead of a flag.** `ColumnEdit` carries `changes: usize` where it carried `values_changed: bool`, one field saying what two did: a count is positive exactly when values changed. `SummaryColumn` and the internal edit set follow.
- [x] **Give a row edit the same count.** `EditSummary::rows` becomes a `Vec<RowEdit>` of a coordinate and a count, rather than a bare `Vec<Coordinate>`, and `SummaryChanges::rows` gains the count beside the pair it already holds.
- [x] **Count a fanout's differing comparisons.** `FanoutEvent` already carries every cell it found; the renderer counts them rather than asking whether there are any.
- [x] **Render `changes: {number}`, and retire `changed: values`.** `col_edit(price, changes: 3)`, `row_edit(2, changes: 4)`, `row_fanout(4 -> [4, 5], changes: 2)`. A type-only edit carries no count, having nothing to count.
- [x] **Correct the fixed field-name set.** `design.md` lists three of the names the format writes, and the renderer's guard test reaches five of six, `incompatible` having no fixture. Both should name the whole set, which this step changes anyway.
- [x] **Cover the machinery.** Unit tests in `src/summary.rs` for a count that spans a held-out column and for two overlapping events whose counts deliberately exceed the cells between them; in `src/human.rs` for each line's new field; integration coverage in `tests/diff.rs` for the counts on a complete `Diff`, including a fanout and a hinted column; CLI snapshots in `tests/cli.rs`.
- [x] **Update `design.md` and both READMEs.** The vocabulary says what a count counts, the value-changes section says why the numbers do not sum, and the output tables show the field.
- [x] **Complete the acceptance pass.** Run `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`, and confirm repeated runs still produce byte-identical output.

# Goal

The format is precise about exactly one kind of edit. A retyped column prints the pair it changed between; everything else says only that something changed:

```console
$ data-diff demo/mixed-old.parquet demo/mixed-new.parquet --key id
col_key([id], basis: declared)
col_drop(product)
col_add(stock)
col_order(price, 3 -> 1)
col_edit(price, changed: values)
row_drop(2)
row_add(3)
row_order(3 -> 1)
```

`col_edit(price, changed: values)` is the same line whether one price changed or every price did, and `row_edit(2)` is the same line whether that row changed in one column or in forty. That is an inconsistency rather than a considered summary: the tool holds the complete cell-level diff throughout, and declines to say the one thing about it a reader most wants — how much. So every edit event gets a count:

```console
$ data-diff demo/mixed-old.parquet demo/mixed-new.parquet --key id
col_key([id], basis: declared)
col_drop(product)
col_add(stock)
col_order(price, 3 -> 1)
col_edit(price, changes: 2)
row_drop(2)
row_add(3)
row_order(3 -> 1)
```

The count replaces `changed: values` outright rather than joining it. A count is positive exactly when values changed, so the flag was the count with its magnitude thrown away, and a type-only edit needs neither — `col_edit(id, type: Int32 -> Int64)` already says everything about itself. That also ends the asymmetry the flag introduced, where one aspect of an edit reported evidence and the other reported only that there was some.

This is a display change and nothing more. Reconciliation is untouched: the minimum-cover objective counts events rather than cells by design, so no count can move which events are chosen, and the cells the counts are read off are already retained in full.

# Scope

## What changes

* `src/model.rs`: `ColumnEdit::values_changed` becomes `changes: usize`, and a new `RowEdit` carries a coordinate and a count; `EditSummary::rows` holds them.
* `src/summary.rs`: `SummaryColumn` and a new `SummaryRow` carry counts, computed over the complete changed-cell set rather than over the graph the optimizer was left with.
* `src/human.rs`: `changes: {number}` on `col_edit()`, `row_edit()`, and `row_fanout()`, in place of `changed: values` and of the fanout's emptiness test.
* `src/lib.rs`: the counts through to `Diff`.
* `design.md`, `README.md`, `demo/README.md`, `tests/diff.rs`, and `tests/cli.rs`.

## What stays and why

`src/cells.rs` is untouched. It already produces every changed cell, grouped by identity, which is the only evidence a count needs; `ColumnChanges::values_changed()` stays as the internal predicate it is, since summarization still asks whether a column has any cells before putting it in the graph.

No new demo datasets. Every case a count can show — one cell, several, a held-out column, a fanout — is already in `demo/`, so the demos gain numbers rather than files.

Reconciliation does not move, and neither do the thresholds. A count is read off the result; nothing reads a count.

`row_add()` and `row_drop()` gain nothing, and neither do `col_add()` and `col_drop()`. An added or dropped row is an atomic event whose cells are deliberately never compared with anything, so it has no changed cells to count; `design.md` says as much where it keeps them out of the changed-cell set and out of summarization.

## Explicitly deferred

* **`optimal: false` and budgets.** Still the benchmarking entry's business. Counts are correct whether or not the cover is minimum, since each one describes its own row or column rather than the cover.
* **Any display of the cells themselves.** The complete set stays in `Diff` and unrendered, as it has been; a count summarizes it rather than being a step toward enumerating it.
* **Proportions.** Decided against below rather than deferred, but recorded here too: no `changes: 3/11`, and no percentage.

# Design

## What a count counts

**Every changed cell incident to the event.** A `col_edit()` counts the matched rows in which that column differs; a `row_edit()` counts the identified columns in which that row differs.

The consequence has to be stated plainly, because it looks like a bug: **the counts do not sum to the number of changed cells, and are not meant to.** A cell at the intersection of a reported row and a reported column is counted by both. Given changed cells at `(r1, c1)`, `(r1, c2)`, and `(r2, c1)`, the minimum cover is one row and one column, and the output is `col_edit(c1, changes: 2)` beside `row_edit(r1, changes: 2)` — four, over three cells.

The alternative is to count only the cells nothing else covers, so that the numbers partition the cell set. It is rejected, for two reasons:

* **It makes the number an artifact of the cover.** `design.md` says any minimum cover is acceptable and that no particular tied cover is preferred, precisely so the implementation may choose freely. A count that changed with that choice would make the tie-break user-visible, and would need a rule of its own to stay deterministic.
* **It makes the number false about its own subject.** `row_edit(2, changes: 4)` should mean row 2 has four changed cells, because that is what a reader will take it to mean and what they can check against the data. Under the partitioning reading it would mean "four changed cells not otherwise accounted for", which is a statement about the summary rather than about the data.

So a count is a fact about the row or column it sits on, and the events are overlapping descriptions of one change rather than a decomposition of it. That is already true of the events themselves — a rectangular change is reported by rows *or* by columns, whichever is fewer — and the counts inherit it rather than introducing it.

A held-out column follows the same rule. Cells in a retyped or hinted column leave the optimizer's graph, but they are still changed cells, so they count toward that column's own edit and toward any row edit they fall in. That is what keeps a `col_edit()` hint from quietly changing what a count means: a hint moves which events are reported, not what is true of a row.

## Where a count is absent

A type-only edit prints no count. `col_edit(id, type: Int32 -> Int64)` has no changed cells to count, and `changes: 0` would be a zero the reader has to interpret rather than an absence they can read past. Every count the format writes is therefore positive, which is also what lets `changes` replace `values_changed` without loss: the flag was true exactly when the count is positive.

## A fanout's count

`row_fanout(4 -> [4, 5], changed: values)` becomes `row_fanout(4 -> [4, 5], changes: 2)`. What it counts is the differing comparisons inside the event — one old row against each of its new rows, over the identified non-key columns — which is what the event holds and what `Diff` already exposes as `FanoutEvent::cells`.

This is a slightly different quantity from a row edit's count, because a one-to-many event has no single cell to point at: two new rows disagreeing with the old one in the same column is two. That is the honest reading of a fanout and needs no special spelling, the count sitting on a line whose whole subject is the one-to-many relationship.

## Not a proportion

`col_edit(price, changes: 3)` rather than `changes: 3/11` or a percentage. A count is exact and true without a denominator; a proportion invites the reader to weigh it against a threshold, and the thresholds in this tool decide identity rather than describe edits. The one proportion the format writes is a guessed key's `overlap`, where the denominator is the whole point — how much of the data the key accounts for — and it is not a precedent for anything an edit reports.

## The field-name set

`design.md` says field names are drawn from a fixed set and then lists `basis`, `overlap`, `type`, which was already three of the five the format writes: `missing` and `incompatible` appear on `hint_ignored()` lines. With `changed` replaced by `changes`, the set is `basis`, `changes`, `incompatible`, `missing`, `overlap`, `type`, and the design should say so.

The renderer's `every_field_name_comes_from_the_fixed_set` guard has the same gap from the other direction: it asserts the set exactly, but no fixture in it produces an `incompatible` line, so that name is neither reached nor listed. A fixture for it makes the guard cover every field the format can write, which is what it claims to do.

# Verification

* `src/summary.rs` unit tests: a column's count equal to its changed rows and a row's count equal to its changed columns; a count that includes cells in a held-out column, which is the claim that a hint moves events and not facts; and the overlapping case above asserted exactly — two events whose counts sum to more than the cells between them — so the decision is pinned rather than rediscovered.
* `src/human.rs`: a rendering per line kind carrying its count, and one showing a type-only edit with no count beside a both-changed edit with one. The field-name guard grows an `incompatible` fixture and asserts the full six-name set.
* `tests/diff.rs` asserts counts on a complete `Diff`: a column edit, a row edit, a fanout, and a `col_edit` hint that forces a column, checking that the forced column's count is its own cells and that a row edit surviving beside it counts the cells in that column too.
* One test pins that a rendered `changes` is always positive, and absent exactly where the edit is type-only.
* `tests/cli.rs` snapshots the counts on the demo-shaped fixtures, confirming the exit status stays zero.
* Repeated runs of every changed fixture are structurally and byte-identical.

# Definition of done

This step is complete when:

* every edit event the format writes carries `changes: {number}` where it has cells to count, and `changed: values` is gone from the format;
* a count is every changed cell incident to its own row or column, so counts overlap and do not sum, with `design.md` saying so and a test pinning it;
* a type-only `col_edit()` carries no count, and every count written is positive;
* `ColumnEdit` carries a count in place of `values_changed`, and `EditSummary::rows` carries `RowEdit`s rather than bare coordinates;
* `design.md` and the renderer's guard both name the whole fixed field-name set, including `incompatible`;
* `README.md` and `demo/README.md` show the counts and say what they count; and
* the full test suite, strict Clippy, formatting, and diff checks pass across the workspace, and repeated runs still produce byte-identical output.
