mod common;

use std::sync::Arc;

use arrow_array::{Int64Array, StringArray};
use data_diff::{DiffOptions, diff_tables};
use serde_json::json;

#[test]
fn combines_schema_row_order_and_cell_changes() {
    let old = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("value", Arc::new(Int64Array::from(vec![10, 20]))),
        ("drop", Arc::new(StringArray::from(vec!["x", "y"]))),
    ]);
    let new = common::batch([
        ("value", Arc::new(Int64Array::from(vec![21, 11, 99]))),
        ("id", Arc::new(Int64Array::from(vec![2, 1, 3]))),
        ("add", Arc::new(StringArray::from(vec!["a", "b", "c"]))),
    ]);

    let diff = diff_tables(
        &old,
        &new,
        &DiffOptions {
            key: vec!["id".into()],
        },
    )
    .unwrap();
    let value = serde_json::to_value(diff).unwrap();

    assert_eq!(value["columns"]["identities"], json!([[1, 2], [2, 1]]));
    assert_eq!(value["columns"]["added"], json!([3]));
    assert_eq!(value["columns"]["dropped"], json!([3]));
    assert_eq!(
        value["columns"]["edited"],
        json!([{
            "column": [2, 1],
            "type_changed": false,
            "values_changed": true
        }])
    );
    assert_eq!(value["rows"]["added"], json!([3]));
    assert_eq!(value["rows"]["matched"], json!([[1, 2], [2, 1]]));
    assert_eq!(value["order"]["columns"], json!([[2, 1]]));
    assert_eq!(value["order"]["rows"], json!([[2, 1]]));
    assert_eq!(value["cells"], json!([[[1, 2], [2, 1]], [[2, 2], [1, 1]]]));
}

#[test]
fn repeated_comparisons_are_byte_identical() {
    let table = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("value", Arc::new(Int64Array::from(vec![10, 20]))),
    ]);
    let options = DiffOptions {
        key: vec!["id".into()],
    };

    let first = serde_json::to_vec(&diff_tables(&table, &table, &options).unwrap()).unwrap();
    let second = serde_json::to_vec(&diff_tables(&table, &table, &options).unwrap()).unwrap();

    assert_eq!(first, second);
}
