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

With no `--key`, `data-diff` guesses the key: `id` is unique on both sides and shares all three values, so the output leads with `col_key(guessed: "id", overlap: 1.0)`. All rows and columns retain identity. Row 2 changes in both `name` and `score`, which is summarized as one `row_edit(2)`.

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

## Unsupported fanout

```console
data-diff demo/fanout-old.parquet demo/fanout-new.parquet --key id
```

This intentionally fails: key `1` occurs twice in the new file. Fanout is
planned after the MVP, so the command exits non-zero with an explanatory error.
