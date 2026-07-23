# data-diff

`data-diff` compares two Parquet files and emits a semantic, coordinate-only
JSON diff for human inspection.

## Usage

```console
data-diff old.parquet new.parquet --key customer_id,date,region
```

`--key` is required. It accepts a comma-separated simple or compound key whose
columns have the same names in both files. Output is pretty JSON on stdout.
Input, schema, and key errors are written to stderr and return a non-zero exit
status.

## MVP behavior

The MVP:

* loads each file into memory;
* supports booleans, signed and in-range unsigned integers, `float32`,
  `float64`, UTF-8 strings, dictionary-encoded strings, and typed nulls;
* compares compatible numeric and parsed string representations exactly;
* reports schema additions, drops, and source-type edits;
* reports added, dropped, matched, and relatively reordered rows;
* reports relatively reordered columns and every changed matched cell; and
* emits deterministic one-based coordinates referring to the original files.

It rejects duplicate column names, unsupported types, incompatible same-name
columns, missing or invalid keys, null or `NaN` key values, non-unique old keys,
and new-side duplicates that would require fanout.

The MVP intentionally does not guess keys, accept paired old/new key names,
infer renames, summarize cells into row/column edit events, stream large files,
or provide an interactive UI. See [plan.md](plan.md) for the completed MVP
sequence and the post-MVP roadmap.

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
