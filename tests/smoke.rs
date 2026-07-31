mod common;

use data_diff::{DiffOptions, KeyBasis, diff_tables};
use test_support::table;

#[test]
fn library_boundary_always_resolves_a_key() {
    let old = table! {};
    let new = table! {};

    // Two tables with nothing to key on still produce a diff: the chain runs
    // out of candidates and matches rows by position.
    let diff = diff_tables(&old, &new, &DiffOptions::default()).unwrap();

    assert_eq!(diff.key.basis, KeyBasis::Fallback);
    assert!(diff.key.columns.is_empty());
}

#[test]
fn parquet_fixture_helper_writes_a_file() {
    let dir = common::TempDir::new();
    let path = dir.path().join("empty.parquet");
    let table = table! { "id" => [1, 2] };

    common::write_parquet(&path, &table);

    assert!(path.is_file());
}
