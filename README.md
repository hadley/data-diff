# data-diff

`data-diff` compares two Parquet files and emits a semantic diff as either
coordinate JSON or a compact, operation-oriented summary.

## Usage

```console
data-diff old.parquet new.parquet
```

When `--key` is omitted, `data-diff` guesses the row key: it considers every same-name single column that is compatible, free of nulls and `NaN`, and unique on both sides, and selects the one whose canonical values share the largest exact intersection across the files, breaking ties by old-column order. If no column qualifies — including when either input has no rows — the comparison fails and asks for an explicit key; a positional row-number fallback is planned as a separate step.

A user who knows the correct identity can declare it instead:

```console
data-diff old.parquet new.parquet --key customer_id,date,region
```

`--key` accepts a comma-separated simple or compound key whose columns have the same names in both files. An explicit key always overrides guessing, even when another column would be the strongest guess, and errors in a declared key stay fatal rather than being silently replaced by a guess. Output is the compact human format on stdout by default. Input, schema, and key errors are written to stderr and return a non-zero exit status.

To inspect the complete structured result, select JSON explicitly:

```console
data-diff old.parquet new.parquet --key id --format json
```

The default human format leads with the resolved key and then emits one operation per line:

```text
col_key(guessed: "id", overlap: 0.6666666666666666)
col_drop("product")
col_add("stock")
col_order("price", 3 -> 1)
col_edit("price", values)
row_drop(2)
row_add(3)
row_order(3 -> 1)
```

Column names are quoted, and row and column coordinates are one-based. The `col_key` line reports `declared: [...]` for an explicit key or `guessed: ...` with the normalized overlap `shared_values / min(old_rows, new_rows)` for a guessed key; the same ratio appears in the JSON `key.overlap` field. This initial human format intentionally identifies rows by position and does not include old or new cell values. It uses an exact minimum combination of `row_edit` and `col_edit` operations to summarize changed cells; for example, multiple changes in one row become `row_edit(2)`. An unchanged comparison emits `no_changes()` after the key line. Use `--format json` to inspect the summary together with the complete changed-cell evidence.

## Current behavior

`data-diff`:

* loads each file into memory;
* supports booleans, signed and in-range unsigned integers, `float32`,
  `float64`, UTF-8 strings, dictionary-encoded strings, and typed nulls;
* compares compatible numeric and parsed string representations exactly;
* guesses a single-column row key from exact cross-file evidence when `--key` is omitted, and reports the selected basis and overlap;
* reports schema additions, drops, and source-type edits;
* reports added, dropped, matched, and relatively reordered rows;
* reports relatively reordered columns and every changed matched cell;
* summarizes changed cells with an exact minimum set of row and column edits;
  and
* emits deterministic one-based coordinates referring to the original files.

It rejects duplicate column names, unsupported types, incompatible same-name columns, invalid declared keys, null or `NaN` declared-key values, non-unique old keys, new-side duplicates that would require fanout, and comparisons where no key was supplied and no eligible key could be guessed.

It does not yet fall back to row numbers when no key can be guessed, guess compound keys, accept paired old/new key names, infer renames, stream large files, or provide an interactive UI. See [plan.md](plan.md) for the current implementation plan and subsequent steps.

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
