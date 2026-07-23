# Demo datasets

Generate or refresh all fixtures from the repository root:

```console
cargo run --example generate_demo
```

The commands below use the development build. Replace
`cargo run --quiet --` with `data-diff` if you installed the binary with
`cargo install --path .`.

## Basic value edits

```console
cargo run --quiet -- \
  demo/basic-old.parquet demo/basic-new.parquet \
  --key id
```

All rows and columns retain identity. Row 2 changes in both `name` and `score`,
so the result contains two changed cells and two value-edited columns.

## Mixed structural changes

```console
cargo run --quiet -- \
  demo/mixed-old.parquet demo/mixed-new.parquet \
  --key id
```

This pair reorders columns and rows, drops `product` and row `102`, adds `stock`
and row `104`, and changes the prices of the two matched rows.

## Type-only changes

```console
cargo run --quiet -- \
  demo/types-old.parquet demo/types-new.parquet \
  --key id
```

`id` changes from `int32` to `int64`, and `amount` changes from `int32` to
`double`, while all canonical values remain equal. The result contains two
type-only column edits and no changed cells.

## Unsupported fanout

```console
cargo run --quiet -- \
  demo/fanout-old.parquet demo/fanout-new.parquet \
  --key id
```

This intentionally fails: key `1` occurs twice in the new file. Fanout is
planned after the MVP, so the command exits non-zero with an explanatory error.
