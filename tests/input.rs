mod common;

use std::sync::Arc;

use arrow_array::{Int64Array, StringArray};
use data_diff::{DiffError, read_parquet};

#[test]
fn parquet_batches_become_one_table_in_file_order() {
    let dir = common::TempDir::new();
    let path = dir.path().join("rows.parquet");
    let first = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("label", Arc::new(StringArray::from(vec!["a", "b"]))),
    ]);
    let second = common::batch([
        ("id", Arc::new(Int64Array::from(vec![3]))),
        ("label", Arc::new(StringArray::from(vec!["c"]))),
    ]);
    common::write_parquet_batches(&path, &[first, second]);

    let table = read_parquet(&path).unwrap();

    assert_eq!(table.num_rows(), 3);
    assert_eq!(table.num_columns(), 2);
    let ids = table
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    assert_eq!(ids.values(), &[1, 2, 3]);
}

#[test]
fn unreadable_parquet_has_path_context() {
    let dir = common::TempDir::new();
    let path = dir.path().join("missing.parquet");

    let error = read_parquet(&path).unwrap_err();

    assert!(matches!(error, DiffError::ReadParquet { .. }));
    assert!(error.to_string().contains(&path.display().to_string()));
}

#[test]
fn invalid_parquet_has_path_context() {
    let dir = common::TempDir::new();
    let path = dir.path().join("invalid.parquet");
    std::fs::write(&path, b"not parquet").unwrap();

    let error = read_parquet(&path).unwrap_err();

    assert!(matches!(error, DiffError::ReadParquet { .. }));
    assert!(error.to_string().contains(&path.display().to_string()));
}

#[test]
fn zero_row_parquet_preserves_its_schema() {
    let dir = common::TempDir::new();
    let path = dir.path().join("empty.parquet");
    let empty = common::batch([("id", Arc::new(Int64Array::from(Vec::<Option<i64>>::new())))]);
    common::write_parquet(&path, &empty);

    let table = read_parquet(&path).unwrap();

    assert_eq!(table.num_rows(), 0);
    assert_eq!(table.schema().field(0).name(), "id");
}
