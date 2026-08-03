use std::fs::File;
use std::path::{Path, PathBuf};

use arrow_array::RecordBatch;
use parquet::arrow::ArrowWriter;
use test_support::table;

fn main() {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demo");
    std::fs::create_dir_all(&output).unwrap();

    write(
        &output,
        "basic-old.parquet",
        table! {
            "id" => [1, 2, 3],
            "name" => ["Ada", "Ben", "Cy"],
            "score" => [10, 20, 30],
        },
    );
    write(
        &output,
        "basic-new.parquet",
        table! {
            "id" => [1, 2, 3],
            "name" => ["Ada", "Bea", "Cy"],
            "score" => [10, 25, 30],
        },
    );

    write(
        &output,
        "scatter-old.parquet",
        table! {
            "id" => [1, 2, 3],
            "a" => [10, 20, 30],
            "b" => [40, 50, 60],
            "c" => [70, 80, 90],
        },
    );
    write(
        &output,
        "scatter-new.parquet",
        table! {
            "id" => [1, 2, 3],
            "a" => [11, 20, 30],
            "b" => [41, 50, 60],
            "c" => [70, 81, 91],
        },
    );

    // The same three columns and the same three rows in each file, both
    // rotated by one, so nothing changes but the order.
    write(
        &output,
        "order-old.parquet",
        table! {
            "id" => [101, 102, 103],
            "product" => ["apple", "bread", "coffee"],
            "price" => [10.0, 20.0, 30.0],
        },
    );
    write(
        &output,
        "order-new.parquet",
        table! {
            "price" => [30.0, 10.0, 20.0],
            "id" => [103, 101, 102],
            "product" => ["coffee", "apple", "bread"],
        },
    );

    write(
        &output,
        "types-old.parquet",
        table! {
            "id" => i32[1, 2, 3],
            "amount" => i32[10, 20, 30],
        },
    );
    write(
        &output,
        "types-new.parquet",
        table! {
            "id" => [1, 2, 3],
            "amount" => [10.0, 20.0, 30.0],
        },
    );

    // "amount" and "total" hold the same values in every matched row, which
    // is what identifies them as one renamed column.
    write(
        &output,
        "rename-old.parquet",
        table! {
            "id" => [1, 2, 3],
            "amount" => [10, 20, 30],
            "note" => ["ok", "ok", "ok"],
        },
    );
    write(
        &output,
        "rename-new.parquet",
        table! {
            "id" => [1, 2, 3],
            "total" => [10, 20, 30],
            "note" => ["ok", "checked", "ok"],
        },
    );

    // "amount" and "total" agree in ten of the eleven shared rows, which is
    // enough to identify them as one column that was renamed and edited.
    write(
        &output,
        "approx-rename-old.parquet",
        table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "amount" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        },
    );
    write(
        &output,
        "approx-rename-new.parquet",
        table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "total" => [10, 20, 30, 40, 50, 60, 99, 80, 90, 100, 110],
        },
    );

    // Each column holds what the other used to, so the likelier account is one
    // exchange rather than two columns rewritten from scratch.
    write(
        &output,
        "swap-old.parquet",
        table! {
            "id" => [1, 2, 3],
            "price" => [10, 20, 30],
            "cost" => [1000, 2000, 3000],
        },
    );
    write(
        &output,
        "swap-new.parquet",
        table! {
            "id" => [1, 2, 3],
            "price" => [1000, 2000, 3000],
            "cost" => [10, 20, 30],
        },
    );

    // The columns appear to have traded types, but each new column holds the
    // other's old values in its own representation, so the account that fits
    // is an exchange rather than two impossible retypes.
    write(
        &output,
        "swap-types-old.parquet",
        table! {
            "id" => [1, 2, 3],
            "flag" => [true, false, true],
            "count" => [1000, 2000, 3000],
        },
    );
    write(
        &output,
        "swap-types-new.parquet",
        table! {
            "id" => [1, 2, 3],
            "flag" => [1000, 2000, 3000],
            "count" => [true, false, true],
        },
    );

    // A date column compares exactly within its type, while "flag" changed to
    // a type the matrix cannot relate to its old one and reads as unrelated
    // columns.
    write(
        &output,
        "temporal-old.parquet",
        table! {
            "id" => [1, 2, 3],
            "when" => date32[19700, 19701, 19702],
            "flag" => [0, 1, 0],
        },
    );
    write(
        &output,
        "temporal-new.parquet",
        table! {
            "id" => [1, 2, 3],
            "when" => date32[19700, 19725, 19702],
            "flag" => date32[19800, 19801, 19802],
        },
    );

    // The key column is called something different in each file, so only a
    // paired --key component can line these rows up.
    write(
        &output,
        "key-rename-old.parquet",
        table! {
            "customer_id" => [1, 2, 3],
            "amount" => [10, 20, 30],
        },
    );
    write(
        &output,
        "key-rename-new.parquet",
        table! {
            "id" => [1, 2, 3],
            "amount" => [10, 25, 30],
        },
    );

    // "discount" became "markdown" and every value changed with it, so no
    // evidence connects the two columns and only a hint can.
    write(
        &output,
        "hint-rename-old.parquet",
        table! {
            "id" => [1, 2, 3],
            "discount" => [10, 20, 30],
            "note" => ["ok", "ok", "ok"],
        },
    );
    write(
        &output,
        "hint-rename-new.parquet",
        table! {
            "id" => [1, 2, 3],
            "markdown" => [99, 98, 97],
            "note" => ["ok", "ok", "ok"],
        },
    );

    // Nothing here can identify a row: both columns repeat a value in `old`,
    // so neither is eligible and rows are matched by position.
    write(
        &output,
        "no-key-old.parquet",
        table! {
            "region" => ["north", "north", "south"],
            "reading" => [11.4, 11.4, 9.8],
        },
    );
    write(
        &output,
        "no-key-new.parquet",
        table! {
            "region" => ["north", "north", "south"],
            "reading" => [11.4, 12.5, 9.8],
        },
    );

    // Nothing can identify a row here either, and beyond that nothing agrees:
    // every cell of the positional matching differs, so the diff is not
    // credible as a story of edits and is reported as a regeneration.
    write(
        &output,
        "regenerate-old.parquet",
        table! {
            "batch" => ["a", "a", "b", "b"],
            "reading" => [0.31, 0.48, 0.79, 0.66],
        },
    );
    write(
        &output,
        "regenerate-new.parquet",
        table! {
            "batch" => ["c", "c", "d", "d"],
            "reading" => [0.52, 0.13, 0.44, 0.97],
        },
    );

    // One of ten shared keys is exactly the 10% limit, so this pair is
    // retained and reported as a fanout.
    write(
        &output,
        "fanout-old.parquet",
        table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "value" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100],
        },
    );
    write(
        &output,
        "fanout-new.parquet",
        table! {
            "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
            "value" => [10, 20, 30, 40, 41, 50, 60, 70, 80, 90, 100],
        },
    );

    println!("wrote demo datasets to {}", output.display());
}

fn write(directory: &Path, name: &str, batch: RecordBatch) {
    let file = File::create(directory.join(name)).unwrap();
    let mut writer = ArrowWriter::try_new(file, batch.schema(), None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
}
