use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::{ArrayRef, Float64Array, Int32Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{Field, Schema};
use parquet::arrow::ArrowWriter;

fn main() {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demo");
    std::fs::create_dir_all(&output).unwrap();

    write(
        &output,
        "basic-old.parquet",
        table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2, 3]))),
            (
                "name",
                Arc::new(StringArray::from(vec!["Ada", "Ben", "Cy"])),
            ),
            ("score", Arc::new(Int64Array::from(vec![10, 20, 30]))),
        ]),
    );
    write(
        &output,
        "basic-new.parquet",
        table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2, 3]))),
            (
                "name",
                Arc::new(StringArray::from(vec!["Ada", "Bea", "Cy"])),
            ),
            ("score", Arc::new(Int64Array::from(vec![10, 25, 30]))),
        ]),
    );

    write(
        &output,
        "mixed-old.parquet",
        table(vec![
            ("id", Arc::new(Int64Array::from(vec![101, 102, 103]))),
            (
                "product",
                Arc::new(StringArray::from(vec!["apple", "bread", "coffee"])),
            ),
            (
                "price",
                Arc::new(Float64Array::from(vec![10.0, 20.0, 30.0])),
            ),
        ]),
    );
    write(
        &output,
        "mixed-new.parquet",
        table(vec![
            (
                "price",
                Arc::new(Float64Array::from(vec![31.0, 11.0, 40.0])),
            ),
            ("id", Arc::new(Int64Array::from(vec![103, 101, 104]))),
            ("stock", Arc::new(Int64Array::from(vec![8, 5, 12]))),
        ]),
    );

    write(
        &output,
        "types-old.parquet",
        table(vec![
            ("id", Arc::new(Int32Array::from(vec![1, 2, 3]))),
            ("amount", Arc::new(Int32Array::from(vec![10, 20, 30]))),
        ]),
    );
    write(
        &output,
        "types-new.parquet",
        table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2, 3]))),
            (
                "amount",
                Arc::new(Float64Array::from(vec![10.0, 20.0, 30.0])),
            ),
        ]),
    );

    write(
        &output,
        "fanout-old.parquet",
        table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2]))),
            ("value", Arc::new(Int64Array::from(vec![10, 20]))),
        ]),
    );
    write(
        &output,
        "fanout-new.parquet",
        table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 1, 2]))),
            ("value", Arc::new(Int64Array::from(vec![10, 11, 20]))),
        ]),
    );

    println!("wrote demo datasets to {}", output.display());
}

fn table(columns: Vec<(&str, ArrayRef)>) -> RecordBatch {
    let fields = columns
        .iter()
        .map(|(name, values)| Field::new(*name, values.data_type().clone(), true))
        .collect::<Vec<_>>();
    let arrays = columns.into_iter().map(|(_, values)| values).collect();
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).unwrap()
}

fn write(directory: &Path, name: &str, batch: RecordBatch) {
    let file = File::create(directory.join(name)).unwrap();
    let mut writer = ArrowWriter::try_new(file, batch.schema(), None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
}
