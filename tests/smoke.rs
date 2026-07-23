mod common;

use std::sync::Arc;

use arrow_array::Int64Array;
use data_diff::{DiffError, DiffOptions, diff_tables};

#[test]
fn library_boundary_requires_a_key() {
    let old = common::empty_batch();
    let new = common::empty_batch();

    assert_eq!(
        diff_tables(&old, &new, &DiffOptions::default()),
        Err(DiffError::MissingKey)
    );
}

#[test]
fn parquet_fixture_helper_writes_a_file() {
    let dir = common::TempDir::new();
    let path = dir.path().join("empty.parquet");
    let table = common::batch([("id", Arc::new(Int64Array::from(vec![1, 2])))]);

    common::write_parquet(&path, &table);

    assert!(path.is_file());
}
