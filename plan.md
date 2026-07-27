---
title: Compact test fixture construction
---

# Todo

- [x] **Add the `test-support` workspace member.** Turn the root `Cargo.toml` into a workspace with one member, add `test-support/Cargo.toml` depending on `arrow-array` and `arrow-schema` at the versions the root crate pins, and add `test-support = { path = "test-support" }` to the root `[dev-dependencies]` so both the inline `#[cfg(test)]` modules and the integration tests can use it. Refresh `Cargo.lock`.
- [x] **Implement the fixture value model.** In `test-support/src/lib.rs`, add the `CellValue` trait, its `Kind` classification, the `Cell` value enum, the annotation vocabulary, and the array builder that turns a list of cells and a resolved Arrow type into an `ArrayRef`.
- [x] **Implement the `column!` and `table!` macros.** Cover the annotated and unannotated forms, the empty-list form, and the zero-column form, and add `rows_without_columns`. Give the crate its own unit tests asserting the produced `DataType`, null placement, row count, and field nullability for every form the fixtures use, the exact keys and dictionary of `dict` and the exact bytes of `binary`, and a `#[should_panic]` case for each run-time misuse through both macros.
- [x] **Convert the five behavior modules.** Rewrite the fixtures in `src/cells.rs`, `src/human.rs`, `src/key.rs`, `src/rows.rs`, and `src/schema.rs` to `table!`, delete each module's local `fn table`, and confirm each test module's imports no longer mention `std::sync::Arc` or any Arrow array type.
- [x] **Convert the representation modules.** Rewrite the fixtures in `src/input.rs` and `src/compare.rs`, and delete `input.rs`'s local `fn table` and `fn empty`. Keep explicit Arrow construction where an Arrow type is the subject of the assertion, which after this step means `compare.rs`'s hand-encoded dictionary alone; leave a comment there saying why it stays.
- [x] **Convert the integration tests.** Replace `common::batch` and `common::empty_batch` with `table!` in `tests/diff.rs`, `tests/cli.rs`, `tests/input.rs`, and `tests/smoke.rs`, and delete both helpers from `tests/common/mod.rs`, which keeps `TempDir` and the Parquet writers.
- [x] **Complete the acceptance pass.** Run `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`, confirm no inline snapshot changed, and confirm repeated runs still produce byte-identical output.

# Goal

Every fixture in the suite is written as `("id", Arc::new(Int64Array::from(vec![1, 2, 3])))`, and the same `fn table(columns: Vec<(&str, ArrayRef)>) -> RecordBatch` is copied into six modules and duplicated again as `common::batch` for the integration tests. The scaffolding is longer than the data it carries, so a two-column fixture takes six lines and the shape of the table has to be decoded rather than read.

Replace both with one shared construction helper that infers the Arrow array type from the literal values:

```rust
let old = table! {
    "id" => [1, 2, 3],
    "value" => [10, 20, 30],
};
```

A successful result is a suite that mentions Arrow only where an Arrow type is genuinely part of what a test says: the `RecordBatch` and `ArrayRef` parameters of a module's own pipeline helpers, the `DataType` enumerations that two tests are entirely about, and one hand-encoded dictionary whose subject is the encoding. This is a pure refactor: every existing assertion keeps its current meaning, every fixture keeps its current Arrow types, no inline snapshot changes, and the suite stays green throughout.

# Scope

## What changes

* One new workspace member, `test-support`, holding the fixture macros and the small helpers that go with them.
* The fixtures and imports of the seven inline test modules that build tables or arrays: `cells`, `human`, `key`, `rows`, `schema`, `input`, and `compare`.
* The fixtures of the four integration test files, and the removal of `batch` and `empty_batch` from `tests/common/mod.rs`.
* `Cargo.toml`, which gains a `[workspace]` section and one dev-dependency, and `Cargo.lock`.

## What stays and why

Every assertion, test name, and inline snapshot stays exactly as it is. The fixtures must produce the same Arrow types they produce today, because several tests are about those types: `human.rs` snapshots `type "Int32" -> "Int64"`, `cells.rs` asserts `type_changed` across `Int32`, `Int64`, and `Float64`, and `input.rs` enumerates every supported Arrow representation. Any snapshot change during this step is a bug in the conversion, not an update to accept.

`tests/common/mod.rs` keeps `TempDir`, `write_parquet`, and `write_parquet_batches`. They are Parquet fixtures shared between integration test binaries, not table construction, and moving them would pull a `parquet` dependency into `test-support` for no benefit to this step.

`src/input.rs` and `src/compare.rs` keep the Arrow imports their subjects require. `rejects_every_unsupported_type_family` enumerates `DataType` variants and `comparison_matrix_accepts_only_compatible_pairs` enumerates `DataType` pairs; both are about the Arrow type system and should keep naming it directly, and `dictionary_strings_use_logical_values` keeps building its dictionary by hand for the reason given under conversion notes. What those modules lose everywhere else is the `Arc::new(SomeArray::from(vec![...]))` wrapping around their data.

Unit tests stay inline in their production module, as the settled conventions require. `test-support` is a fixture library, not a home for tests.

## Explicitly deferred

* Moving `TempDir` and the Parquet writers out of `tests/common/mod.rs`.
* Any change to what the tests assert, including the granularity or naming of existing tests.
* Any change to production code. `src/lib.rs` and the eleven production modules are untouched apart from their inline test modules.
* Extending the fixture vocabulary beyond the Arrow types the current fixtures use; new types arrive with the steps that need them.

# Fixture helper design

## Where it lives

The helper has to be reachable from inline `#[cfg(test)]` modules under `src/` and from integration tests under `tests/`. A `#[cfg(test)]` module inside the crate is invisible to integration tests, which link the crate compiled without `cfg(test)`; a feature-gated `pub mod` would put fixture code in the public API and would have to stay compiled out of normal builds by hand. A separate crate consumed as a path dev-dependency has neither problem: dev-dependencies are available to both kinds of test target and to neither the library nor the binary.

The root `Cargo.toml` becomes a workspace root with one member. CI already runs `cargo build --workspace --all-targets` and `cargo test --workspace`, so no workflow change is needed.

`test-support` does not re-export `RecordBatch` or `ArrayRef`. Re-exporting the types that appear in a crate's own API is conventional where it saves the caller a dependency or pins them to one version of a type, and neither applies here: `arrow-array` is an ordinary dependency of the package, so both test targets can already name it, and one workspace on one pinned version has no skew to prevent. What would be left is cosmetic — making the word `arrow` disappear from imports — at the price of misstating where a well-known type comes from. Five local helpers name `RecordBatch` or `ArrayRef` in their signatures and import it from `arrow_array` directly, which is honest and costs one line each.

## Syntax

A column is a name, an optional type annotation, and a list of values:

```rust
let old = table! {
    "id" => [1, 2, 3],
    "label" => ["a", "b", "c"],
    "small" => i32[10, 20, 30],
    "score" => [Some(1.5), None],
    "blank" => i64[],
};
```

Names are string literals, which is what a column name is, and which keeps unusual names such as `"line\n\"quoted\""` in the same form as every other fixture. `=>` separates the name from the values so the two halves of a column read apart at a glance.

The annotation is a bare token immediately before the bracket, so the common case carries no annotation at all and the exceptional case reads as a typed array literal. `column!` takes the same right-hand side and produces an `ArrayRef` for the tests that compare arrays rather than tables:

```rust
let (old, new) = values(column!(["1", "1.0"]), column!([1, 1]));
```

Two further forms cover the tables that have no columns to infer from. `table! {}` is the schema-preserving empty batch that `common::empty_batch` and `rows.rs`'s empty branch build today, and `rows_without_columns(2)` is the columnless two-row batch that `key.rs` builds with `RecordBatchOptions`.

## Type inference

Each value list is a Rust array literal, so all its elements share one Rust type, and that type determines the Arrow type. A `CellValue` trait maps each supported Rust type to a `Kind` and to a `Cell`; the blanket implementation for `Option<T>` inherits `T`'s kind, so a column of nulls still knows what it is:

| Rust value type | `Kind` | Arrow type without annotation |
| --- | --- | --- |
| `bool` | `Bool` | `Boolean` |
| `i32`, `i64` | `Int` | `Int64` |
| `u64` | `Int` | `Int64` |
| `f32`, `f64` | `Double` | `Float64` |
| `&str` | `Text` | `Utf8` |
| `Option<T>` | `T`'s kind | `T`'s type, with nulls where `None` |

Integer literals fall back to `i32` and float literals to `f64` when nothing else constrains them, and both map to the `Int` and `Double` kinds, so `[1, 2, 3]` builds the `Int64` column the fixtures overwhelmingly want and `[1.5]` builds a `Float64` one. `u64` is implemented because one fixture needs values above `i64::MAX`; cells hold integers as `i128` so that range survives to the builder. This was verified against `rustc` before the plan was written, including the `Option` and empty-list forms.

An annotation overrides the inferred Arrow type for the column's kind. The vocabulary is exactly what the current fixtures need: `bool`, `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `str`, `large_str`, `dict`, and `binary`. `dict` builds a `Dictionary(Int8, Utf8)` column whose dictionary holds the distinct values in first-appearance order, so its keys carry no information beyond the logical values; `binary` builds a `Binary` column from the bytes of its string values.

Because the dictionary form encodes rather than transcribes, it is only correct for a test whose subject is the logical values. A test that is about the encoding itself keeps building its `DictionaryArray` by hand, as `src/compare.rs` does below. Assigning keys by sorted rather than first-appearance order was considered and rejected: it happens to reproduce today's encoding for one fixture, which would make that test's strength depend on an incidental helper policy rather than on what the test says.

An unannotated empty list has no element type, which the compiler rejects; the message to the test author is that an empty column must say what it is, as `"blank" => i64[]`. Fields are created nullable, exactly as all six current helpers create them, and a `table!` whose columns disagree in length panics through `RecordBatch::try_new`.

## Failure behavior

Three misuses are caught at run time and panic: an unknown annotation, an annotation whose Arrow type cannot hold the kind it was given (`dict[1, 2]`), and a value outside the annotated width (`u8[300]`). The builder raises these, so its message names the annotation and the offending value but cannot name a column. `table!` passes each column's name into the builder as context and `column!` passes none, so a table failure reads `column "id": 4294967296 does not fit u32` and the same failure from `column!` drops the prefix. The two misuses the compiler already catches — an unannotated empty list, and a value type with no `CellValue` implementation — stay compile errors and are documented rather than tested.

## The helper's own tests

`test-support` gets an inline `#[cfg(test)] mod tests`. A silently wrong helper would weaken every fixture in the suite at once, so its behavior is pinned directly rather than only through its callers:

* the inferred Arrow type for each kind, and the resulting type for each annotation in the vocabulary;
* `None` placement within a column, a typed empty column, the zero-column table, `rows_without_columns`, row counts, and field nullability;
* the exact contents of the two annotations that transform their values rather than storing them — that `dict["b", "a", "b"]` has keys `[0, 1, 0]` over the dictionary `["b", "a"]`, and that `binary["a"]` holds `b"a"` — since a mistake there would be invisible in the `DataType` alone; and
* a `#[should_panic(expected = ...)]` test for each of the three run-time misuses, including one through `table!` to pin the column-name prefix and one through `column!` to pin its absence.

# Conversion notes

Most fixtures convert mechanically. The cases that need a decision:

* `key.rs`'s `guessing_breaks_ties_by_old_column_order` builds its two identical columns through a `shared()` closure that exists only to clone an `ArrayRef`; the columns become two literal lists.
* `key.rs`'s `rows_without_columns_leave_nothing_to_guess` builds its batch with `RecordBatchOptions::new().with_row_count(Some(2))`, which becomes the `rows_without_columns(2)` helper and lets the module drop its last Arrow import.
* `rows.rs`'s local helper special-cases an empty column list to build a schema-less batch; that branch becomes `table! {}` at the one call site that needs it, and the special case disappears with the helper.
* `input.rs`'s `normalizes_every_supported_arrow_representation` becomes a single `table!` with one annotated column per representation, which is the test's actual subject stated directly. Its dictionary column becomes `dict["a"]`, one row like every other column in that table: today's fixture stores one key over a two-value dictionary, and the unused second value is invisible to the assertion, which only reads the column's normalized type. `dict["a", "b"]` would be two rows against every other column's one and would fail in `RecordBatch::try_new`. `rejects_unsupported_types` becomes `binary["a"]`.
* `input.rs`'s `rejects_first_out_of_range_unsigned_integer` becomes `u64[Some(1), None, Some(i64::MAX as u64 + 1), Some(u64::MAX)]`.
* `input.rs`'s `concatenates_batches_in_input_order` and the integration test `parquet_batches_become_one_table_in_file_order` read their result back by downcasting to `Int64Array`, which would keep an array type imported into two tests that are about batch concatenation rather than about Arrow. Both compare the resulting column against a fixture column instead, which says the same thing about the same values and additionally compares the column's type and null mask.
* `compare.rs` builds arrays rather than tables, so its fixtures become `column!` and lose their `std::sync::Arc::new` wrapping. Its bit-pattern and boundary values — `f64::from_bits(...)`, `-0.0`, `i64::MIN`, `None::<&str>` — are ordinary expressions inside the list and stay exactly as they read today.
* `compare.rs`'s `dictionary_strings_use_logical_values` is the one fixture that keeps its explicit `DictionaryArray` construction. It stores keys `[1, 0]` over the dictionary `["a", "b"]` precisely so that storage order and logical order disagree, which is what makes it evidence that canonicalization follows the keys. Rebuilt as `dict["b", "a"]` the helper would emit keys `[0, 1]` over `["b", "a"]`, where an implementation that ignored the keys and read the dictionary positionally would still pass. Inventing a second syntax that spells out keys and dictionary separately would serve this one test only; the test is about the encoding, so it states the encoding. This is the reason `compare.rs` keeps importing `Arc`, `Int8Type`, `Int8Array`, `DictionaryArray`, and `StringArray`, and a comment on the fixture records why it was not converted.
* The integration tests keep their `mod common;` for `TempDir` and the Parquet writers, but their `common::batch([...])` calls become `table! { ... }` and `common::empty_batch()` becomes `table! {}`.

# Verification

The refactor is behavior-preserving, so the evidence is that nothing moved:

* Every inline snapshot in `src/human.rs` and `tests/cli.rs` passes unchanged. Because insta snapshots are inline, an accidental type change shows up as a failing assertion rather than a silent update, and the diff for this step must contain no snapshot edits.
* The determinism tests in `tests/diff.rs` continue to assert structural equality of repeated `Diff` values and byte equality of their rendered output, on fixtures built through the new helper.
* The test count is unchanged apart from the new `test-support` unit tests; no test is added, removed, renamed, or merged.
* The acceptance pass runs `cargo build --workspace --all-targets`, which is what CI runs and what confirms the new workspace layout builds every target; `cargo test --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; and `git diff --check`.

# Definition of done

This step is complete when:

* one `test-support` workspace member provides `table!`, `column!`, and `rows_without_columns`, with its own unit tests;
* no `fn table(columns: Vec<(&str, ArrayRef)>)` helper remains anywhere in the repository, and `common::batch` and `common::empty_batch` are gone;
* every fixture in `src/` and `tests/` is built through the shared helper, except where an Arrow type is the subject of the assertion;
* the only surviving Arrow imports are the `RecordBatch` or `ArrayRef` named by a module's own helper signature, the `DataType` enumerated by `src/input.rs` and `src/compare.rs`, and the array types `src/compare.rs` needs to build its one hand-encoded dictionary; the suite's one remaining `std::sync::Arc` import is `src/input.rs`'s, which wraps the `Field` inside the `List` and `Struct` types that test enumerates;
* every assertion, test name, and inline snapshot is unchanged, and every fixture still produces the Arrow types it produced before this step; and
* the full test suite, strict Clippy, formatting, and diff checks pass across the workspace, and repeated runs still produce byte-identical output.
