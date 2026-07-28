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

    write(
        &output,
        "mixed-old.parquet",
        table! {
            "id" => [101, 102, 103],
            "product" => ["apple", "bread", "coffee"],
            "price" => [10.0, 20.0, 30.0],
        },
    );
    write(
        &output,
        "mixed-new.parquet",
        table! {
            "price" => [31.0, 11.0, 40.0],
            "id" => [103, 101, 104],
            "stock" => [8, 5, 12],
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

    // The only candidate that can identify rows is the one that fans out:
    // "region" repeats in `old` and so can never be a key.
    write(
        &output,
        "guessed-fanout-old.parquet",
        table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "region" => [
                "north", "south", "north", "south", "north",
                "south", "north", "south", "north", "south",
            ],
        },
    );
    write(
        &output,
        "guessed-fanout-new.parquet",
        table! {
            "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
            "region" => [
                "north", "south", "north", "south", "east", "north",
                "south", "north", "south", "north", "south",
            ],
        },
    );

    // One of two shared keys is 50%, which reads as a broken key rather than
    // as fanout.
    write(
        &output,
        "fanout-broad-old.parquet",
        table! {
            "id" => [1, 2],
            "value" => [10, 20],
        },
    );
    write(
        &output,
        "fanout-broad-new.parquet",
        table! {
            "id" => [1, 1, 2],
            "value" => [10, 11, 20],
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
