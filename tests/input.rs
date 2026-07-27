mod common;

use data_diff::{DiffError, read_parquet};
use test_support::table;

#[test]
fn parquet_batches_become_one_table_in_file_order() {
    let dir = common::TempDir::new();
    let path = dir.path().join("rows.parquet");
    let first = table! {
        "id" => [1, 2],
        "label" => ["a", "b"],
    };
    let second = table! {
        "id" => [3],
        "label" => ["c"],
    };
    common::write_parquet_batches(&path, &[first, second]);

    let table = read_parquet(&path).unwrap();

    assert_eq!(table.num_rows(), 3);
    assert_eq!(table.num_columns(), 2);
    assert_eq!(table.column(0), table! { "id" => [1, 2, 3] }.column(0));
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
    let empty = table! { "id" => i64[] };
    common::write_parquet(&path, &empty);

    let table = read_parquet(&path).unwrap();

    assert_eq!(table.num_rows(), 0);
    assert_eq!(table.schema().field(0).name(), "id");
}
