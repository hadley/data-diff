# data-diff

`data-diff` compares two Parquet files and emits a semantic diff as a compact, operation-oriented summary.

## Usage

```console
data-diff old.parquet new.parquet
> table_key([id], basis: guessed, overlap: 1.00)
> col_drop(product)
> col_add(stock)
> col_edit(price, changes: 4)
```

If you know the primary key (that is, the set of columns that uniquely identifies each row), supply it:

```console
data-diff old.parquet new.parquet --key customer_id,date
> table_key([customer_id, date], basis: declared)
> row_drop(4)
> row_add(9)
> row_edit(2, changes: 3)
```

Otherwise `data-diff` guesses, and takes the single column that identifies the most rows across both files. Where nothing can identify a row, it matches rows by position and says so. `--key '#row'` asks for that key directly. The key line always says which key was used and, for a guess, how much of the data it accounts for. 

A key `data-diff` chose for itself is also judged by the diff it produces. If the first pass's rename inference identifies a better candidate (for example, a renamed column), the key is reconsidered. A guess whose diff changes more than half of the two files' cells is not believed. It is retracted, reported as `key_retracted([column], reason: excessive_change)`, and the comparison reruns on the next candidate or on row position.

A key you declare can turn out not to identify rows: it repeats a value, names a column one file lacks, or fans out too broadly. That is reported and the comparison continues on whatever can identify rows instead:

```console
data-diff old.parquet new.parquet --key customer_id/id
> key_invalid([customer_id -> id], reason: non_unique_old)
> ----
> table_key([#row], basis: fallback)
> col_rename(customer_id -> id, basis: declared)
> row_edit(2, changes: 1)
```

A paired component asserts two things: that the two columns are one, and that the column identifies rows. The first assertion survives even if the second fails.

When a file is new or was deleted, there is nothing to compare it against. Name the missing side `'#missing'`, and the file that exists is summarized: a table-level headline with the row count, then its columns.

```console
data-diff '#missing' new.parquet
> table_add(rows: 3)
> col_add(id)
> col_add(price)
```

Use a pair when the key column itself was renamed:

```console
data-diff old.parquet new.parquet --key customer_id/id
> table_key([customer_id -> id], basis: declared)
> col_rename(customer_id -> id, basis: declared)
> row_edit(2, changes: 1)
```

## Hints

Some changes cannot be worked out from the data: for example, a column renamed and rewritten at the same time, or columns and rows modified together. Then you can provide a **hint** to say what happened. A hint is written the way the output prints it, so the operation you want is the one you type:

```console
data-diff old.parquet new.parquet --key id --hint 'col_rename(discount -> markdown)'
> table_key([id], basis: declared)
> col_rename(discount -> markdown, basis: hinted)
> col_edit(markdown, changes: 3)
```

You can repeat `--hint`, or use `--hints hints.txt` to read a file of hints. Blank lines and `#` comments are skipped.

There are four hints:

| Hint | Says |
|---|---|
| `col_rename(old -> new)` | these two columns are one column |
| `col_drop(old)`, `col_add(new)` | this column has no counterpart. Supply both to choose replacement over a rename |
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
| `table_regenerate()` | the new file is not usefully described as an edit of the old |
| `table_add(rows: n)`, `table_drop(rows: n)` | a one-sided diff: the whole file is new or gone |
| `key_invalid(subject, reason: why)` | a declared key this data cannot support |
| `key_retracted([column], reason: why)` | a guessed key withdrawn after the diff it produced |
| `incomplete_renames()`, `incomplete_swaps()`, `incomplete_summary()` | a stage its computation budget cut short |
| `hint_ignored(hint, reason: why)` | an instruction that was declined |

Every edit says how much changed. `changes` counts the cells the event stands for: for a column, the rows it differs in, and for a row, the columns it differs in. A row edit and a column edit that cross both count the cell they share. The numbers therefore describe their own row and their own column, and they do not add up to the total. A type-only edit has no cells to count, so it carries no number.


A rename's `basis` is one of five options: `declared` for a paired `--key` component, `hinted` for a `col_rename()` you supplied, `exact` where the values agree in every shared row, `approximate` where they agree closely enough and by more than chance, and `swapped` where two columns hold each other's values. A swap prints as two renames, and each says it is half of one exchange. The first two bases are certainties and the rest are judgements, which is the difference the field exists to show.

A key's `basis` is one of three values: `declared` for a `--key` you supplied, `guessed` where a column was chosen for you, and `fallback` where nothing identified a row and rows were matched by position.

`key_invalid()` names one component where that component failed to resolve on its own account. It names the whole declared key, bracketed, where uniqueness or fanout failed, because those are properties of the tuple rather than of any column in it. The reason is one of `missing_column`, `incompatible_types`, `duplicate_column`, `invalid_value`, `non_unique_old`, and `excessive_fanout`. It is the one reason that stopped validation, not every reason that can apply.

When more than half of the cells change under a guessed or fallback key, the row story is withheld, and `table_regenerate()` stands in for it. Row events and value counts describe a matching the tool no longer believes. Only what follows from schemas and identities is kept: the key line, renames, column adds, drops, order, and type changes. A declared key is exempt. You vouched for the matching, so the edits are reported in full.

The searches behind rename inference, swap inference, and the edit summary run under computation budgets, so very large inputs stay fast. A budget that runs out never makes anything up: it stops a search early and says so with an `incomplete_*()` line. `incomplete_renames()` means some drop/add pairs were never compared, so a `col_drop()` beside a `col_add()` can really be a rename. `incomplete_swaps()` means two heavily edited same-name columns can really be an exchange. `incomplete_summary()` means the row and column edits cover every changed cell, but might use more events than the minimum. Everything reported is still real, still exact, and still deterministic.

Anything that went wrong comes first: a rejected key, a retracted guess, a search cut short, a declined hint. Then comes a `----` line, then what the comparison found. With nothing to report, there is no separator, and the output opens on the key line.


Problems with the input — duplicate column names, an unsupported type, a `--key` string that cannot be read — go to stderr, and the exit status is non-zero. A key that is well formed but cannot identify rows is reported on stdout, as above, and the comparison still runs.

## Development

Build and run directly from the checkout:

```console
cargo build
cargo run -- old.parquet new.parquet --key customer_id,date,region
```

The debug binary is written to `target/debug/data-diff`. To install the current checkout into Cargo's binary directory instead:

```console
cargo install --path .
data-diff old.parquet new.parquet --key customer_id,date,region
```

If you change the source and want to replace the installed binary, re-run `cargo install --path . --force`. For local development checks:

```console
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

[demo/README.md](demo/README.md) shows the output of every command it documents, and `cargo test` re-runs them all against it, so that file cannot drift from what the tool does. After a deliberate change to the output format, refresh its transcripts rather than edit them by hand:

```console
UPDATE_README=1 cargo test --test readme
```

See [demo/README.md](demo/README.md) for ready-to-run sample datasets, [design.md](design.md) for what the operations mean and how they are resolved, and [plan.md](plan.md) with [plan-next.md](plan-next.md) for the step in progress and the ones after it.
