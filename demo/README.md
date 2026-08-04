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
table_key([id], basis: declared)
row_edit(2, changes: 2)
```

But if you don't supply it, `data-diff` will guess, looking for columns that have the same name on both sides and taking the one that identifies the most rows:

```console
$ data-diff demo/basic-old.parquet demo/basic-new.parquet
table_key([id], basis: guessed, overlap: 1.00)
row_edit(2, changes: 2)
```

You can also match keys that have been renamed, by naming both sides as a pair:

```console
$ data-diff demo/key-rename-old.parquet demo/key-rename-new.parquet --key customer_id/id
table_key([customer_id -> id], basis: declared)
col_rename(customer_id -> id, basis: declared)
row_edit(2, changes: 1)
```

The pair is not required, however. If we don't supply it, `data-dict` goes to work. First it looks at all pairs of identically named columns, looking for potential keys that overlap between the two columns. In this case it settles on `amount`, but then rename inference identifies `customer_id` and `id` as one column, so key is reconsidered once with that identity in hand. The renamed pair wins on the evidence, so we reconstruct the correct key:

```console
$ data-diff demo/key-rename-old.parquet demo/key-rename-new.parquet
table_key([customer_id -> id], basis: guessed, overlap: 1.00)
col_rename(customer_id -> id, basis: exact)
row_edit(2, changes: 1)
```

The result is the same diff the explicit pair produces, with `basis: guessed` recording that the tool arrived at it rather than being told.

### When nothing can identify a row

Both columns here repeat a value, so neither can be a key. Rather than give up, `data-diff` matches rows by position:

```console
$ data-diff demo/no-key-old.parquet demo/no-key-new.parquet
table_key([#row], basis: fallback)
row_edit(2, changes: 1)
```

You can also specify this manually

```console
$ data-diff demo/no-key-old.parquet demo/no-key-new.parquet --key '#row'
table_key([#row], basis: declared)
row_edit(2, changes: 1)
```

### When the whole file changed

Positional matching only tells a useful story when most of the file is the same file. Here nothing can identify a row *and* every cell disagrees, so enumerating edits would describe the matching rather than the data. When more than half of the cells change under a key `data-diff` chose itself, it says what the evidence actually supports — the file was regenerated:

```console
$ data-diff demo/regenerate-old.parquet demo/regenerate-new.parquet
table_key([#row], basis: fallback)
table_regenerate()
```

A key you declare is never second-guessed this way: with an explicit `--key`, the edits are reported in full however many there are.

## Fanout

We can still use a key, even if it's duplicated (up to 10%) in the new table, as this might indicate a join gone wrong:

```console
$ data-diff demo/fanout-old.parquet demo/fanout-new.parquet --key id
table_key([id], basis: declared)
row_fanout(4 -> [4, 5], changes: 1)
```

## Value edits

When cells change, `data-diff` reports the minimal set of rows and columns that accounts for them. For example, here four cells change in an L shape: `a` and `b` both change in row 1, and `c` changes in rows 2 and 3.

```console
$ data-diff demo/scatter-old.parquet demo/scatter-new.parquet --key id
table_key([id], basis: declared)
col_edit(c, changes: 2)
row_edit(1, changes: 2)
```

We'll also report a column whose type changed even when all of its values compare as equal:

```console
$ data-diff demo/types-old.parquet demo/types-new.parquet --key id
table_key([id], basis: declared)
col_edit(id, type: Int32 -> Int64)
col_edit(amount, type: Int32 -> Float64)
```

## Row and column order

If the position of rows and columns changes but the values stay the same, we just report that the order changed:

```console
$ data-diff demo/order-old.parquet demo/order-new.parquet --key id
table_key([id], basis: declared)
col_order(price, 3 -> 1)
row_order(3 -> 1)
```

## Renames and swaps

`data-diff` can detect renamed columns if all values are the same:

```console
$ data-diff demo/rename-old.parquet demo/rename-new.parquet --key id
table_key([id], basis: declared)
col_rename(amount -> total, basis: exact)
row_edit(2, changes: 1)
```

Or if a small fraction of values are different:

```console
$ data-diff demo/approx-rename-old.parquet demo/approx-rename-new.parquet --key id
table_key([id], basis: declared)
col_rename(amount -> total, basis: approximate)
row_edit(7, changes: 1)
```

Or if the values in two columns were swapped:

```console
$ data-diff demo/swap-old.parquet demo/swap-new.parquet --key id
table_key([id], basis: declared)
col_rename(price -> cost, basis: swapped)
col_rename(cost -> price, basis: swapped)
col_order(price, 3 -> 2)
```

A swap can even explain what looks like two columns changing type at once. Here `flag` appears to have become an integer column and `count` a boolean one, but each new column holds the other's old values, so the account that fits is an exchange — each column keeps its own type and its contents moved:

```console
$ data-diff demo/swap-types-old.parquet demo/swap-types-new.parquet --key id
table_key([id], basis: declared)
col_rename(flag -> count, basis: swapped)
col_rename(count -> flag, basis: swapped)
col_order(flag, 3 -> 2)
```

Or if you provide an explicit hint:

```console
$ data-diff demo/hint-rename-old.parquet demo/hint-rename-new.parquet --key id \
    --hint 'col_rename(discount -> markdown)'
table_key([id], basis: declared)
col_rename(discount -> markdown, basis: hinted)
col_edit(markdown, changes: 3)
```

## Beyond the core types

Dates, timestamps, decimals, binary, and nested values all participate. Here the `when` dates are diffed like any other column, while `flag` changed from an integer to a date — two types with no comparison between them — so its type change is the whole of its report: the values are never compared, so no `changes:` count is ever claimed:

```console
$ data-diff demo/temporal-old.parquet demo/temporal-new.parquet
table_key([id], basis: guessed, overlap: 1.00)
col_edit(flag, type: Int64 -> Date32)
row_edit(2, changes: 1)
```

### Retypes that keep their values

Where a decided rule connects the two types, a retyped column's values compare right across the retype — always exactly, never through a lossy conversion. Timestamps compare as instants across units and timezones, decimals meet the integers and doubles they equal, `Date32` meets `Date64`, and strings parse against dates, timestamps, and decimals under strict ISO 8601 and exact numeric grammars. Here `at` moved from milliseconds to microseconds with one genuinely edited value, which is caught across the unit change; `price` became a decimal column and `day`'s ISO strings became real dates, and every value survived both retypes, so the type changes are the whole of their reports:

```console
$ data-diff demo/promoted-old.parquet demo/promoted-new.parquet
table_key([id], basis: guessed, overlap: 1.00)
col_edit(at, type: "Timestamp(Millisecond, Some(\"UTC\"))" -> "Timestamp(Microsecond, Some(\"UTC\"))", changes: 1)
col_edit(price, type: Int64 -> "Decimal128(10, 2)")
col_edit(day, type: Utf8 -> Date32)
```

## One-sided diffs

A file that was just added, or just deleted, has nothing to compare against. Name the missing side `'#missing'` — quoted, since `#` starts a comment in most shells — and `data-diff` summarizes the file that exists: a table-level headline with the row count, then the columns. Every row is new (or gone) because the file is, so listing them would say nothing the headline doesn't:

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
