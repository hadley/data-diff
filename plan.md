---
title: Drop JSON output
---

# Todo

- [x] **Remove the JSON output path.** Delete `src/json.rs`, its `mod json;` declaration, and the `write_json` re-export from `src/lib.rs`. In `src/main.rs`, delete the `OutputFormat` enum, the `--format` argument, and the `clap::ValueEnum` import, leaving one unconditional `write_human` call that keeps its current error message and trailing newline.
- [x] **Strip serialization from the result model.** In `src/model.rs`, remove the `use serde::Serialize` import, every `Serialize` derive, every `#[serde(...)]` attribute, and the three hand-written `impl Serialize` blocks for `Coordinate`, `CellCoordinate`, and `KeyOverlap`. Every field, constructor, and variant keeps its current name, visibility, and meaning; `KeyOverlap::ratio()` stays because `human.rs` reports it.
- [x] **Prune dependencies left without a consumer.** Drop `serde` from `[dependencies]`, drop the unused `indoc` dev-dependency, and reduce `insta` to its default features once no `assert_json_snapshot!` remains. Keep `serde_json`, which `human::quote` still uses to quote and escape column names, and refresh `Cargo.lock`.
- [x] **Rewrite the library integration tests against `Diff`.** Replace every `serde_json::to_value(diff)` assertion in `tests/diff.rs` with a direct assertion on the corresponding `Diff` field, building expected coordinates with the public `Coordinate::from_zero_based` and `CellCoordinate::from_zero_based`. Keep each surviving test's current granularity rather than collapsing them into whole-value comparisons, and remove the two tests that belong below it: delete the one whose claims are already made stage by stage, folding its composed claim into its neighbour, and move declared-key precedence into `key.rs`.
- [x] **Restate the determinism guarantee.** Convert both byte-comparison tests to assert structural equality of the two `Diff` values and byte equality of their rendered human output, so determinism covers both the retained evidence and the artifact users actually see.
- [x] **Rework CLI coverage around the single output format.** Update the help snapshot, convert the JSON-based process tests to human-output assertions, and delete the mixed-change JSON test that now only duplicates its human-format twin.
- [x] **Prune the model's serialization-shape tests.** In `src/model.rs`, replace the JSON assertions with `positions()` and `ratio()` assertions where behavior is still observable, and delete the tests whose only subject was the JSON document shape.
- [x] **Record the evidence deferral in `design.md`.** State that the CLI emits only the human format, that the complete cell-level change set is retained in the library `Diff` value, and that exposing that evidence to users again is future work.
- [x] **Update the README.** Remove the JSON format from the summary, usage, and behavior sections, and add re-exposing the complete result to the list of things `data-diff` does not do yet.
- [x] **Complete the acceptance pass.** Run the full test suite, strict Clippy, formatting, and diff checks, and confirm repeated runs still produce byte-identical output.

# Goal

`data-diff` is a visual tool for humans, and `design.md` states that machine-readable output is not a goal of the final product. The JSON format was an early scaffold for inspecting the result before the human format existed; it now has no place in the product but still shapes the result model, the CLI surface, the dependency list, and almost every integration test.

Remove it, leaving the human format as the only output, and leave behind a result model that carries exactly the same evidence with none of the serialization machinery:

```console
data-diff old.parquet new.parquet
```

This is a subtraction with no behavior change. Every diff that succeeds today succeeds afterwards, with byte-identical human output; every diff that fails today fails afterwards with the same message and exit status.

# Scope

The step removes the JSON output path and every fragment that existed only to serve it, then rewrites the tests that were written through it.

## What is removed

* `src/json.rs`, including `write_json` and the `CompactArrayFormatter` that kept coordinate arrays on one line.
* The `mod json;` declaration and the `pub use json::write_json;` re-export in `src/lib.rs`.
* The `OutputFormat` enum, the `--format` flag, and the format match in `src/main.rs`.
* Every `Serialize` derive, `#[serde(...)]` attribute, and hand-written `Serialize` implementation in `src/model.rs`.
* The `serde` dependency, the unused `indoc` dev-dependency, and `insta`'s `json` feature.

## What stays and why

The result model keeps every type, field, constructor, and doc comment it has today. `cells`, `columns.identities`, `rows.matched`, and the rest retain their current meanings even though nothing prints them: they are the complete cell-level evidence that `design.md` treats as a central invariant, and the human format deliberately summarizes rather than enumerates them.

`serde_json` stays in `[dependencies]`. `human::quote` uses `serde_json::to_string` to render column names as quoted, escaped JSON strings, which is what makes unusual names unambiguous in the human format. Hand-rolling that escaping to shed the dependency is a separate question and is not part of this step.

`Coordinate` and `CellCoordinate` keep their private `CoordinateRepr` and `CellCoordinateRepr` enums. Those enums are reachable from `from_zero_based` and `positions()`, so they are not dead code, and the collapsed old/new form is the coordinate vocabulary `design.md` uses throughout. Flattening them into plain old/new fields would be a behavior-preserving simplification, but it belongs with the step that decides how the retained evidence is displayed again, not with this removal.

`KeyOverlap` keeps its exact `shared` and `possible` counts and its `ratio()` accessor. Exact counts are what preserve `Eq` on `KeyDiff` and `Diff`, and `human.rs` calls `ratio()` for the `overlap:` field.

## Explicitly deferred

* Re-exposing the complete cell-level diff, which will most likely be library-only access to `Diff`, possibly with public coordinate accessors.
* Any change to what the human format prints, including rendering cells.
* Removing `serde_json` by writing the string escaping by hand.
* Simplifying the collapsed coordinate representations.
* Compacting test table construction, which is the next queued step and must not be mixed into this one.

# Test-rewrite design

## Library integration tests

`tests/diff.rs` is rewritten rather than dropped alongside the format it was written through, because it is the only coverage of three things. It exercises the assembly in `diff_tables` itself, including the zero-based to one-based translation applied to every index: no unit test calls `diff_tables`, so an off-by-one there, or `old` and `new` swapped in one `Coordinate::from_zero_based` call, passes the entire unit suite. It observes the public fields no output prints — `cells`, `columns.identities`, `rows.matched`, `summary.optimal`, and the exact `key.overlap` counts — which after this step is the only place the retained cell-level evidence is observed at all, making this file the working guard on the invariant `design.md` is asked to restate below. And it checks cross-stage consistency on a single input, where the unit tests feed each stage hand-built inputs standing in for the previous one.

Delete `disjoint_keys_are_all_atomic_row_events`. Its classification assertions restate `rows.rs`'s `disjoint_keys_make_every_row_atomic` on the same fixture values, and its empty-`cells` assertion restates `cells.rs`'s `added_and_dropped_rows_do_not_manufacture_cells`. Its one composed claim — that a fully disjoint pair yields no cells and an empty summary through the public entry point — moves into `empty_inputs_preserve_schema_and_classify_the_other_side` as a fourth case, which is renamed `unmatched_rows_are_classified_without_cells_or_edits` and keeps its existing schema-retention assertion.

Move `an_explicit_key_overrides_a_stronger_eligible_guess` into `key.rs`. Declared-key precedence is a key-resolution rule, and `key.rs` currently asserts it only in `declared_keys_bypass_the_zero_row_guard`, which covers the zero-row case; nothing covers a declared key winning over a stronger eligible candidate on non-empty inputs. As a unit test it asserts the declared basis, the absent overlap, and the selected column on inputs where a different column is the stronger candidate. The downstream half of the integration test, that the weaker declared key then drives row matching, follows mechanically from the resolved key and is covered by `rows.rs`. Keep `automatic_resolution_without_an_eligible_key_is_an_error`: the absence of any usable key is worth pinning at the public entry point even though `key.rs` covers each ineligibility rule.

Move `a_guessed_key_stays_out_of_top_level_changed_cells` into `cells.rs` as `a_guessed_key_column_is_excluded_like_a_declared_one`. Key exclusion is a cell-comparison rule that `cells.rs` already covers for a declared key, and cell comparison cannot see the basis, so the claim needs a unit test with a guessed key rather than a whole-pipeline one. The test helper there gains a `changes_with` variant that resolves automatically when no key is named.

Delete `summary_forces_and_coalesces_type_edits`; every claim it makes exists upstream. `cells.rs`'s `type_changes_are_independent_of_value_changes` covers a key column whose type changes without producing cells and a column that changes in both type and value, `schema.rs`'s `records_key_and_non_key_type_changes` covers type changes on and off the key, `summary.rs`'s `forced_columns_are_coalesced_before_optimization` covers forcing a cell-free type-changed column into the summary, and the CLI's `empty_files_still_report_type_only_schema_changes` now shows the whole path end to end as printed output.

The remaining tests currently read the diff through `serde_json::to_value` and assert against `json!` literals with one-based coordinates. Each assertion becomes a direct comparison against the matching `Diff` field, with expected values built through the public constructors:

```rust
assert_eq!(
    diff.columns.identities,
    vec![
        Coordinate::from_zero_based(0, 1),
        Coordinate::from_zero_based(1, 0),
    ]
);
```

`from_zero_based` names its convention at every call site, so no local one-based helper is introduced. `Vec<usize>` fields such as `columns.added` and `rows.dropped` are already one-based and compare against plain vectors unchanged. `summary` compares against a constructed `EditSummary`, and `key` against a constructed `KeyDiff` including `Some(KeyOverlap { shared, possible })`, which replaces today's float assertion with exact counts.

Assertions stay field-by-field at their current granularity. Comparing whole `Diff` values would force every test to spell out both schemas and would obscure what each test is about.

## Determinism

Today both determinism tests compare `serde_json::to_vec` output byte-for-byte. Restate the guarantee as two assertions over the same pair of runs:

```rust
assert_eq!(first, second);
assert_eq!(render(&first), render(&second));
```

Structural equality of the two `Diff` values is the broader check: it covers the retained evidence the human format never prints, and because every field is `Eq` — including `KeyOverlap`, which stores counts rather than a float — the comparison is exact and total. Byte equality of `write_human` output preserves the guarantee in the form users observe. Both determinism tests, declared-key and guessed-key, get both assertions.

## Model unit tests

The tests in `src/model.rs` that assert a JSON document shape have no subject once serialization is gone:

* `coordinate_collapses_equal_positions` and `coordinate_retains_moved_positions` become assertions on `positions()`, which is `pub(crate)` and reachable from the inline test module.
* `cell_collapses_when_both_positions_agree` and `cell_retains_both_positions_when_either_moves` are deleted. `CellCoordinate` has no accessor, and adding a `pub(crate)` one with no production caller would be dead code under strict Clippy. Cell coordinates remain covered by equality in `tests/diff.rs`.
* `diff_serializes_in_stable_field_order` and `declared_keys_omit_overlap` are deleted; field order is not a property of a Rust value, and a declared key's absent overlap is asserted where keys are resolved.
* `overlap_serializes_as_a_normalized_ratio` becomes a `ratio()` assertion.
* `empty_summary_is_still_optimal` is unchanged.

## CLI tests

* The help snapshot loses its `--format` line.
* `compares_two_parquet_files_as_json` becomes a human-format test over an identical pair, asserting `col_key(declared: ["id"])` followed by `no_changes()`, a successful status, and empty stderr. It remains the one test that proves a clean run writes nothing to stderr.
* `guesses_a_key_when_the_flag_is_omitted` keeps its human snapshot and loses the second invocation that re-ran the binary for JSON.
* `reports_mixed_changes_from_real_parquet_files` is deleted. Its schema, row, and order assertions duplicate `reports_mixed_changes_in_human_format` on the same fixture shape, its cell and summary assertions duplicate `combines_schema_row_order_and_cell_changes` in `tests/diff.rs`, and its remaining assertion was about the compact-array JSON formatter.
* `empty_files_still_report_type_only_schema_changes` becomes a human snapshot of the two type-only `col_edit` lines.
* `reports_a_missing_key_when_nothing_can_be_guessed`, `failure_writes_context_only_to_stderr`, and `reports_mixed_changes_in_human_format` are unchanged.

`use serde_json::json;` disappears from both `tests/cli.rs` and `tests/diff.rs`; the crate remains reachable there through `human::quote`'s dependency but nothing in the tests needs it.

# Documentation

`design.md` gains the deferral. The introduction's first principle already says machine-readable output is not a goal; extend the cell-comparison section to say that the complete one-to-one change set is retained in the library result model, that no current output renders it, and that giving users access to it again is future work. The invariant now constrains the library rather than the CLI, and the wording should say so plainly.

`README.md` loses JSON from its opening sentence, loses the `--format json` example and the paragraph introducing it, and states that the human format is the only output. Add re-exposing the complete result to the closing list of deferred capabilities. `demo/README.md` never mentioned JSON and needs no change beyond a read-through confirming its commands remain accurate.

# Definition of done

This step is complete when:

* `data-diff` accepts no `--format` flag and writes only the human format;
* `src/json.rs`, `write_json`, and `OutputFormat` no longer exist;
* no `Serialize` derive, `#[serde(...)]` attribute, or `Serialize` implementation remains in the crate;
* `serde`, `indoc`, and `insta`'s `json` feature are gone from `Cargo.toml`, `serde_json` remains for `human::quote`, and `Cargo.lock` is refreshed;
* the result model exposes the same types, fields, and constructors as before, with the complete cell-level evidence intact;
* `tests/diff.rs` asserts against `Diff` values directly, carries no test whose claims are already made stage by stage, and both determinism tests assert structural equality of the diff and byte equality of its human output;
* CLI coverage exercises the single output format, including a clean run, a guessed key, a declared key, a type-only change, and both failure paths;
* human output for every existing fixture is byte-identical to what it produced before this step;
* `design.md` records that the complete cell-level diff is retained in the library and that re-exposing it is future work;
* the README describes the human format as the only output and lists re-exposing the complete result as deferred; and
* the full test suite, strict Clippy, formatting, and diff checks pass.
