---
title: Opaque columns
---

# Todo

- [x] **Widen what the reader admits.** `validate_tables` stops refusing types outside the four normalized ones. A column is admitted exactly when the canonical row encoding can represent it, probed by constructing the encoder for its field, so "unsupported" narrows to what that encoding genuinely cannot hold and there is no second type list to keep honest. `NormalizedType` gains `Opaque` for everything admitted this way; `DiffError::UnsupportedColumn` stays, for the narrowed remainder, as does `IntegerOutOfRange`.
- [x] **Give opaque pairs a comparison.** `ComparisonPlan::new` returns `Option<Self>` again: the MVP matrix is unchanged, a pair of *identical* source types outside it gets a new `Opaque` domain, and every other pair — opaque against a different opaque, opaque against a normalized type — has no plan. Canonicalization maps a null to `CanonicalValue::Null` first, so the null rules stay uniform, and encodes every other value to `CanonicalValue::Opaque(bytes)` via an `arrow-row` converter built per side from the column's own `DataType` — the two sides' types are identical by construction, and the row encoding hydrates dictionaries to their underlying values, so equal values in different arrays get equal bytes without any shared converter state; a test with differently-interned dictionaries pins that property. `ComparisonPlan` stays `Copy + Eq + Hash` with an information-free `Opaque` kind, which the agreement cache tolerates because its key already carries the column index and an opaque column only ever has the one plan. `arrow-row = "54"` joins the dependencies.
- [x] **Revive the incomparable-pair machinery.** The reading recorded in `design.md` when booleans joined the numeric domain, now implemented: name matching in `reconcile_schema` declines a planless pair, which falls out as `col_drop()` plus `col_add()`; `claimed_identities` declines a planless declared pair; `RejectionReason::IncompatibleTypes` returns for a declared key; `IssueKind::HintIncompatibleTypes` and its `incompatible:` rendering return for rename hints; `guess_key` and rename inference skip planless candidates; `incompatible` rejoins the grammar's fixed field set. `reconcile_schema` stays infallible — declining is not an error — so `run_pass` keeps its shape.
- [x] **Restore the infallible call sites' `expect`s.** `Option`-returning plans touch three mechanical spots the boolean step simplified: `plan_for` in `src/rename.rs` returns `Option` and its callers filter, since a candidate pair may genuinely have no plan; `plan_for` in `src/swap.rs` and the construction in `src/cells.rs` regain `expect`s stating why `None` is impossible there — both read pairs out of the map, and every claim site checks the plan before claiming, so an identity without one cannot exist.
- [x] **Let the rest fall out and prove it.** No stage below key resolution changes semantically: opaque canonical values hash and compare, so an opaque column can be a declared or guessed key, exact and approximate rename inference relate identical-typed opaque candidates, swap inference exchanges them, and cells, ordering, change mass, and reconsideration consume them untouched. Tests assert each of these paths rather than trusting the argument.
- [x] **Extend the fixtures.** `test-support`'s annotation table gains `date32`, `ts_ms` (timestamp, milliseconds, UTC), and `ts_ms_naive` (no timezone), reusing the integer cell path, so tests and `examples/generate_demo.rs` can build temporal columns.
- [x] **Update `design.md`.** The normalized-types section replaces "unsupported by the MVP" with the opaque regime; comparison semantics gains the identical-source-type row; the recorded future-tense reading becomes present tense; the hint issue kinds, key rejection reasons, and grammar field set take their revived entries; the one-sided section's deferred-validation note is resolved, summaries now admitting whatever the reader does.
- [x] **Update `README.md`.** The output section notes that any column the reader can load participates, comparing only against its identical type until given semantics, and `incompatible_types` rejoins the `key_invalid()` reasons.
- [x] **Refresh the demo.** A new section with a `temporal-*.parquet` fixture pair written by `examples/generate_demo.rs`: a date column keyed and edited like any other, and a same-name column retyped from integer to date reporting a type-only edit. Transcripts held to real output by `tests/readme.rs`, no orphaned fixtures.
- [x] **Requeue the remainder.** `plan-next.md` loses the broader-types item and gains, ahead of benchmarking, the promotion step: semantic cross-type rules for the common opaque types — timestamps across units and timezones, date ↔ timestamp, decimal ↔ numeric when exact — replacing the identical-type-only regime this step gives them.
- [x] **Keep incomparable pairs as identities.** Added in review, reversing this plan's drop-and-add reading: the owner directed that an incomparable pair — same-named, hinted, or declared — stays one column, reported as a type-only `col_edit()` with its values never compared: no `changes:` count, no cells, no contribution to summaries or change mass. The principle splits where the earlier one did not: identity does not require comparability, but everything that measures values does, so keys, guessing, and rename inference still refuse planless pairs. Rename hints across incomparable pairs are accordingly *honoured* now, retiring `hint_incompatible_types` and the `incompatible` field again; the `incompatible_types` key rejection stays, keys being about values, with the asserted identity surviving it. Swap inference treats an unmeasurable same-name pair as rewritten vacuously, so an exchange that traded two columns' types is recovered through its identical-typed crossings. A type-only edit now means "values equal" or "values incomparable", and the line's own types say which. A review fix rode along: opaque nulls are read from the logical null mask, so a valid dictionary key pointing at a null value is a null of the column.
- [x] **Cover it.** Unit tests for the opaque domain's equality, hashing, null handling, and cross-array byte stability; integration coverage in `tests/diff.rs` for each path under Verification; CLI snapshots in `tests/cli.rs`; determinism checks.
- [x] **Complete the acceptance pass.** `cargo build --workspace --all-targets`, `cargo test --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`, and byte-identical repeated runs.

# Goal

A Parquet file with a date column cannot be diffed at all today: `validate_tables` refuses the whole comparison over one `Date32` column, and the same goes for timestamps, decimals, binary — the types real datasets carry everywhere. That is the tool's largest practical gap, and closing most of it does not require deciding what a timestamp equals across units, because the overwhelmingly common case is a column whose type did not change: two versions of a file where the date column is still a date column, and equality within one type is exact whatever the type means.

This step admits every column the canonical row encoding can represent and gives it exactly that much comparison: an *opaque* column compares only against its identical source type, by canonical bytes, and is otherwise incomparable. Everything above the comparison falls out unchanged — an opaque column can be the key, can be edited, renamed, swapped, and counted — so this diff, impossible today, just works:

```console
$ data-diff demo/temporal-old.parquet demo/temporal-new.parquet
table_key([id], basis: guessed, overlap: 1.00)
col_edit(flag, type: Int64 -> Date32)
row_edit(2, changes: 1)
```

The `row_edit()` is an edited date comparing exactly within its type; the `col_edit()` is the same fixture's `flag` column retyped from integer to date — one column whose type changed, its values never compared.

Admitting types without cross-type rules makes incomparable pairs constructible for the first time since booleans joined the numeric domain. As revised in review (see the Todo item), an incomparable pair keeps its identity and loses only its value story — a type-only `col_edit()`, no counts, no cells — while everything that measures values refuses it: a declared key on one is rejected as `incompatible_types`, and guessing and inference never propose one. A timestamp with a timezone and one without are two different types, and reporting the type change with the values uncompared is the correct answer until the promotion step decides what the comparison means.

# Scope

## What changes

- `src/input.rs`: admission by row-encodability; `NormalizedType::Opaque`.
- `src/compare.rs`: `Option`-returning plans, the `Opaque` domain, `CanonicalValue::Opaque`.
- `src/schema.rs`, `src/key.rs`, `src/hint.rs`, `src/model.rs`, `src/human.rs`: the revived incomparable-pair machinery.
- `src/rename.rs`: planless candidates skipped.
- `src/cells.rs`, `src/swap.rs`: mechanical — the plan construction regains its invariant-stating `expect`s.
- `test-support`: temporal annotations.
- `Cargo.toml`: `arrow-row`.
- `design.md`, `README.md`, `demo/README.md`, `examples/generate_demo.rs`, `plan-next.md`, and the test suites.

## What stays and why

- **The MVP matrix.** No existing pair's rule changes, and no opaque type gains a cross-type rule here — that is the promotion step's whole business, and each rule (timestamp units, date at midnight, decimal exactness) deserves its own argument rather than riding in on the machinery.
- **Everything below key resolution, semantically.** Row matching, rename and swap inference, ordering, cells, change mass, summarization, and reconsideration are all generic over canonical values; the only code they change is the mechanical `expect`s named above, and the fall-out item is verifying the semantic claim rather than acting on it.
- **`run_pass`'s infallibility.** Declining an identity is not an error; the fatal `IncompatibleColumns` of the pre-boolean era stays retired.
- **Null and `NaN` rules.** Nulls canonicalize to `Null` before opaque encoding, so null/null agrees, null/present disagrees, and nulls invalidate keys — uniformly. `NaN`-invalidates-keys remains a rule about the `double` domain: a `NaN` buried in an opaque value (a `Float16`, a struct field) is just bytes, which is part of what opaque means and is documented rather than special-cased.

## Explicitly deferred

- **Semantic promotion of the common types.** Cross-unit and cross-timezone timestamp comparison, date ↔ timestamp, decimal ↔ integer/double when exact — requeued, ahead of benchmarking, as the owner directed.
- **Value rendering for opaque types.** Nothing prints cell values today, so nothing new arises; when the UI shows values, opaque ones need a display story.
- **Types the row encoding cannot hold.** Unions and whatever else the probe refuses stay fatal `UnsupportedColumn`s; softening those further has no evident use case.
- **String parsing against temporal types.** `"2026-08-03"` versus a date column is a promotion-step question, and maybe not even that.

# Design

## Why opaque-first is the right cut

The queue item bundled two things: admitting the common types, and deciding their cross-type semantics. They separate cleanly, and the first is nearly all of the user value. A dataset's date column retyped to integer is rare; a dataset's date column *existing* is universal, and today it kills the whole comparison. Identical-type equality is also the one comparison that needs no per-type argument: whatever a `Timestamp(ms, UTC)` means, two of them are equal exactly when their values are, and the canonical row encoding says so in bytes. The promotion step then has a stable foundation to argue each cross-type rule on its merits — and this step's incomparable machinery is precisely the fallback those arguments will relax case by case.

The alternative cut — machinery plus full temporal semantics in one step — was rejected as two PRs wearing one coat: every cross-type rule is a separate design decision with its own edges (unit overflow, timezone meaning, midnight), none of which the machinery needs.

## The opaque regime

A column is *opaque* when its type is outside the four normalized ones and the canonical row encoding can represent it. Admission is probed, not listed: `validate_tables` asks the encoder whether it can hold the field, so the set of admitted types is exactly the set the comparison can serve, and it grows with the encoding rather than with a list in this codebase. An opaque column has a comparison plan only against its *identical* source type — exact `DataType` equality, timezone, unit, precision and scale included — and its values canonicalize to their encoded bytes, equal exactly when the values are. Hashing derives from the bytes, so keys, frequency counts, and exact rename digests work unmodified; agreement and κ are already defined over canonical-value equality and frequencies, so approximate inference and swap crossings work unmodified too.

Byte equality must hold *across* the two files' arrays, not merely within one, and dictionary encoding is where that could break: two files interning the same values in different orders must still encode equal rows to equal bytes. The row encoding provides this by construction — it hydrates dictionary arrays to their underlying values during conversion, so each side can build its own converter from the identical `DataType` and the bytes agree without shared state. That keeps `canonicalize_old` and `canonicalize_new` independent calls and `ComparisonPlan` a `Copy + Eq + Hash` cache key, exactly as they are; a shared-converter design was considered and dropped as a refactor the hydration property makes unnecessary. The property is load-bearing rather than assumed, so a test with two differently-ordered dictionaries pins it.

Identical-type-only is deliberately strict. `Timestamp(ms)` against `Timestamp(ns)` is very likely the same instant stream retyped — and the pair is incomparable here, reading as a type-only edit with its values uncompared, because "very likely" is a semantic judgement the promotion step should make with unit-conversion exactness in hand, not something byte comparison should shrug at. The output is honest either way: the columns are reported unrelated, never wrongly equal or wrongly changed.

## The machinery returns as recorded

`design.md` recorded a stronger reading when the boolean step made incomparable pairs unconstructible — identity requires comparability, decline the pair wherever claimed — and this step first implemented it before review narrowed it (see the Todo item): identity survives, and only the value story goes. What remains of the recorded machinery is exactly the value-facing half: `incompatible_types` returns for declared keys, guessing and rename inference never propose planless pairs, and cell comparison reports a planless identity's type change without ever comparing its values. `reconcile_schema` stays infallible, and the map's invariant weakens honestly: a pair need not be comparable, so every stage that measures values asks for the plan first rather than expecting one.

The `incompatible` field rejoins the grammar's fixed set, and the fixed-set test gets its reaching fixture back — a rename hint across an incomparable pair, now spelled with a temporal type where it once used a boolean.

## What deliberately falls out for free

Opaque columns are not second-class above the comparison. A `Date32` column that is unique and shared identifies rows, so it can be guessed as the key or declared as one; a renamed timestamp column is recovered by exact inference on equal bytes; two same-named opaque columns whose contents were exchanged swap on the same evidence as any others, their crossings being identical-typed by construction. One-sided summaries inherit the widened admission automatically — `diff_added` on a file full of timestamps now works — which resolves the relaxed-validation deferral the one-sided step recorded. None of this is new code; all of it is new coverage, and the tests treat each as a claim to prove.

# Verification

- Unit tests in `src/compare.rs`: identical-typed opaque values equal exactly when their bytes do; a null opaque cell follows the null rules; differently-interned dictionaries of equal values compare equal across arrays; no plan for opaque-versus-different-opaque or opaque-versus-normalized; hashes agree with equality.
- Unit tests in `src/input.rs`: temporal, decimal, and binary columns admitted as `Opaque`; a genuinely un-encodable type still refused as `UnsupportedColumn`.
- Integration coverage in `tests/diff.rs`: a date column edited under a guessed integer key; a date column *as* the declared and the guessed key; an exact rename of a timestamp column; a swap of two same-named timestamp columns; a same-name `int64` → `Date32` retype reporting a type-only edit with no cells; `Timestamp(ms, UTC)` versus `Timestamp(ms)` naive likewise; an exchange that traded two columns' types recovered through its crossings; a declared key across an incomparable pair rejected as `incompatible_types` with the identity surviving and the comparison continuing; a rename hint across one honoured as a type change; `diff_added` on a temporal file; determinism on every path.
- CLI snapshots in `tests/cli.rs`: the temporal diff exits zero where it previously failed on stderr; the incomparable-retype output; the revived `hint_ignored(..., incompatible: ...)` line.
- The demo section held to real output by `tests/readme.rs`, its fixture pair written by `examples/generate_demo.rs` and read by its commands.
- Determinism: repeated runs byte-identical on every new path.

# Definition of done

This step is complete when:

- a Parquet file containing dates, timestamps, decimals, or binary columns diffs end to end, with identical-typed columns compared exactly and everything above the comparison — keys, inference, swaps, edits, one-sided summaries — working over them;
- an incomparable pair keeps its identity with a type-only edit and no value story, while everything value-facing refuses it: declared keys reject as `incompatible_types`, guessing and inference never propose one, and nothing is fatal;
- `UnsupportedColumn` survives only for types the canonical encoding cannot hold, probed rather than listed;
- cross-array byte equality is pinned by the dictionary test, and null handling is uniform across opaque and normalized types;
- `design.md` records the opaque regime and its revived vocabulary in present tense; `README.md` matches; the demo shows a temporal diff and an incomparable retype, held to real output; `plan-next.md` carries the promotion step ahead of benchmarking; and
- the full test suite, strict Clippy, formatting, and diff checks pass across the workspace.
