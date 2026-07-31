# data-diff

`data-diff` compares two Parquet files and emits a semantic diff as a compact,
operation-oriented summary.

## Usage

```console
data-diff old.parquet new.parquet
> col_key([id], basis: guessed, overlap: 1.00)
> col_drop(product)
> col_add(stock)
> col_edit(price, changes: 4)
```

If you know what the primary key is (i.e. the set of variables that uniquely identifies each row) you should supply it:

```console
data-diff old.parquet new.parquet --key customer_id,date
> col_key([customer_id, date], basis: declared)
> row_drop(4)
> row_add(9)
> row_edit(2, changes: 3)
```

Otherwise `data-diff` guesses, taking the single column that identifies the most rows across both files. Where nothing can identify a row, it matches them by position and says so; `--key '#row'` asks for that key directly, which is a whole key and cannot be combined with a column. The key line always says which key was used and, for a guess, how much of the data it accounts for, so you can see whether to override it.

A key you declare can turn out not to identify rows --- it repeats a value, names a column one file lacks, or fans out too broadly. That is reported rather than fatal, and the comparison continues on whatever can identify rows instead:

```console
data-diff old.parquet new.parquet --key customer_id/id
> key_invalid([customer_id/id], reason: non_unique_old)
> ----
> col_key([#row], basis: fallback)
> col_rename(customer_id -> id, basis: declared)
> row_edit(2, changes: 1)
```

A paired component asserts two things --- that the two columns are one, and that the column identifies rows --- and the first survives the second failing.

Use a pair when the key column itself was renamed:

```console
data-diff old.parquet new.parquet --key customer_id/id
> col_key([customer_id -> id], basis: declared)
> col_rename(customer_id -> id, basis: declared)
> row_edit(2, changes: 1)
```

## Hints

When a change can't be worked out from the data --- e.g. a column renamed and rewritten at the same time, or both columns and rows modified simultaneously --- you can say what happened. A hint is written the way the output prints it, so the operation you want is the one you type:

```console
data-diff old.parquet new.parquet --key id --hint 'col_rename(discount -> markdown)'
> col_key([id], basis: declared)
> col_rename(discount -> markdown, basis: hinted)
> col_edit(markdown, changes: 3)
```

You can repeat `--hint` or use `--hints hints.txt` to take a file of hints, skipping blank lines and `#` comments.

There are four hints:

| Hint | Says |
|---|---|
| `col_rename(old -> new)` | these two columns are one column |
| `col_drop(old)`, `col_add(new)` | this column has no counterpart; supply both to choose replacement over a rename |
| `col_edit(column)`, `col_edit(old -> new)` | this column changed, rather than being half of a swap or a row's worth of edits |

A hint asserts identity and nothing more: what changed is still reported. Invalid hints are reported, then ignored.

## Output

Output goes to stdout, one operation per line:

| Operation | Meaning |
|---|---|
| `col_add(new)`, `col_drop(old)` | a column that only exists on one side |
| `col_rename(old -> new, basis: how)` | one column, named differently in each file, and how that was established |
| `col_edit(new, ...)` | a column whose type or values changed, and how many cells |
| `col_order(new, old_idx -> new_idx)` | the fewest columns that must move to explain the new order |
| `row_add(new_idx)`, `row_drop(old_idx)` | a row that only exists on one side |
| `row_edit(idx, changes: n)` | a row whose non-key values changed, and how many cells |
| `row_fanout(old_idx -> [new_idx, ...])` | one old row that several new rows share a key with |
| `row_order(old_idx -> new_idx)` | the fewest rows that must move to explain the new order |
| `key_invalid(subject, reason: why)` | a declared key this data could not support |
| `hint_ignored(hint, reason: why)` | an instruction that was declined |

Every edit says how much changed. `changes` counts the cells the event stands for: for a column, the rows it differs in; for a row, the columns it differs in. A row edit and a column edit that cross both count the cell they share, so the numbers describe their own row and their own column rather than adding up to the total. A type-only edit has no cells to count and carries no number.

A rename's `basis` is one of five: `declared` for a paired `--key` component, `hinted` for a `col_rename()` you supplied, `exact` where the values agree in every shared row, `approximate` where they agree closely enough and by more than chance, and `swapped` where two columns hold each other's values, which prints as two renames each saying it is half of one exchange. The first two are certainties and the rest are judgements, which is the difference the field exists to show.

A key's `basis` is one of three: `declared` for a `--key` you supplied, `guessed` where a column was chosen for you, and `fallback` where nothing could identify a row and they were matched by position.

`key_invalid()` names one component where resolving it failed on its own account, and the whole declared key, bracketed, where uniqueness or fanout failed --- those being properties of the tuple rather than of any column in it. The reason is one of `missing_column`, `incompatible_types`, `duplicate_column`, `invalid_value`, `non_unique_old`, and `excessive_fanout`, and it is the one that stopped validation rather than every one that might apply.

Anything that went wrong comes first --- a rejected key, a declined hint --- then a `----` line, then what the comparison found. With nothing to report there is no separator and the output opens on the key line.

A name is quoted only when it has to be: an ordinary one --- letters, digits and underscores, not starting with a digit --- is written bare, so quotes mark a name with something in it worth noticing. Coordinates are one-based, counting positions in the original files. A column is named as the new file names it, except where only the old file has it. When nothing changed, `no_changes()` follows the key line.

Problems with the input --- duplicate column names, an unsupported type, a `--key` string that cannot be read --- go to stderr, and the exit status is non-zero. A key that is well formed but cannot identify rows is reported on stdout as above, and the comparison still runs.

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

[demo/README.md](demo/README.md) shows the output of every command it documents, and `cargo test` re-runs them all against it, so that file cannot drift from what the tool does. After a deliberate change to the output format, refresh its transcripts rather than editing them by hand:

```console
UPDATE_README=1 cargo test --test readme
```

See [demo/README.md](demo/README.md) for ready-to-run sample datasets, [design.md](design.md) for what the operations mean and how they are resolved, and [plan.md](plan.md) with [plan-next.md](plan-next.md) for the step in progress and the ones after it.
