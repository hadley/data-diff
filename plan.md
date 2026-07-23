---
title: data-diff implementation plan
---

# MVP todo

- [x] **Scaffold the library, CLI, and test harness.** Create one Rust package with
  a library that accepts two Arrow `RecordBatch` values plus options and a thin
  `data-diff` binary that reads Parquet files. Add concise builders for in-memory
  test tables, a placeholder result assertion, and temporary Parquet fixtures.
  The initial `cargo test` should exercise one library call and one CLI
  invocation; complete structured diff assertion helpers arrive with the result
  model. Start from the shared dependency baseline described below.
- [x] **Define the result model.** Add types for schemas, column identities, keys,
  row matches, ordering changes, changed cells, and errors. Implement one-based
  collapsed coordinates and deterministic JSON serialization, with focused unit
  tests for every coordinate shape.
- [x] **Load and validate inputs.** Read both Parquet files into memory, reject
  duplicate top-level names and unsupported types, concatenate row groups into
  one `RecordBatch` per side, eagerly validate that unsigned values fit in
  `int64`, and retain source and normalized schemas. Test each supported physical
  representation and each validation error without involving reconciliation.
- [x] **Build the shared comparison layer.** Implement comparison plans,
  canonical values, equality, and stable hashing for the MVP type matrix. Establish
  with table-driven tests that equal values always hash equally, hash matches are
  verified by equality, and null, `NaN`, signed zero, parsing, and exact
  integer/double comparisons follow `design.md`.
- [x] **Parse and validate declared keys.** Require `--key`, initially accepting
  only comma-separated, same-name components. Check presence, compatible types,
  missing values, `NaN`, and uniqueness after canonicalization. Cover simple and
  compound keys, including cross-type components. Any duplicate on either side
  fails in the MVP; distinguish a broken old key from unsupported new-side fanout.
- [x] **Match rows.** Hash compound keys, verify candidate matches, and classify
  rows as added, dropped, or one-to-one matched. Order matched pairs by their old
  row positions and test empty inputs, disjoint keys, and reordered rows.
- [x] **Reconcile the MVP schema.** Give same-name columns identities, classify
  unmatched columns as additions or drops, and independently record source-type
  changes. Reject an identified same-name pair when its types are incompatible.
  Do not infer renames.
- [x] **Detect ordering changes.** Implement the deterministic LIS-based minimum
  moved set for identified columns and matched rows, excluding additions and
  drops. Test insertions, deletions, rotations, ties, and already ordered inputs.
- [x] **Compare cells.** Compare identified, non-key columns over matched rows;
  retain every changed cell while avoiding per-cell changes for added/dropped
  rows or columns. Record an evidence-level column edit for every identified
  column with a source-type change or changed cell, including type-only key
  columns; row edits wait for post-MVP summarization.
- [ ] **Complete the JSON and CLI path.** Emit deterministic, pretty JSON for a
  successful comparison and concise contextual errors for failures. Add small
  end-to-end fixtures covering unchanged data, combined schema/row/value changes,
  type-only changes, empty inputs, and invalid input.
- [ ] **MVP acceptance pass.** Run the complete test suite, confirm repeated runs
  are byte-for-byte deterministic, document the supported CLI and limitations,
  and manually inspect one representative diff before declaring the MVP complete.

# Goal

The MVP proves the complete path from two Parquet files to an inspectable JSON
description of their differences:

```text
Parquet files
    → validated typed tables
    → normalized comparison plans
    → validated key
    → row and column identities
    → ordering and cell changes
    → structured JSON
```

It is an experimentation tool for refining reconciliation, not yet the final
interactive UI. Correctness, deterministic output, and tests that expose the
behavior clearly are more important than large-data performance.

# Architecture

Use a single Rust package with two entry points:

* The library owns all input-independent reconciliation types and algorithms.
  Its top-level function accepts two in-memory Arrow `RecordBatch` values and
  structured options, and returns either a structured diff or a typed error.
* The binary parses arguments, loads the two Parquet files, calls the library,
  and writes JSON. It should contain no reconciliation logic.

The loader reads every row group and concatenates its batches in file order into
one `RecordBatch` per input, preserving the schema even when there are no rows.
All row coordinates therefore index this single logical batch.

Keep stages separate enough to test directly: input validation, normalization,
comparison planning, key validation, row matching, schema reconciliation,
ordering, cell comparison, and serialization. Intermediate types should express
stage invariants so later stages do not repeatedly validate earlier assumptions.

The MVP may load both datasets fully into memory. It does not need sampling,
streaming, caching across runs, elapsed-time limits, or partial results.

# Dependency alignment

Reuse the established choices in
`~/Documents/data-dict/data-dict/Cargo.toml` where they fit:

* Rust edition 2024.
* `clap` 4 with `derive` for the CLI.
* `parquet` 54 with default features disabled and the same `snap`, `flate2`,
  `lz4`, `zstd`, and `brotli` codec features. Also enable its `arrow` feature,
  which `data-diff` needs to load `RecordBatch` values.
* `serde` 1 with `derive` and `serde_json` 1 for the result model and output.
* `insta` 1 and `indoc` 2 as development dependencies for compact boundary
  snapshots and readable inline fixtures.

Use Arrow crates from the same 54 release family as `parquet`: `arrow-array`
and `arrow-schema` for the table boundary, plus `arrow-select` for concatenating
batches. Keep these versions aligned rather than allowing two Arrow release
families into the dependency graph.

Follow `data-dict`'s test style where appropriate: invoke the binary with
`env!("CARGO_BIN_EXE_data-diff")`, normalize JSON through `serde_json::Value`
before snapshotting it, sanitize variable paths, and keep fixture construction
next to the behavior it exercises. Continue to use typed assertions for
algorithm unit tests; snapshots are reserved for compact CLI and serialization
boundaries.

XXH3 is a `data-diff`-specific requirement, so add the smallest crate that
provides seeded XXH3-128. Do not adopt `rayon`, `hashbrown`, `criterion`, or
other `data-dict` dependencies until the corresponding parallelism, performance,
or benchmarking need appears. When either project upgrades a shared dependency,
keep their compatible version and feature choices aligned where practical.

# Test infrastructure

Establish the test vocabulary before implementing reconciliation. A test should
show its inputs, operation, and expected result together without a large fixture
or snapshot.

Provide these helpers:

* A compact builder for named Arrow columns and a single `RecordBatch`, including
  nulls, dictionary strings, and explicit source types.
* A `diff_tables()` helper that runs the library directly, avoiding Parquet and
  the CLI in algorithm tests.
* Constructors for expected row, column, and cell coordinates, so coordinate
  assertions do not become JSON punctuation tests.
* A temporary-Parquet writer for the small set of loader and end-to-end tests.
* A CLI runner that captures status, stdout, and stderr.

Prefer assertions on typed results. Test JSON only at the serialization boundary,
using small `serde_json::Value` expectations rather than large golden files.
Use table-driven cases for the comparison matrix and validation rules. Keep most
fixtures under roughly five rows and five columns; add a larger fixture only
when the size itself is the behavior under test.

Every implementation stage should add:

1. Narrow unit tests for its rules and edge cases.
2. At least one integration test showing that the new result survives the
   downstream stages already implemented.
3. A determinism test when iteration, hashing, or tie-breaking is involved.

Important properties to test directly include:

* equality implies identical canonical hashes;
* hash collisions cannot create false equality, using an injected test hasher
  that deliberately collides;
* every input row receives exactly one MVP classification;
* every resolved column endpoint appears in at most one identity;
* additions and drops do not manufacture changed cells;
* reported moves are minimal and deterministic;
* coordinates always refer to the original inputs;
* output arrays have stable coordinate/input order; and
* identical inputs and options produce byte-identical JSON.

# Comparison infrastructure

Derive a comparison plan from each pair of column types and use it consistently
for key validation, row matching, and cell comparison. Canonicalization, hashing,
and equality belong to that plan: values that compare equal must have identical
canonical hashes. A hash match only identifies candidates; always verify them
with exact equality so collisions cannot change the result.

Use XXH3-128 with seed 0 as `stable-hash-v1`. Hash an explicit canonical byte
encoding rather than Rust memory:

* fixed tags distinguish value categories, including separate null and `NaN`
  tags;
* integers and floating-point values use fixed little-endian bytes;
* every `NaN` has one representation and both signed zeros use one
  representation;
* strings are length-prefixed bytes; and
* compound keys contain length-prefixed canonical components.

The same logical input must hash identically across processes and platforms.
Changing the algorithm or byte encoding requires a new internal hash version.
Keep the hasher behind a narrow internal interface so tests can force collisions
without weakening production hashing.

# MVP interface

The initial command is:

```console
data-diff old.parquet new.parquet --key customer_id,date,region
```

`--key` is required and accepts a comma-separated list of bare column names.
Each name identifies the same-named column on both sides. Empty components,
duplicate components, and `old/new` paired components are errors in the MVP.
The library should represent key components as structured old/new column
identities so paired names can be added later without changing its core model.

Successful comparisons write pretty JSON to stdout. Failures return a non-zero
status, write no partial JSON, and identify the side, column or key component,
and offending type or positions where applicable. Exact wording is not a stable
interface, but tests should assert the typed library error and the CLI's essential
context.

# MVP result

The internal result should preserve evidence rather than prematurely reduce it:

* original and normalized schemas;
* resolved same-name column identities;
* added, dropped, and edited columns;
* the declared key;
* added, dropped, and matched rows;
* row and column ordering changes; and
* the complete set of changed cells.

For the MVP, `columns.edited` is an evidence-level rollup, not the minimum edit
summary: include every identified column with a source-type change or at least
one changed cell, coalescing both facts into one entry. There is no
`rows.edited` field yet because row edits arise only from choosing a row/column
summary. Post-MVP summarization adds a separate `summary`; it does not replace
the evidence-level column entries.

Coordinates are one-based positions in the original inputs. Use the collapsing
rules from `design.md`: an unchanged old/new position is one integer; a moved
position is `[old, new]`; and a changed cell is `[row, column]` when both
coordinates agree or `[[old_row, old_column], [new_row, new_column]]` otherwise.
The same collapsed column coordinate is used in `columns.identities`,
`columns.edited`, and `key.columns`. Additions and drops are separate arrays and
need no sentinel coordinate.

`rows.matched` is the complete old-to-new position mapping, emitted in old-row
order. By contrast, `order.rows` contains only the LCS-minimal subset whose
relative order changed. Inserting a row can therefore change many mapped
positions while leaving `order.rows` empty. The corresponding distinction
applies to column identities and `order.columns`.

Emit arrays deterministically in input/coordinate order. Use an empty array when
an MVP stage ran and found no changes. Do not add placeholder `null` fields for
post-MVP stages such as fanout, inference issues, or edit summarization. The JSON
is an internal experimental representation and is not versioned.

For example, a same-name MVP comparison might produce:

```json
{
  "schemas": {
    "old": [
      {"name": "id", "source_type": "INT64", "normalized_type": "int64"},
      {"name": "value", "source_type": "DOUBLE", "normalized_type": "double"}
    ],
    "new": [
      {"name": "id", "source_type": "INT64", "normalized_type": "int64"},
      {"name": "value", "source_type": "DOUBLE", "normalized_type": "double"},
      {"name": "note", "source_type": "UTF8", "normalized_type": "string"}
    ]
  },
  "columns": {
    "identities": [1, 2],
    "added": [3],
    "dropped": [],
    "edited": [
      {"column": 2, "type_changed": false, "values_changed": true}
    ]
  },
  "key": {
    "basis": "declared",
    "columns": [1]
  },
  "rows": {
    "added": [3],
    "dropped": [],
    "matched": [1, 2]
  },
  "order": {
    "columns": [],
    "rows": []
  },
  "cells": [[2, 2]]
}
```

For a non-trivial coordinate example, suppose the two identified columns and
two matched rows both exchange positions, and the value cell changes:

```json
{
  "columns": {
    "identities": [[1, 2], [2, 1]],
    "edited": [
      {"column": [2, 1], "type_changed": false, "values_changed": true}
    ]
  },
  "key": {"basis": "declared", "columns": [[1, 2]]},
  "rows": {"matched": [[1, 2], [2, 1]]},
  "order": {"columns": [[2, 1]], "rows": [[2, 1]]},
  "cells": [[[1, 2], [2, 1]]]
}
```

This fragment shows coordinate shapes only; omitted fields follow the complete
result above.

# MVP behavior

The MVP supports booleans; signed and unsigned integers whose values fit in
`int64`; `float32` and `float64`; UTF-8 and dictionary-encoded strings; and
nulls within those columns. It rejects the entire comparison when either input
contains a decimal, binary, temporal, interval, nested, or other unsupported
column. Dictionary encoding is supported only when its logical value type is
UTF-8 string; dictionaries of numeric or other values are unsupported.

Validate unsigned integer ranges eagerly while loading. Report the side, column,
source type, and first one-based row containing a value above `i64::MAX`.

The following outcomes are required:

| Condition | Outcome |
|---|---|
| Unreadable or invalid Parquet | Fail before reconciliation |
| Duplicate top-level name | Fail with side, exact name, and one-based positions |
| Unsupported type | Fail with side, column, and source type |
| Unsigned integer above `i64::MAX` | Fail with side, column, source type, and first row |
| Missing `--key` | Fail |
| Paired, missing, or duplicate key component | Fail with the component |
| Incompatible key types | Fail with both types |
| Null or `NaN` in a key | Fail key validation |
| Key duplicated in `old` after canonicalization | Fail with `non_unique_old` |
| Key duplicated only in `new` | Fail with unsupported-fanout context |
| Valid key with no shared values | Classify all old rows as drops and all new rows as additions |
| One empty input | Classify every row on the other side atomically; still compare schemas |
| Both inputs empty | Emit no row or cell changes; still compare schemas |
| Compatible source type changes but values agree | Emit a type-only column edit |
| Key source type changes but canonical values agree | Emit a type-only column edit and no key cells |
| Same-name non-key columns have incompatible types | Fail with both columns and types |
| Added or dropped row/column | Emit the atomic event, not per-cell changes |
| Valid inputs and key | Emit deterministic coordinate-only JSON |

Implement comparisons, canonicalization, string parsing, key rules, and ordering
exactly as specified in `design.md`; the plan intentionally does not duplicate
those algorithms.

# Definition of done

The MVP is complete when:

* all checklist items are checked;
* the supported behavior table is covered by clear tests;
* unit tests exercise algorithms without filesystem or CLI setup;
* end-to-end tests prove both success and failure paths through real Parquet
  files;
* no post-MVP inference is required to obtain a complete valid result;
* output is deterministic across repeated runs; and
* the CLI can explain one representative mixed data change through its JSON.

# After the MVP

Add reconciliation features in dependency order, giving each the same combination
of isolated fixtures, integration coverage, and determinism checks:

1. Summarize changed cells with an exact minimum bipartite vertex cover. Emit a
   separate summary with `optimal: true`; bounded fallback is added only in step
   7, when this field may become false.
2. Guess eligible single-column keys and allow users to override the guess.
3. Add paired key components and validated rename/add/drop/edit hints.
4. Infer exact renames from aligned matched rows.
5. Support bounded declared-key fanout while keeping fanout cells separate.
6. Add approximate rename inference and then swap detection, initially examining
   all matched rows; step 7 replaces the full set with deterministic sampling
   when required.
7. Benchmark the complete pipeline and introduce sampling, computation budgets,
   valid partial results, and incomplete-stage reporting.
8. Expand scalar type support, then design a bounded large-data execution model
   and interactive UI.

Defer decisions about hint syntax, UI presentation, thresholds, and concrete
budgets until the prerequisite behavior exists and can be benchmarked. Preserve
the central invariants: deterministic reconciliation, no inferred event without
underlying evidence, and continued access to the complete cell-level diff.
