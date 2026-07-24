# data-diff

`data-diff` compares two Parquet files and emits a semantic diff as either
coordinate JSON or a compact, operation-oriented summary.

## Usage

```console
data-diff old.parquet new.parquet --key customer_id,date,region
```

`--key` is required. It accepts a comma-separated simple or compound key whose
columns have the same names in both files. Output is the compact human format
on stdout by default. Input, schema, and key errors are written to stderr and
return a non-zero exit status.

To inspect the complete structured result, select JSON explicitly:

```console
data-diff old.parquet new.parquet --key id --format json
```

The default human format emits one operation per line:

```text
col_drop("product")
col_add("stock")
col_order("price", 3 -> 1)
col_edit("price", values)
row_drop(2)
row_add(3)
row_order(3 -> 1)
```

Column names are quoted, and row and column coordinates are one-based. This
initial human format intentionally identifies rows by position and does not
include old or new cell values. It uses an exact minimum combination of
`row_edit` and `col_edit` operations to summarize changed cells; for example,
multiple changes in one row become `row_edit(2)`. An unchanged comparison emits
`no_changes()`. Use `--format json` to inspect the summary together with the
complete changed-cell evidence.

## Current behavior

`data-diff`:

* loads each file into memory;
* supports booleans, signed and in-range unsigned integers, `float32`,
  `float64`, UTF-8 strings, dictionary-encoded strings, and typed nulls;
* compares compatible numeric and parsed string representations exactly;
* reports schema additions, drops, and source-type edits;
* reports added, dropped, matched, and relatively reordered rows;
* reports relatively reordered columns and every changed matched cell;
* summarizes changed cells with an exact minimum set of row and column edits;
  and
* emits deterministic one-based coordinates referring to the original files.

It rejects duplicate column names, unsupported types, incompatible same-name
columns, missing or invalid keys, null or `NaN` key values, non-unique old keys,
and new-side duplicates that would require fanout.

It does not yet guess keys, accept paired old/new key names, infer renames,
stream large files, or provide an interactive UI. See [plan.md](plan.md) for
the current implementation plan and subsequent steps.

## Development

Build and run directly from the checkout:

```console
cargo build
cargo run -- old.parquet new.parquet --key customer_id,date,region
```

The debug binary is written to `target/debug/data-diff`. To install the current
checkout into Cargo's binary directory instead:

```console
cargo install --path .
data-diff old.parquet new.parquet --key customer_id,date,region
```

Re-run `cargo install --path . --force` after changing the source if you want to
replace the installed binary. For local development checks:

```console
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Algorithm tests construct compact Arrow tables in memory. Parquet and CLI tests
are limited to the file and process boundaries.

See [demo/README.md](demo/README.md) for ready-to-run sample datasets.
