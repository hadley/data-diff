# data-diff

`data-diff` compares two Parquet files and emits a semantic diff as a compact,
operation-oriented summary.

## Usage

```console
data-diff old.parquet new.parquet
```

When `--key` is omitted, `data-diff` guesses the row key: it considers every same-name single column that is compatible, free of nulls and `NaN`, unique in the old file, and no more duplicated in the new file than the fanout limit allows, and selects the one sharing the largest number of key values across the files. A candidate that duplicates a shared key is not penalized for it beyond that limit, because a true key that duplicated a row identifies more rows than a column that is unique by coincidence; freedom from fanout only breaks a tie, and old-column order breaks what remains. If no column qualifies — including when either input has no rows — the comparison fails and asks for an explicit key; a positional row-number fallback is planned as a separate step.

A user who knows the correct identity can declare it instead:

```console
data-diff old.parquet new.parquet --key customer_id,date,region
```

`--key` accepts a comma-separated simple or compound key. A component is one column name used in both files, or an `old/new` pair naming a column that was renamed between them:

```console
data-diff old.parquet new.parquet --key customer_id/id,date
```

A paired component identifies the two columns as one, so they are neither a drop nor an addition, the rows line up through them, and the change is reported as `col_rename("customer_id" -> "id")`. No column may be claimed by more than one component. An explicit key always overrides guessing, even when another column would be the strongest guess, and errors in a declared key stay fatal rather than being silently replaced by a guess. The human format is the only output and is written to stdout. Input, schema, and key errors are written to stderr and return a non-zero exit status.

Output leads with the resolved key and then emits one operation per line:

```text
col_key(guessed: ["id"], overlap: 0.67)
col_drop("product")
col_add("stock")
col_order("price", 3 -> 1)
col_edit("price", values)
row_drop(2)
row_add(3)
row_order(3 -> 1)
```

Column names are quoted, and row and column coordinates are one-based. The summary is deliberately minimal: several changed cells in one row are reported as a single `row_edit()`. The complete cell-level diff is retained in the library result rather than printed.

## Current behavior

`data-diff`:

* loads each file into memory;
* supports booleans, signed and in-range unsigned integers, `float32`,
  `float64`, UTF-8 strings, dictionary-encoded strings, and typed nulls;
* compares compatible numeric and parsed string representations exactly;
* guesses a single-column row key from exact cross-file evidence when `--key` is omitted, allowing it to fan out under the same limit as a declared key, and reports the selected basis and overlap;
* identifies a declared old/new key column pair across the two files, reporting it as a rename;
* infers a rename between an otherwise dropped and added column when the two hold identical values in every matched row, comparing across compatible types and ignoring added, dropped, and fanned-out rows;
* reports schema additions, drops, and source-type edits;
* reports added, dropped, matched, and relatively reordered rows;
* keeps a declared key that identifies one old row and several new rows, when at most 10% of the key values shared by the two files are duplicated in the new one, and reports each affected key as a `row_fanout()` event holding the old row, its new rows, and the values that differ between them;
* reports relatively reordered columns and every changed matched cell;
* summarizes changed cells with an exact minimum set of row and column edits;
  and
* emits deterministic one-based coordinates referring to the original files.

It rejects duplicate column names, unsupported types, incompatible same-name columns, invalid declared keys, malformed key components and columns claimed by more than one of them, null or `NaN` declared-key values, non-unique old keys, declared keys whose new-side duplication exceeds the fanout limit, and comparisons where no key was supplied and no eligible key could be guessed. A guessed key is held to the same fanout limit as a declared one, but a candidate that exceeds it is simply passed over rather than reported.

It does not yet expose the complete cell-level result to users, fall back to row numbers when no key can be guessed, guess compound keys, accept column hints, infer a rename from partial rather than exact agreement, detect swapped columns, stream large files, or provide an interactive UI. See [plan.md](plan.md) for the current implementation plan and subsequent steps.

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
