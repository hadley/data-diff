mod common;

use data_diff::{DiffError, DiffOptions, diff_tables};
use test_support::table;

#[test]
fn library_boundary_requires_a_key() {
    let old = table! {};
    let new = table! {};

    assert_eq!(
        diff_tables(&old, &new, &DiffOptions::default()),
        Err(DiffError::MissingKey)
    );
}

#[test]
fn parquet_fixture_helper_writes_a_file() {
    let dir = common::TempDir::new();
    let path = dir.path().join("empty.parquet");
    let table = table! { "id" => [1, 2] };

    common::write_parquet(&path, &table);

    assert!(path.is_file());
}
