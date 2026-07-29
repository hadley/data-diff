# data-diff

`data-diff` compares two Parquet files and emits a semantic diff as a compact,
operation-oriented summary.

## Usage

```console
data-diff old.parquet new.parquet
> col_key([id], basis: guessed, overlap: 1.00)
> col_drop(product)
> col_add(stock)
> col_edit(price, values)
```

If you know what the primary key is (i.e. the set of variables that uniquely identifies each row) you should supply it:

```console
data-diff old.parquet new.parquet --key customer_id,date
> col_key([customer_id, date], basis: declared)
> row_drop(4)
> row_add(9)
> row_edit(2)
```

Otherwise `data-diff` guesses, taking the single column that identifies the most rows across both files. The first line of output always says which key was used and, for a guess, how much of the data it accounts for, so you can see whether to override it.

Use a pair when the key column itself was renamed:

```console
data-diff old.parquet new.parquet --key customer_id/id
> col_key([customer_id -> id], basis: declared)
> col_rename(customer_id -> id)
> row_edit(2)
```

## Hints

When a change can't be worked out from the data --- e.g. a column renamed and rewritten at the same time --- you can say what happened. A hint is written the way the output prints it, so the operation you want is the one you type:

```console
data-diff old.parquet new.parquet --key id --hint 'col_rename(discount -> markdown)'
> col_key([id], basis: declared)
> col_rename(discount -> markdown)
> col_edit(markdown, values)
```

Quotes are optional, and needed only for a name holding a comma, a bracket, an arrow, or spaces at either end. The output quotes a little more readily than that, so any line you read back is one you can type straight in. `--hint` repeats, and `--hints hints.txt` reads one per line, skipping blank lines and `#` comments.

A hint asserts identity and nothing more: what changed inside the column it identified is still reported. One you get wrong is reported rather than obeyed, on a `hint_ignored()` line, and the comparison still runs.

## Output

Output goes to stdout, one operation per line:

| Operation | Meaning |
|---|---|
| `col_add(new)`, `col_drop(old)` | a column that only exists on one side |
| `col_rename(old -> new)` | one column, named differently in each file |
| `col_edit(new, ...)` | a column whose type or values changed |
| `col_order(new, old_idx -> new_idx)` | the fewest columns that must move to explain the new order |
| `row_add(new_idx)`, `row_drop(old_idx)` | a row that only exists on one side |
| `row_edit(idx)`, `row_edit(old_idx -> new_idx)` | a row whose non-key values changed |
| `row_fanout(old_idx -> [new_idx, ...])` | one old row that several new rows share a key with |
| `row_order(old_idx -> new_idx)` | the fewest rows that must move to explain the new order |

A name is quoted only when it has to be: an ordinary one --- letters, digits and underscores, not starting with a digit --- is written bare, so quotes mark a name with something in it worth noticing. Coordinates are one-based, counting positions in the original files. A column is named as the new file names it, except where only the old file has it. When nothing changed, `no_changes()` follows the key line.

Problems with the input --- duplicate column names, an unsupported type, a key that cannot identify rows --- go to stderr, and the exit status is non-zero.

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

See [demo/README.md](demo/README.md) for ready-to-run sample datasets, [design.md](design.md) for what the operations mean and how they are resolved, and [plan.md](plan.md) with [plan-next.md](plan-next.md) for the step in progress and the ones after it.
