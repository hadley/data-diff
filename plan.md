---
title: Bounded declared-key fanout
---

# Todo

- [x] **Admit bounded fanout during key resolution.** In `src/key.rs`, extract the collision-safe key lookup shared by validation and row matching into a `KeyIndex`, keep old-side duplication fatal, and replace new-side rejection with the affected-key rate rule: retain the declared key when at most 10% of shared key values are duplicated in `new`, and otherwise fail with a new `DiffError::ExcessiveFanout { affected, shared }`. Delete `DiffError::UnsupportedFanout`.
- [x] **Classify fanout groups when matching rows.** In `src/rows.rs`, rebuild `match_rows` on `KeyIndex` and apply the design's classification order, so a shared key with two or more new rows becomes one `FanoutGroup { old, new }` whose new rows are neither matched nor added, and a duplicated key absent from `old` still produces additions.
- [x] **Nest fanout cells inside their event.** In `src/cells.rs`, compare each fanout group's old row against every new row over identified non-key columns during the existing per-identity pass, and return the result as `CellChanges::fanout` — one entry per group, ordered by old row, with cells sorted by `(new_row, old_column, new_column)`. Fanout cells never enter `ColumnChanges::rows`, so they cannot reach `changed_cells()`, `columns.edited`, or summarization. Growing the struct breaks the three exhaustive `CellChanges` literals in `src/summary.rs`'s test module, which gain `..CellChanges::default()` so they stay about the columns they construct and survive the next field too.
- [x] **Surface fanout at the library boundary.** In `src/model.rs`, add `FanoutEvent { old, new, cells }` with one-based coordinates and `RowsDiff::fanout`; export `FanoutEvent` from `src/lib.rs` and populate the field from `CellChanges::fanout`.
- [x] **Render `row_fanout()`.** In `src/human.rs`, emit one `row_fanout(old -> [new, ...])` line per event between the `row_add()` and `row_order()` blocks, with a `, values` suffix when the event has changed cells.
- [x] **Add integration coverage and determinism checks.** Extend `tests/diff.rs` with a bounded-fanout comparison asserting the complete `Diff`, that fanout cells stay out of `cells` and `summary`, and that repeated runs are structurally and byte-identical; extend `tests/cli.rs` with a retained-fanout snapshot and the excessive-fanout error.
- [x] **Refresh the demo datasets and documentation.** Regenerate `demo/fanout-*.parquet` as a retained fanout, add `demo/fanout-broad-*.parquet` for the rejected case in `examples/generate_demo.rs`, and update `demo/README.md` and `README.md` to describe fanout as supported within the bound.
- [x] **Complete the acceptance pass.** Run `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`, and confirm repeated runs still produce byte-identical output.

# Goal

A declared key that is unique in `old` but repeats in `new` is currently fatal: `validate_unique` reports `UnsupportedFanout` for the first repeat and no comparison happens. That is the wrong answer for the case the design cares about — a join that accidentally duplicated a handful of rows — where the key still identifies almost every row and the useful report is "these two new rows both came from this old row", not "your key is broken".

This step implements the design's declared-key fanout rule. A key survives when at most 10% of the key values shared between the two inputs are duplicated in `new`; each affected key becomes one `row_fanout()` event holding the old row, all of its new rows, and the cells that differ between them. Everything else about the comparison is unchanged, because a fanout event is self-contained: its cells stay inside it, so the top-level cell set, the minimum edit summary, and row ordering all continue to describe one-to-one matched rows only.

After this step:

```console
$ data-diff demo/fanout-old.parquet demo/fanout-new.parquet --key id
col_key(declared: ["id"])
row_fanout(4 -> [4, 5], values)
```

# Scope

## What changes

* `src/key.rs`: a shared `KeyIndex`, the affected-key rate, and the admission decision.
* `src/rows.rs`: fanout groups in `RowMatches`, and the classification order that produces them.
* `src/cells.rs`: per-group cell comparison, kept out of the one-to-one cell set.
* `src/summary.rs`: three test-module `CellChanges` literals only; the solver and its inputs are unchanged.
* `src/model.rs` and `src/lib.rs`: `FanoutEvent`, `RowsDiff::fanout`, the replaced error variant, and the assembled result.
* `src/human.rs`: the `row_fanout()` operation.
* `tests/diff.rs` and `tests/cli.rs`: integration and CLI coverage.
* `examples/generate_demo.rs`, `demo/README.md`, and `README.md`: the demo pair that now succeeds, a new pair that still fails, and the prose describing both.

## What stays and why

`src/order.rs` and `src/summary.rs` keep their logic. Both already consume only `RowMatches::matched` and `CellChanges::columns`, which is exactly the exclusion the design requires of fanout rows and fanout cells, so keeping fanout out of those two inputs is the whole of the work; `summary.rs` changes only where its tests build `CellChanges` exhaustively. That the solvers need no edit is a property worth asserting: the integration test checks that a fanout with changed values produces no `row_edit()`, no `col_edit()`, and no `row_order()` entry for the fanned-out row.

Old-side duplication stays fatal with `NonUniqueOldKey`. The design defines fanout as one-directional and says the MVP terminates on old-side duplication, and nothing in this step changes that. Old uniqueness is also what makes the rate well defined, so it is still validated first: with a unique `old`, each old row contributes one distinct key and the shared-key count is a row count.

Guessed keys still cannot fan out. `candidate_overlap` rejects any candidate column that repeats a value on either side, so a guessed key is unique on both sides by construction. This step adds a test pinning that, but no code. Whether guessing should admit bounded fanout is a real question rather than a settled one, and it is the first item in `plan-next.md`; it changes key selection rather than key validation, so it needs its own `design.md` amendment.

`KeyOverlap` stays `None` for declared keys. The affected-key rate is an admission decision, not evidence about the key that a consumer needs; reporting it belongs with the issue channel described below.

## Explicitly deferred

* **Falling back to a guessed key when a declared key is rejected.** The design says an invalid declared key records an `invalid_key` issue and continues to guessing and then to row numbers, with the fallback basis visible. There is no issue channel in `Diff` today, and inventing one here would mean silently swapping the user's declared key for a guessed one with nowhere to say so. Excessive fanout therefore stays a fatal error with counts in the message, and the fallback arrives with the row-number fallback step, which is where the design's "which stages remain valid" question is decided.
* **Reverse fanout.** Many old rows mapping to one new row is undefined by the design.
* **Tuning the 10% threshold, or exposing it as an option.** It is a named constant with the design's value.
* **Rendering fanout cells.** The human format never enumerates cells; `row_fanout()` reports the coordinates and whether anything differs, and the cells themselves are reachable only through `Diff`, like the one-to-one cell set.

# Design

## The admission rule

Key resolution validates a declared key in the existing order: components exist, types are compatible, no null or `NaN`, `old` is unique. Only then does new-side duplication become a question, and it is answered by the design's affected-key rate. With `old` unique, every old row is a distinct key value, so both counts are obtained in one pass over the old rows:

* `shared` counts old rows whose key occurs at least once in `new`, which is $|K_o \cap K_n|$;
* `affected` counts old rows whose key occurs two or more times in `new`, which counts each affected key once however many new rows it produces.

The key is retained when `affected * 10 <= shared`, which is $f \le 0.10$ evaluated in exact integer arithmetic, inclusive at the boundary as the design specifies. `shared == 0` forces `affected == 0`, so the design's $f = 0$ convention for no shared keys needs no special case: a new-side key that has no counterpart in `old` is a set of additions, never a fanout, and cannot invalidate the key.

Otherwise resolution fails with `ExcessiveFanout { affected, shared }`, displayed as `declared key fans out for 3 of 5 shared key values, above the 10% limit; supply a different --key`. The counts are the reason for the decision, which is why they replace `UnsupportedFanout`'s first-repeat row pair: with a rate rule, one example repeat no longer explains the outcome.

## Sharing the collision-safe lookup

Both `key.rs` and `rows.rs` need "the new rows whose key equals this key", and both need it to survive a hash collision — a bucket may hold rows with different keys, so membership must be confirmed by comparing canonical values, exactly as `match_rows` and `candidate_overlap` do today. This step gives that one home in `key.rs`:

```rust
pub(crate) struct KeyIndex<'a> {
    keys: &'a [Vec<CanonicalValue>],
    buckets: HashMap<u128, Vec<usize>>,
    hash: fn(&[CanonicalValue]) -> u128,
}

impl<'a> KeyIndex<'a> {
    pub(crate) fn new(keys: &'a [Vec<CanonicalValue>]) -> Self;
    fn with_hash(keys: &'a [Vec<CanonicalValue>], hash: fn(&[CanonicalValue]) -> u128) -> Self;
    /// Rows whose key equals `key`, in ascending row order.
    pub(crate) fn rows(&self, key: &[CanonicalValue]) -> impl Iterator<Item = usize> + '_;
}
```

Buckets are built in row order and `rows` filters within a bucket, so results are ascending and independent of hash iteration order. `match_rows` is then driven by the old rows in order and a `used_new` mask, which keeps every output list deterministic without sorting.

The confirmation step is unreachable unless two keys collide, so the hash is a field rather than a hard-wired call and `with_hash` lets a test force every key into one bucket. A plain `fn` pointer carries it, which costs one word and no generic parameter, and it mirrors `candidate_overlap`, which already takes its hash for the same reason.

## Row classification

`match_rows` applies the design's table in its stated order, checking old-side presence before new-side multiplicity:

| Present in `old` | Rows in `new` | Result |
| --- | ---: | --- |
| yes | 0 | `dropped` |
| no | 1 or more | each row in `added` |
| yes | 1 | `matched` |
| yes | 2 or more | one `FanoutGroup` |

```rust
pub(crate) struct RowMatches {
    pub added: Vec<usize>,
    pub dropped: Vec<usize>,
    pub matched: Vec<(usize, usize)>,
    pub fanout: Vec<FanoutGroup>,
}

pub(crate) struct FanoutGroup {
    pub old: usize,
    pub new: Vec<usize>,
}
```

Groups are ordered by old row and their new rows ascending. A fanned-out old row appears in no other list, and its new rows are not additions, so `added`, `dropped`, and `matched` continue to partition the remaining rows.

## Fanout events and cell separation

`compare_cells` already canonicalizes each identified column pair once and then walks the matched rows; the fanout comparison joins that pass rather than adding a second one. For each identity that is not a key column, and for each group, the old row's value is compared against each new row's value and any difference is recorded against that group. Added and dropped columns still produce no cells, and key columns are excluded inside events exactly as they are at the top level.

```rust
pub(crate) struct CellChanges {
    pub columns: Vec<ColumnChanges>,
    pub fanout: Vec<FanoutChanges>,
}

pub(crate) struct FanoutChanges {
    pub old: usize,
    pub new: Vec<usize>,
    pub cells: Vec<ChangedCell>,
}
```

There is one `FanoutChanges` per group even when nothing differs, because the design keeps the event whether or not values changed. Cells are sorted by `(new_row, old_column, new_column)`; the old row is constant within an event, and grouping by new row is what makes the event readable as "how each new row differs from the old one". Nothing is pushed into `ColumnChanges::rows`, so a column whose only differences are inside fanout events is not an edited column, does not appear in `columns.edited`, and contributes no edge to the vertex cover.

At the boundary, `RowsDiff` gains

```rust
pub struct FanoutEvent {
    pub old: usize,
    pub new: Vec<usize>,
    pub cells: Vec<CellCoordinate>,
}
```

with one-based positions. `old` and `new` are plain positions rather than `Coordinate`s: a `Coordinate` pairs one old with one new position and collapses when they agree, and a fanout has one old row against many new rows, so there is no pair to collapse. This matches `rows.added` and `rows.dropped`, which are also plain one-based positions. The cells keep `CellCoordinate` because each of them does pair one old and one new coordinate.

## Human format

A fanout is a row-population event like an addition or a drop, so it is emitted with them, after the `row_add()` block and before `row_order()`:

```text
col_key(declared: ["id"])
row_add(7)
row_fanout(4 -> [4, 5], values)
row_order(6 -> 5)
row_edit(2)
```

The arrow and bracketed list mirror `col_key`'s bracketed component list and `row_order`'s `old -> new`, and the `, values` suffix mirrors `col_edit`'s detail suffix. The suffix is present exactly when the event has at least one changed cell, which is the one thing a reader cannot infer from the coordinates and which the design explicitly calls out as varying. No cells are enumerated.

# Verification

* Unit tests in `key.rs` cover: a retained key at exactly the 10% boundary; a rejected key with the counts in the error; each affected key counted once when one key produces three new rows; new-side duplicates whose key is absent from `old` leaving the key valid with `affected == 0`; a duplicated new key with no shared keys at all; old-side duplication still reported as `NonUniqueOldKey` when both sides duplicate; and a guessed key never fanning out.
* One of those cases exists to pin the denominator, which the other cases cannot distinguish because their old row count and shared-key count coincide. Twenty old keys of which five occur in `new`, with one of those five duplicated, must be rejected as `ExcessiveFanout { affected: 1, shared: 5 }`: dividing by the shared keys gives $f = 0.2$ and rejects, while dividing by all old keys would give $f = 0.05$ and wrongly retain. The error's counts are asserted, so the test also fails if the right decision is reached for the wrong reason.
* Unit tests in `rows.rs` cover the classification table, including ascending new rows within a group, the disjointness of the four lists, and stability under a forced hash collision.
* Unit tests in `cells.rs` cover: fanout cells appearing in their event and not in `changed_cells()` or `columns`; an event with no changed cells; and key and added/dropped columns excluded within events.
* Compound keys are tested rather than assumed. Keys are already tuples of canonical values, so fanout should need no extra code for them, but "should need none" is a claim about the tuple path and is checked directly: one `key.rs` test admits a fanned-out compound key and one `cells.rs` test asserts the resulting event and its cells. The shared fixture makes the duplicated rows agree on the first component and differ only in the second, so a grouping that read one component instead of the tuple would classify the wrong rows, and it gives the second component different types across the sides, so fanout has to be using the same per-component comparison plans as matching. It also needs ten distinct compound keys to put one duplicate inside the 10% bound, which is worth knowing before writing it: the small two-row and three-row compound fixtures already in the suite would be rejected as excessive rather than retained.
* One `cells.rs` test makes a single identified column carry both kinds of change at once — one matched row differs and one fanned-out key also differs in that column — because separation must partition the column's cells rather than suppress the column. The matched cell stays in `ColumnChanges::rows` and `changed_cells()` while only the fanout cell is nested, and `tests/diff.rs` carries the same fixture to the boundary, asserting that the matched cell still reaches `Diff::cells`, `columns.edited`, and the minimum summary, which covers it with one `row_edit()`. The summary is asserted whole, so a fanout cell leaking into the cover would show up as an extra event. This is the direct check that the complete cell-level diff invariant survives: every changed cell is still reachable, each from exactly one place.
* A `human.rs` snapshot pins the rendered line, its position among the row operations, and both the plain and `, values` forms.
* `tests/diff.rs` asserts a complete `Diff` for a bounded fanout — including that `summary` and `order.rows` ignore it — and repeats the comparison to assert structural and byte-identical equality.
* `tests/cli.rs` snapshots the retained fanout through the binary and asserts the excessive-fanout message on stderr with a non-zero status.
* The demo pair `demo/fanout-*.parquet` becomes ten keys with one duplicated in `new`, which is exactly the 10% boundary and therefore retained; `demo/fanout-broad-*.parquet` keeps the current two-key shape, whose single duplicate is 50% and is rejected. `demo/README.md` gains a section for each, and `README.md` moves fanout from the rejection list to the current-behavior list.

# Definition of done

This step is complete when:

* a declared key that is unique in `old` and duplicates at most 10% of the shared key values in `new` is retained, and one is rejected above that bound with `ExcessiveFanout { affected, shared }`;
* every shared key duplicated in `new` produces exactly one fanout event carrying the old row, all its new rows in ascending order, and the cells that differ, and no other row event or cell mentions those rows;
* fanout cells are absent from `Diff::cells`, `columns.edited`, `summary`, and `order.rows`, and reachable only through `rows.fanout`, while a column carrying both a matched-row change and a fanout change keeps the matched one in the ordinary cell and edit-summary path;
* the human format renders one `row_fanout()` line per event between the row additions and the row ordering, with `, values` exactly when the event has changed cells;
* `DiffError::UnsupportedFanout` no longer exists, and old-side duplication is still `NonUniqueOldKey`;
* the demo datasets and both READMEs describe fanout as supported within the bound and rejected above it; and
* the full test suite, strict Clippy, formatting, and diff checks pass across the workspace, and repeated runs still produce byte-identical output.
