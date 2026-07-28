---
title: Exact rename inference
---

# Todo

- [x] **Share the sequence hash.** Move `key::compound_hash` to `src/compare.rs` as `sequence_hash`, since it now hashes column values as well as key tuples, and derive `Hash` on `ComparisonPlan` and the two enums behind it so a digest can be cached per plan.
- [x] **Add `src/rename.rs`.** Infer renames between the provisional drops and adds using only the one-to-one matched rows: digest each candidate column over those rows under each comparison plan it participates in, compare only equal-digest pairs elementwise, and assign accepted pairs in column order.
- [x] **Apply inferred identities to the schema.** Turn each accepted pair into a `ColumnIdentity` carrying its own `type_changed` and `is_key: false`, remove both endpoints from `dropped` and `added`, and keep `identities` sorted by old position, which `detect_order` relies on.
- [x] **Run inference in the pipeline.** In `src/lib.rs`, infer renames after `reconcile_schema` and before `detect_order` and `compare_cells`, so ordering and cells both see the final bijection.
- [x] **Separate two existing fixtures.** `names_a_renamed_column_as_the_new_file_does` drops `gone` and adds `fresh`, both `[1, 2, 3]`, and `a_paired_component_cannot_be_read_as_two_components` drops `b` and adds `a`, both `[10, 20]`. Inference will pair each, changing both snapshots and the second one's column order. Give the columns different values so each test keeps testing its own subject, rather than absorbing an inferred rename it is not about.
- [x] **Cover the inference.** Unit tests in `src/rename.rs` for a found rename, a cross-type rename, incompatible and unmatched candidates, the no-matched-rows skip, ambiguity resolved in column order, and a forced digest collision that cannot manufacture a rename. Integration coverage in `tests/diff.rs` and a CLI snapshot.
- [x] **Refresh the demo datasets and documentation.** Add a `demo/rename-*.parquet` pair with a renamed non-key column, and describe inference in `demo/README.md` and `README.md`.
- [x] **Complete the acceptance pass.** Run `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`, and confirm repeated runs still produce byte-identical output.

# Goal

`data-diff` can now be *told* that two differently named columns are the same column, through a paired key component. It still cannot work that out for itself. Rename a non-key column and the output is a `col_drop()` and a `col_add()` with no relationship between them, even when the two columns hold identical values in every matched row — which is the strongest possible evidence that they are the same column.

This step infers those renames:

```console
$ data-diff demo/rename-old.parquet demo/rename-new.parquet --key id
col_key(declared: ["id"])
col_rename("amount" -> "total")
row_edit(2)
```

The edit there belongs to a third column, not to the renamed one. Acceptance requires equality in every matched row, and `compare_cells` then compares that identity over the same rows under the same plan, so an exactly inferred rename can never carry a value change of its own. It can carry a type change, which has no cells. A column that was renamed *and* edited is what approximate inference is for.

Nothing new is rendered. `col_rename()` already exists, derived from an identity whose two ends carry different names, so this step's whole job is to establish more of those identities from evidence rather than from declaration. Every consequence follows from the bijection: a renamed column stops being a drop and an addition, its cells are compared, it takes part in column ordering, and a rename that also changed type reports a `col_edit()` beside it.

# Scope

## What changes

* `src/compare.rs`: `sequence_hash` moves in from `key.rs`, and `ComparisonPlan` gains `Hash`.
* `src/key.rs`: the moved function and its call sites.
* `src/rename.rs`: new — candidate selection, digests, verification, and assignment.
* `src/schema.rs`: applying accepted pairs to a `SchemaMatches`.
* `src/human.rs`: two fixtures whose dropped and added columns hold identical values, which inference would now pair.
* `src/lib.rs`: one call, placed between schema reconciliation and order detection.
* `tests/diff.rs` and `tests/cli.rs`.
* `examples/generate_demo.rs`, `demo/README.md`, and `README.md`.

## What stays and why

`design.md` needs no amendment; this implements the "Exact renames" section as written, including its explicit note that there is initially no minimum row-count or information-content requirement.

Nothing in the human format changes. The rendering arrived with paired key components, and an inferred rename produces exactly the same shape of identity a declared pair does. That the output needs no work is the point worth testing rather than the point worth building: the integration test asserts a rename that was never declared appears as `col_rename()`.

Same-name identities are not candidates. Reinterpreting two same-named columns as each other's rename is a swap, which needs its own evidence rules and arrives with approximate inference.

The aligned tables the design describes are not materialized. `old_matching` and `new_matching` exist to put matched rows in a common order for column hashing; projecting a canonicalized column onto `rows.matched` in order produces exactly that sequence without copying two tables' worth of data. The design's requirement is the alignment, not the allocation.

## Explicitly deferred

* **Approximate renames and swaps**, the next step, which is also where thresholds, minimum row counts, and chance-corrected agreement arrive.
* **Hint exclusions.** Candidates would exclude endpoints reserved by `col_add`/`col_drop` hints and identities protected by `col_edit` hints; hints do not exist yet, and their queue entries carry the requirement.
* **Information content.** With no such requirement, two all-null columns are exactly equal and will be paired as a rename. This is what the design asks for initially, and it is stated here rather than left to be discovered: it is a real weakness of exact inference, and the natural place to fix it is alongside the approximate step's frequency machinery, which already computes the expected agreement that makes a column's information content measurable.
* **Budgets and sampling.** Exact inference hashes each candidate column a bounded number of times and verifies only equal-digest pairs, so it is not the stage the benchmarking step is aimed at.

# Design

## Candidates

The candidates are the provisional drops and adds from `reconcile_schema`. A pair is considered only when `ComparisonPlan::new` accepts its two Arrow types; incompatible pairs are skipped rather than compared. If `rows.matched` is empty there is no evidence at all, so the step is skipped and the columns stay as drops and additions — which also covers two tables with no key values in common, and every fanout row, since fanout groups never enter `rows.matched`.

## Digests over the matched rows

A column's evidence is its values in the matched rows, in matched order: canonicalize the whole column once, then project it onto `rows.matched`, taking `old_row` for an old column and `new_row` for a new one. Hashing that sequence with `sequence_hash` gives a digest that can be compared without comparing columns pairwise.

The subtlety is that canonicalization depends on the pair, not on the column: a string column compared with a string column keeps its bytes, while the same column compared with an integer column is parsed into integers. A single digest per column would therefore only ever match same-kind pairs, and would silently miss a column that was renamed and retyped in one step — a plausible edit, and one the design includes by saying only that the types must be compatible.

So a digest is computed per column *and plan*, and cached. In practice a candidate participates in one or two plans, so this is a small multiple of one pass per column, and it keeps the design's per-column hashing rather than degrading to a comparison for every pair.

Equal digests are then verified elementwise under the same plan before a pair is accepted, so a hash collision can never manufacture a rename. This is the discipline `KeyIndex` and `candidate_overlap` already follow, and the test forces a constant digest to prove it.

## Assignment

Accepted pairs are chosen by walking the old candidates in column order and taking, for each, the first unclaimed new candidate in column order whose values are equal. Where a column has exactly one exact partner and vice versa, this accepts that mutually unique pair. Where several columns are exactly equal — which is common, since equal columns are often equal to each other — it pairs them off in column order, which is what the design specifies for the ambiguous case and is deterministic without needing an assignment algorithm.

## Applying the result

An accepted pair becomes a `ColumnIdentity` with `type_changed` from its two Arrow types and `is_key: false`, its endpoints leave `dropped` and `added`, and `identities` is re-sorted by old position. That last part is a real requirement rather than tidiness: `minimal_moves` asserts its input is strictly increasing in old position, and inferred identities are discovered in candidate order, not schema order.

# Verification

* `src/rename.rs` unit tests cover: a renamed column found from identical values; a renamed and retyped column found across a compatible type pair; a candidate pair with incompatible types left alone; a column whose values match nothing staying a drop; no matched rows leaving every candidate untouched; and two equal old columns paired with two equal new ones in column order.
* One test forces every digest to collide and asserts the result is unchanged, since verification, not the hash, is what decides a rename.
* One test pins the all-null pairing described under deferrals. It is not desirable behavior, but it is the specified behavior, and pinning it means the step that fixes it has to change a test that says what it is doing.
* `tests/diff.rs` asserts a complete `Diff` for an inferred rename that also changed type and moved position, so it renders as `col_rename()` beside a type-only `col_edit()` and a `col_order()` entry, with `added` and `dropped` left empty and a separate column supplying the row edit, plus a repeated run that is structurally and byte-identical.
* Two tests defend the two ways this could pass by accident. One reorders the matched rows, so a rename is only found if the columns are compared through `rows.matched` rather than by position. The other gives a fanned-out key whose extra new rows disagree in the candidate columns while every matched row agrees: the rename must still be found, which it can only be if fanout rows are excluded.
* `tests/cli.rs` snapshots an inferred rename end to end, which is also the check that no rendering work was needed.
* Every remaining existing snapshot and assertion passes unchanged. That is a real check rather than an assumption: two fixtures do currently pair up under inference and are separated by the checklist item above, and the rest were confirmed by inspection to have either incompatible types or unequal values across their drop and add.

# Definition of done

This step is complete when:

* a dropped and an added column whose values agree in every matched row are identified as one column, reported as `col_rename()`, and removed from `columns.added` and `columns.dropped`;
* identification uses only one-to-one matched rows, ignores fanout rows, requires compatible types, and is skipped entirely when no rows matched;
* a digest collision cannot produce a rename, because every accepted pair is verified elementwise;
* ambiguous exact matches are paired in column order, deterministically;
* an inferred rename participates in cells, column ordering, and type-change reporting exactly as a declared pair does, with no change to the human format;
* the demo datasets and both READMEs describe inferred renames; and
* the full test suite, strict Clippy, formatting, and diff checks pass across the workspace, and repeated runs still produce byte-identical output.
