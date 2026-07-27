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

With no `--key`, `data-diff` guesses the key: `id` is unique on both sides and shares all three values, so the output leads with `col_key(guessed: ["id"], overlap: 1.00)`. All rows and columns retain identity. Row 2 changes in both `name` and `score`, which is summarized as one `row_edit(2)`.

## Declaring the key explicitly

```console
data-diff demo/basic-old.parquet demo/basic-new.parquet --key id
```

The same comparison with a declared key produces the same operations behind a `col_key(declared: ["id"])` line. An explicit `--key` always overrides guessing, which matters when the strongest same-name overlap is not the real row identity.

## Scattered value edits

```console
data-diff demo/scatter-old.parquet demo/scatter-new.parquet --key id
```

Row 1 changes in columns `a` and `b`, while column `c` changes in rows 2 and 3.
The minimum summary therefore contains both `row_edit(1)` and
`col_edit("c", values)`.

## Mixed structural changes

```console
data-diff demo/mixed-old.parquet demo/mixed-new.parquet --key id
```

This pair reorders columns and rows, drops `product` and row `102`, adds `stock`
and row `104`, and changes the prices of the two matched rows. The human format
summarizes the two price cells as one `col_edit("price", values)`.

## Type-only changes

```console
data-diff demo/types-old.parquet demo/types-new.parquet --key id
```

`id` changes from `int32` to `int64`, and `amount` changes from `int32` to
`double`, while all canonical values remain equal. The result contains two
type-only column edits and no changed cells.

## Bounded fanout

```console
data-diff demo/fanout-old.parquet demo/fanout-new.parquet --key id
```

Key `4` identifies one old row and two new rows, as a join that duplicated a row would produce. One of the ten shared keys is affected, which is exactly the 10% limit, so the declared key is kept and the duplication is reported as `row_fanout(4 -> [4, 5], values)`. The two new rows are not additions, and the values that differ between the old row and its new rows stay inside the event rather than becoming a `row_edit()`.

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
