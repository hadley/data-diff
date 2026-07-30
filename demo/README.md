# Demo datasets

Generate or refresh all fixtures from the repository root:

```console
cargo run --example generate_demo
```

Install the development build once:

```console
cargo install --path .
```

The commands below use that installed `data-diff` binary.

## Basic value edits with a guessed key

```console
data-diff demo/basic-old.parquet demo/basic-new.parquet
```

With no `--key`, `data-diff` guesses the key: `id` is unique on both sides and shares all three values, so the output leads with `col_key([id], basis: guessed, overlap: 1.00)`. All rows and columns retain identity. Row 2 changes in both `name` and `score`, which is summarized as one `row_edit(2, changes: 2)` — the count saying how many cells that one event stands for.

## Declaring the key explicitly

```console
data-diff demo/basic-old.parquet demo/basic-new.parquet --key id
```

The same comparison with a declared key produces the same operations behind a `col_key([id], basis: declared)` line. An explicit `--key` always overrides guessing, which matters when the strongest same-name overlap is not the real row identity.

## Scattered value edits

```console
data-diff demo/scatter-old.parquet demo/scatter-new.parquet --key id
```

Row 1 changes in columns `a` and `b`, while column `c` changes in rows 2 and 3.
The minimum summary therefore contains both `row_edit(1, changes: 2)` and
`col_edit(c, changes: 2)`. The counts overlap where the events cross, so they
describe their own row and their own column rather than dividing four cells
between them; here the two events happen to be disjoint.

## Mixed structural changes

```console
data-diff demo/mixed-old.parquet demo/mixed-new.parquet --key id
```

This pair reorders columns and rows, drops `product` and row `102`, adds `stock`
and row `104`, and changes the prices of the two matched rows. The human format
summarizes the two price cells as one `col_edit(price, changes: 2)`.

## Type-only changes

```console
data-diff demo/types-old.parquet demo/types-new.parquet --key id
```

`id` changes from `int32` to `int64`, and `amount` changes from `int32` to
`double`, while all canonical values remain equal. The result contains two
type-only column edits and no changed cells.

## A renamed column, worked out from the values

```console
data-diff demo/rename-old.parquet demo/rename-new.parquet --key id
```

Nothing here declares that `amount` became `total`. They are identified as one column because they hold the same value in every row the two files share, which is the strongest evidence available that they are the same column. The `row_edit(2, changes: 1)` belongs to `note`: a column identified this way agrees everywhere by definition, so it can never be the source of a value change.

```console
$ data-diff demo/rename-old.parquet demo/rename-new.parquet --key id
col_key([id], basis: declared)
col_rename(amount -> total, basis: exact)
row_edit(2, changes: 1)
```

Every rename says on what basis the two columns are one, because some of the ways of arriving at that are certainties and some are judgements, and the line reads the same either way without it. This one is `exact`: the values agree in every shared row. The rest of this file shows the other four — `approximate` next, then `swapped`, `declared`, and `hinted`.

## A renamed column that was also edited

```console
data-diff demo/approx-rename-old.parquet demo/approx-rename-new.parquet --key id
```

`amount` and `total` disagree in one of the eleven shared rows, so the evidence for identifying them is strong but no longer perfect. Ten in eleven is more than the nine in ten a rename is asked for, and far more than unrelated columns of distinct values would reach by chance, so they are identified anyway and the row they disagree in becomes a `row_edit(7, changes: 1)`. Unlike the exact case above, an approximately identified column can be the source of a value change: that is what makes it approximate.

Nothing here has to reach twenty rows or any other minimum. The threshold does impose one implicitly, though, since nine in ten has to be exceeded rather than met: below eleven rows, a single disagreement is already too many.

## Two columns that swapped

```console
data-diff demo/swap-old.parquet demo/swap-new.parquet --key id
```

Both `price` and `cost` change in every row, which read alone would be two columns rewritten from scratch. Each holds exactly what the other used to, so the likelier account is one exchange, and it is reported as the two renames it is, each saying on its own line that it is half of one:

```console
$ data-diff demo/swap-old.parquet demo/swap-new.parquet --key id
col_key([id], basis: declared)
col_rename(price -> cost, basis: swapped)
col_rename(cost -> price, basis: swapped)
col_order(price, 3 -> 2)
```

The `col_order()` line is not a separate claim. Identifying old `cost` with new `price` puts that column second where it used to be third, and column ordering reads positions off the identities like any other operation.

## A key column that was renamed

```console
data-diff demo/key-rename-old.parquet demo/key-rename-new.parquet --key customer_id/id
```

The key column is `customer_id` in the old file and `id` in the new one. A paired `--key` component names both, which identifies them as one column and lets the rows line up: the output is one `col_rename()` and the single row that changed.

Without the pair the rows no longer line up. Inference does still identify the two columns, their values agreeing in every row the files share, but it runs after the key has been resolved and so cannot supply one. The only guessable key is `amount`, and by the time the rename is worked out the rows have already been matched by it — which reports the changed row as a drop and an add rather than an edit:

```console
$ data-diff demo/key-rename-old.parquet demo/key-rename-new.parquet
col_key([amount], basis: guessed, overlap: 0.67)
col_rename(customer_id -> id, basis: exact)
row_drop(2)
row_add(2)
```

So the pair earns its keep even where inference would have found the rename anyway: naming both sides is what makes the identity available early enough to match rows with.

## A rename only you could know about

```console
data-diff demo/hint-rename-old.parquet demo/hint-rename-new.parquet --key id
```

`discount` became `markdown` and every one of its values changed at the same time. No evidence connects the two columns, so the diff reports exactly what it can see:

```console
$ data-diff demo/hint-rename-old.parquet demo/hint-rename-new.parquet --key id
col_key([id], basis: declared)
col_drop(discount)
col_add(markdown)
```

A hint supplies what the data cannot. It is written the way the output prints it, so the operation you want is the one you type:

```console
$ data-diff demo/hint-rename-old.parquet demo/hint-rename-new.parquet --key id \
    --hint 'col_rename(discount -> markdown)'
col_key([id], basis: declared)
col_rename(discount -> markdown, basis: hinted)
col_edit(markdown, changes: 3)
```

Note what the hint did *not* do. It asserted that the two columns are one, and nothing about their values, so the change it made visible is reported as an edit. Being unmatched is what had been hiding it: a dropped column has no cells to compare.

A hint you get wrong is reported rather than obeyed, and the comparison still runs:

```console
$ data-diff demo/hint-rename-old.parquet demo/hint-rename-new.parquet --key id \
    --hint 'col_rename(discount -> mrkdown)'
col_key([id], basis: declared)
hint_ignored(col_rename(discount -> mrkdown), missing: mrkdown)
col_drop(discount)
col_add(markdown)
```

For several hints, or for hints generated alongside a change, `--hints hints.txt` reads one per line and skips blank lines and `#` comments.

## A replacement that looks like a rename

```console
data-diff demo/replace-old.parquet demo/replace-new.parquet --key id
```

`region` went and `zone` arrived, and their values agree in every row. That is the strongest evidence there is that two columns are the same column, so inference identifies them:

```console
$ data-diff demo/replace-old.parquet demo/replace-new.parquet --key id
col_key([id], basis: declared)
col_rename(region -> zone, basis: exact)
```

Only you know the two have nothing to do with each other. `col_drop()` and `col_add()` reserve their columns as having no partner, which keeps them out of rename inference:

```console
$ data-diff demo/replace-old.parquet demo/replace-new.parquet --key id \
    --hint 'col_drop(region)' --hint 'col_add(zone)'
col_key([id], basis: declared)
col_drop(region)
col_add(zone)
```

Either hint alone produces the same two lines here, the column left over having nothing else to pair with. That is a fact about this pair of files rather than about the hints: give `zone` a second candidate in the old file and `col_drop(region)` alone would identify it with that one instead, since reserving a column says that column has no partner and nothing about any other. Supplying both is how you say the whole of what you mean.

## Saying which change it was

Where a change can be read two ways, `col_edit()` says which. The swap above is the first case: two columns each holding what the other used to is usually an exchange, and where it is not, saying one of them was edited withdraws the reading.

```console
$ data-diff demo/swap-old.parquet demo/swap-new.parquet --key id --hint 'col_edit(price)'
col_key([id], basis: declared)
col_edit(price, changes: 3)
col_edit(cost, changes: 3)
```

Naming one column is enough. An exchange takes two, so withdrawing either end leaves both columns to be described under their own names.

The second case is a rectangular change, which can be summarized by its rows or by its columns. The scatter fixture changes columns `a` and `b` in row 1, and column `c` in rows 2 and 3, and the smallest description mixes the two:

```console
$ data-diff demo/scatter-old.parquet demo/scatter-new.parquet --key id
col_key([id], basis: declared)
col_edit(c, changes: 2)
row_edit(1, changes: 2)
```

Hinting the two columns that row 1 accounts for takes their cells out of the reckoning, and what is left has no row worth naming:

```console
$ data-diff demo/scatter-old.parquet demo/scatter-new.parquet --key id \
    --hint 'col_edit(a)' --hint 'col_edit(b)'
col_key([id], basis: declared)
col_edit(a, changes: 1)
col_edit(b, changes: 1)
col_edit(c, changes: 2)
```

An edit hint asserts that something changed, so one naming a column that did not is ignored and reported like any other hint the data contradicts:

```console
$ data-diff demo/scatter-old.parquet demo/scatter-new.parquet --key id --hint 'col_edit(id)'
col_key([id], basis: declared)
hint_ignored(col_edit(id), unchanged)
col_edit(c, changes: 2)
row_edit(1, changes: 2)
```

## Bounded fanout

```console
data-diff demo/fanout-old.parquet demo/fanout-new.parquet --key id
```

Key `4` identifies one old row and two new rows, as a join that duplicated a row would produce. One of the ten shared keys is affected, which is exactly the 10% limit, so the declared key is kept and the duplication is reported as `row_fanout(4 -> [4, 5], changes: 1)`. The two new rows are not additions, and the values that differ between the old row and its new rows stay inside the event rather than becoming a `row_edit()`.

## A guessed key that fans out

```console
data-diff demo/guessed-fanout-old.parquet demo/guessed-fanout-new.parquet
```

With no `--key`, the only column that can identify rows is `id`, because `region` repeats in the old file. `id` duplicates one of its ten shared keys, which is within the limit, so it is guessed anyway and the duplication is reported as a fanout. A guessed key is held to the same limit as a declared one; above it the candidate is passed over rather than reported, and the comparison falls back to any other eligible column.

## Fanout too broad to be a fanout

```console
data-diff demo/fanout-broad-old.parquet demo/fanout-broad-new.parquet --key id
```

This intentionally fails. Key `1` is duplicated again, but here it is one of only two shared keys, so half the identity is ambiguous and the key reads as broken rather than as a fanout. The command exits non-zero and names both counts.
