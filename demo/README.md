# Demo datasets

Generate or refresh all fixtures from the repository root:

```console
cargo run --example generate_demo
```

Install the development build once:

```console
cargo install --path .
```

The commands below use that installed `data-diff` binary. Every one is shown with the output it produces, and `tests/readme.rs` re-runs them all against this file, so nothing here can drift from what the tool does.

## Keys

`data-diff` works best with an explicit key:

```console
$ data-diff demo/basic-old.parquet demo/basic-new.parquet --key id
col_key([id], basis: declared)
row_edit(2, changes: 2)
```

But if you don't supply it, `data-diff` will guess, looking for columns that have the same name on both sides and taking the one that identifies the most rows:

```console
$ data-diff demo/basic-old.parquet demo/basic-new.parquet
col_key([id], basis: guessed, overlap: 1.00)
row_edit(2, changes: 2)
```

You can also match keys that have been renamed, by naming both sides as a pair:

```console
$ data-diff demo/key-rename-old.parquet demo/key-rename-new.parquet --key customer_id/id
col_key([customer_id -> id], basis: declared)
col_rename(customer_id -> id, basis: declared)
row_edit(2, changes: 1)
```

Without the pair the rows no longer line up. Key guessing only pairs candidates with the same name in both files, so it cannot see this one and settles on `amount` instead:

```console
$ data-diff demo/key-rename-old.parquet demo/key-rename-new.parquet
col_key([amount], basis: guessed, overlap: 0.67)
col_rename(customer_id -> id, basis: exact)
row_drop(2)
row_add(2)
```

The rename is still found, but only after the key has been resolved and the rows matched by it, which is too late to be any use.

## Fanout

We can still use a key, even if it's duplicated (up to 10%) in the new table, as this might indicate a join gone wrong:

```console
$ data-diff demo/fanout-old.parquet demo/fanout-new.parquet --key id
col_key([id], basis: declared)
row_fanout(4 -> [4, 5], changes: 1)
```

## Value edits

When cells change, `data-diff` reports the minimal set of rows and columns that accounts for them. For example, here four cells change in an L shape: `a` and `b` both change in row 1, and `c` changes in rows 2 and 3.

```console
$ data-diff demo/scatter-old.parquet demo/scatter-new.parquet --key id
col_key([id], basis: declared)
col_edit(c, changes: 2)
row_edit(1, changes: 2)
```

We'll also report a column whose type changed even when all of its values compare as equal:

```console
$ data-diff demo/types-old.parquet demo/types-new.parquet --key id
col_key([id], basis: declared)
col_edit(id, type: Int32 -> Int64)
col_edit(amount, type: Int32 -> Float64)
```

## Row and column order

If the position of rows and columns changes but the values stay the same, we just report that the order changed:

```console
$ data-diff demo/order-old.parquet demo/order-new.parquet --key id
col_key([id], basis: declared)
col_order(price, 3 -> 1)
row_order(3 -> 1)
```

## Renames and swaps

`data-diff` can detect renamed columns if all values are the same:

```console
$ data-diff demo/rename-old.parquet demo/rename-new.parquet --key id
col_key([id], basis: declared)
col_rename(amount -> total, basis: exact)
row_edit(2, changes: 1)
```

Or if a small fraction of values are different:

```console
$ data-diff demo/approx-rename-old.parquet demo/approx-rename-new.parquet --key id
col_key([id], basis: declared)
col_rename(amount -> total, basis: approximate)
row_edit(7, changes: 1)
```

Or if the values in two columns were swapped:

```console
$ data-diff demo/swap-old.parquet demo/swap-new.parquet --key id
col_key([id], basis: declared)
col_rename(price -> cost, basis: swapped)
col_rename(cost -> price, basis: swapped)
col_order(price, 3 -> 2)
```

Or if you provide an explicit hint:

```console
$ data-diff demo/hint-rename-old.parquet demo/hint-rename-new.parquet --key id \
    --hint 'col_rename(discount -> markdown)'
col_key([id], basis: declared)
col_rename(discount -> markdown, basis: hinted)
col_edit(markdown, changes: 3)
```
