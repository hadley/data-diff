# data-diff

`data-diff` compares two Parquet files and shows which rows, columns, and cell values changed.

## Installation

Pre-built binaries are coming soon. Clone the repository, build the frontend, and install the CLI:

```console
git clone https://github.com/hadley/data-diff
cd data-diff/
(cd ui && npm install && npm run build)
cargo install --path .
```

Then run the demo from the repository root:

```console
data-diff demo/basic-old.parquet demo/basic-new.parquet --ui
```

## Basic usage

```console
data-diff demo/basic-old.parquet demo/basic-new.parquet
> table_key([id], basis: guessed, overlap: 1.00)
> row_edit(rows: 1, changes: 2, columns: [name, score])
```

Here the row with `id = 2` has two edits: `name` changed from `Ben` to `Bea`, and `score` changed from `20` to `25`.

See more examples in [demo/README.md](demo/README.md).

## Additional Options

### --key

`--key` tells `data-diff` which columns identify the same row in both files. For example, use `--key id` to match rows by their `id` values:

```console
data-diff demo/basic-old.parquet demo/basic-new.parquet --key id
> table_key([id], basis: declared)
> row_edit(rows: 1, changes: 2, columns: [name, score])
```

After matching rows by `id`, `data-diff` compares all remaining columns, including `name` and `score` in this example.

**Renamed key column:** Give the old and new column names separated by `/`. For example, `--key customer_id/id` matches `customer_id` in the old file with `id` in the new file.

**Matching by row position:** Use `--key :row` to compare the first row in each file, then the second, and so on.

**Invalid key:** If the selected columns do not form a usable key, `data-diff` reports the problem and tries to guess another key. If it cannot find one, it matches rows by position.

### --ui

Add the `--ui` flag to get an interactive UI. You can click on each summary in the sidebar to see the details. The server prints its address (default http://127.0.0.1:9471, `--port` to change) and opens it in your browser.

## Semantics

`data-diff` attempts to turn every change into one of the following row and column operations:

| Operation | Meaning |
|---|---|
| `col_add(new)`, `col_drop(old)` | a column that only exists on one side |
| `col_rename(old -> new, basis: how)` | one column, named differently in each file, and how that was established |
| `col_edit(new, ...)` | a column whose type or values changed, and how many cells |
| `col_order(new, old_idx -> new_idx)` | the fewest columns that must move to explain the new order |
| `row_add(rows: n)`, `row_drop(rows: n)` | rows that only exist on one side |
| `row_edit(rows: n, changes: m, columns: [...])` | rows sharing one changed-column set: how many rows, how many cells, and which columns when the list is short |
| `row_fanout(old_idx -> [new_idx, ...])` | one old row that several new rows share a key with |
| `row_order(old_idx -> new_idx)` | the fewest rows that must move to explain the new order |

This is most challenging for cell edits: how do you figure out whether changes scattered across a table represent row or column changes? `data-diff` frames this as a problem of parsimony: it picks the smallest description that summarises the changes. And then because that might be wrong, it exposes other options in the UI.

## Hints

Some changes cannot be worked out from the data: for example, a column renamed and rewritten at the same time, or columns and rows modified together. In this case, you can provide a **hint** to say what happened, e.g.:

```console
data-diff demo/hint-rename-old.parquet demo/hint-rename-new.parquet --key id --hint 'col_rename(discount -> markdown)'
> table_key([id], basis: declared)
> col_rename(discount -> markdown, basis: hinted)
> col_edit(markdown, changes: 3)
```

You can repeat `--hint`, or use `--hints hints.txt` to read a file of hints. Blank lines and `#` comments are skipped.

There are currently four hints:

| Hint | Says |
|---|---|
| `col_rename(old -> new)` | these two columns are one column |
| `col_drop(old)`, `col_add(new)` | this column has no counterpart. Use to choose replacement over a rename |
| `col_edit(column)`, `col_edit(old -> new)` | this column changed, rather than being half of a swap or a row's worth of edits |
