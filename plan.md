---
title: One-sided diffs
---

# Todo

- [x] **Add the model.** `OneSidedDiff { side: Side, columns: Vec<ColumnSchema>, rows: usize }` in `src/model.rs`: which side exists, the schema as validation already describes it, and the row count. Nothing else — a one-sided diff has no key, no identities, no cells, and holding empty versions of those would claim a reconciliation that never ran.
- [x] **Add the library entry points.** `diff_added(new: &RecordBatch)` and `diff_removed(old: &RecordBatch)` in `src/lib.rs`, thin wrappers over one internal constructor, each returning `Result<OneSidedDiff, DiffError>`. Validation is the same `validate_table` the two-sided path runs, on the one side that exists; that function is private to `src/input.rs` today and becomes `pub(crate)`, the constructor staying in `src/lib.rs` beside the other entry points rather than moving into the input module.
- [x] **Render it.** `write_human_one_sided` in `src/human.rs`: a `table_add(rows: n)` or `table_drop(rows: n)` headline — the field omitted when the count is zero, as every count the format writes is positive — followed by one `col_add()` or `col_drop()` line per column in file order. No key line, no separator: nothing was matched and nothing can go wrong.
- [x] **Extend the grammar's fixed field set.** `rows` joins `basis`, `changes`, `missing`, `overlap`, `reason`, `type` in `design.md`'s line grammar, and the fixed-set test in `src/human.rs` gains a one-sided rendering so the new field is reached.
- [x] **Wire up the CLI.** The reserved path `#missing` names an absent side: `data-diff '#missing' new.parquet` is an added file and `data-diff old.parquet '#missing'` a removed one. The token is `pub const MISSING_FILE` in `src/input.rs` — it names an input path, so the input module owns it, as `src/key.rs` owns `POSITIONAL_COMPONENT` — re-exported from `src/lib.rs` so the two reserved names sit beside each other in the public API; no shared constants module. It is recognized only as the exact bare argument — a real file of that name is still reachable as `./#missing` — and `src/main.rs` refuses both sides missing, and a `--key` or hint beside a missing side, as faults in the instruction: fatal, on stderr, before anything is read.
- [x] **Update `design.md`.** The vocabulary table gains `table_add()` and `table_drop()`; a new "One-sided diffs" section records the entry points, the model, the rendering, and the decisions argued below — same validation as two-sided, no types on the column lines, count rather than positions.
- [x] **Update `README.md`.** The usage section shows `'#missing'` in each position, and the output table gains the two lines.
- [x] **Refresh the demo.** A "One-sided diffs" section in `demo/README.md` running `'#missing'` in each position over the existing `basic-*.parquet` fixtures, so no new fixture is written and none is orphaned. Transcripts held to real output by `tests/readme.rs`, prose written by hand.
- [x] **Cover it.** Unit tests for the renderer including the zero-row and zero-column edges; integration tests in `tests/diff.rs` for both entry points and validation failures; CLI snapshots in `tests/cli.rs` for both sentinel positions, the updated `--help`, and the refused combinations; determinism checks.
- [x] **Rename `col_key()` to `table_key()`.** Added in review: the key line is context about the whole table, not a change to a column — the positional key's `col_key([#row])` named no column at all — and the `table_` family this step created is its natural home. Purely a rendering rename; `KeyDiff` and every model name stay. `key_invalid()` and `key_retracted()` remain the key's problem lines, so the key's problems and its answer now wear different prefixes on either side of the separator.
- [x] **Complete the acceptance pass.** `cargo build --workspace --all-targets`, `cargo test --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`, and byte-identical repeated runs.

# Goal

A dataset's history contains more than modifications: files appear and files go away, and a review of "how did the AI change my data" has to say something about those too. Today the tool has no way to be asked. Both CLI positionals are required, `diff_tables` takes two tables, and even if a caller synthesized an empty old side, the output would enumerate every row of the new file as its own `row_add()` line and open with a `table_key([#row], basis: fallback)` that describes a matching that never meaningfully ran.

This step gives the absent side a first-class spelling — the reserved path `#missing`, following `#row`'s precedent for names the tool reserves — and a brief answer:

```console
$ data-diff '#missing' demo/basic-new.parquet
table_add(rows: 3)
col_add(id)
col_add(name)
col_add(score)
```

```console
$ data-diff demo/basic-old.parquet '#missing'
table_drop(rows: 3)
col_drop(id)
col_drop(name)
col_drop(score)
```

The headline says what happened and how much of it, the column lines say the shape, and that is the whole of it: brief because everything else — every row added, every cell "new" — is implied by the file being one-sided and would be enumeration without information.

# Scope

## What changes

- `src/model.rs` gains `OneSidedDiff`.
- `src/lib.rs` gains `diff_added` and `diff_removed`.
- `src/human.rs` gains `write_human_one_sided` and renders the two table lines.
- `src/input.rs` exports `MISSING_FILE`; `src/main.rs` recognizes it and routes to the one-sided path.
- `design.md`, `README.md`, `demo/README.md`, and the test suites.

## What stays and why

- **`diff_tables` and everything under it.** A one-sided diff runs no reconciliation, so nothing in the pipeline changes. In particular, comparing against a genuinely empty table — zero rows, or zero columns — behaves exactly as the empty-inputs section already specifies; emptiness is a property of a file that exists, not a way of spelling absence.
- **Validation.** The one present side passes through the same `validate_table` as either side of a two-sided comparison: duplicate names would break the output's own naming, and keeping one rule for what the tool reads is worth more than tolerating, in the summary, a file the comparison would refuse. Relaxing this is noted below.
- **The two-sided rendering of many added rows.** A two-sided diff that appends a thousand rows still prints a thousand `row_add()` lines. That is a real problem, but it is a rendering-budget problem the benchmarking step owns; solving it here for the one file-level case would leave the general case inconsistent with it.

## Explicitly deferred

- **Types on the one-sided column lines.** `col_add(id, type: Int64)` would make the summary a fuller schema description, but two-sided `col_add()` lines carry no type either, and one vocabulary line should not have two shapes. The model's `ColumnSchema` holds source and normalized types for the UI to show; if the format ever grows types on unmatched columns, it should do so for both arities at once.
- **Relaxed validation for summaries.** A removed-file summary failing because the departed file holds a timestamp column is defensible but unhelpful; tolerating unsupported types in a summary that never compares values is a plausible loosening. It waits for the broader-types step, which owns what an unsupported column even is.
- **Directory-level pairing.** Deciding *which* files were added, removed, or renamed across two directories is the caller's problem (git already solves it); the tool takes one file and a direction.
- **Accepting the null device as an alias.** Git invokes difftools with `/dev/null` (or `NUL`) standing for an absent side; treating those paths as `#missing` would let `data-diff` serve as a difftool unconfigured. It is one platform-aware equality check when wanted, and it waits until git integration is actually pursued.
- **A machine-readable result.** `OneSidedDiff` is library-public like `Diff`; anything beyond that is the UI item's business.

# Design

## An absence is spelled, not synthesized

The absent side keeps its argument position and is named there: `#missing`, a reserved path following `#row`'s precedent for names the tool reserves rather than looks up. The command's shape never changes — old then new, always — and the direction falls out of which position is missing instead of being restated in a flag, so `data-diff a.parquet b.parquet` and `data-diff a.parquet '#missing'` differ only in the one fact that differs. The token is reserved only as the exact bare argument: since it never contains a separator, a real file named `#missing` is still reachable as `./#missing`, and the quoting the shell requires is the same the documented `--key '#row'` already carries.

Two other spellings of absence are rejected. An empty table is a file that exists with nothing in it — the empty-inputs section gives it real semantics (schema comparison still runs, a declared key must still resolve) — and overloading it to mean "no file" would make those two situations indistinguishable in the model and in the output. The null device (`/dev/null`, the convention git passes to difftools) is platform-dependent and reads as a real path while never being one this tool could parse; accepting it as an alias for `#missing` would ease git integration and is noted below as deferred rather than taken on here.

The library API is not the sentinel's business. `diff_tables(Option<&RecordBatch>, Option<&RecordBatch>)` would make `(None, None)` representable and turn every caller's two arguments into puzzles; `diff_added` and `diff_removed` name the two real situations and cannot express the impossible one. Both delegate to one internal constructor taking the present `Side`, so the pair costs no duplication; the CLI maps the sentinel's position onto the right one.

## A dedicated model, not a hollowed-out `Diff`

A `Diff` asserts things a one-sided comparison never established: a resolved key with a basis, a column bijection, a set of matched rows, a cell-level change set. Reusing it with those fields empty would make every consumer ask "is this a real emptiness or an absence?" — the same conflation the sentinel was rejected for, moved into the result. `OneSidedDiff` holds exactly what is known: which side exists, the validated schema, and the row count. The design invariants hold vacuously and honestly: nothing is inferred (every event is read directly off the file), and no cell-level evidence is withheld because none exists — there is nothing to compare a cell against.

The row count is a count, not positions. A wholly added file's row positions are `1..=n` by construction; storing them would be manufacturing coordinates to fill a field, and rendering them is the enumeration this step exists to avoid.

## The rendering

`table_add(rows: 3)` then `col_add()` per column, in file order. The table line comes first because it plays the role the key line plays in a two-sided diff: context that orients everything under it, and the reason there *is* no key line. `rows` is a new field in the grammar's fixed set — `changes` was considered and rejected, because a `changes:` that counts rows on one line and cells on every other would make the field's meaning depend on the line it sits on, which the fixed set exists to prevent. The field is omitted at zero, following the rule that every count the format writes is positive; `table_add()` followed by column lines reads correctly as an empty file with a schema, and `table_add()` alone as a file with neither rows nor columns.

`table_add()` and `table_drop()` join the vocabulary beside `table_regenerate()`: the subject is the table, so the line has no name argument. They are not hint kinds and are rejected as hints by the existing by-kind rule, like `table_key()` and the row operations.

## The CLI contract

Two combinations are refused as faults in the instruction, fatal on stderr before anything is read, following the precedent that a fault in the instruction is fatal while a fault in the data is reported. `data-diff '#missing' '#missing'` compares nothing to nothing and asks no answerable question. A `--key` or hint beside a missing side is an instruction about a reconciliation that cannot run — the same reasoning that had the flags conflict in an earlier draft, enforced in `src/main.rs` now that clap cannot see it in a positional's value. The refusals and the updated `--help` text, whose `<OLD>`/`<NEW>` descriptions now document the sentinel the way `--key`'s documents `#row`, are snapshot-tested.

# Verification

- Unit tests in `src/human.rs`: an added and a removed rendering; the zero-row file omitting `rows:`; the zero-column file rendering the bare table line; quoting on a column name that needs it; and the fixed-field-set test extended with a one-sided rendering so `rows` is reached.
- Integration tests in `tests/diff.rs`: `diff_added` and `diff_removed` return the validated schema and count; a duplicate-name and an unsupported-type file fail with the same errors the two-sided path gives; repeated runs are equal and render byte-identically.
- CLI snapshots in `tests/cli.rs`: an added and a removed transcript with exit zero; the updated `--help`; `#missing` on both sides, and `#missing` beside `--key` and beside `--hint`, each refused with a non-zero exit; and a file literally named `#missing` reached as `./#missing`.
- `tests/readme.rs` holds the new demo section to real output, with both commands reading existing fixtures and no fixture orphaned.
- Determinism: repeated one-sided runs byte-identical.

# Definition of done

This step is complete when:

- `data-diff '#missing' file.parquet` and `data-diff file.parquet '#missing'` print a `table_add(rows: n)` / `table_drop(rows: n)` headline and one column line per column, and exit zero;
- `diff_added` and `diff_removed` are public library entry points returning `OneSidedDiff`, validated exactly as the two-sided path validates a side;
- the impossible states are unrepresentable in the library: no sentinel table, no optional pair of tables, no `Diff` with a fabricated key;
- contradictory instructions — both sides missing, or a missing side beside `--key` or hints — are refused at the command line;
- `design.md` carries the vocabulary additions, the `rows` field, and the one-sided section; `README.md` matches; the demo shows both directions over existing fixtures and `tests/readme.rs` holds it to real output; and
- the full test suite, strict Clippy, formatting, and diff checks pass across the workspace.
