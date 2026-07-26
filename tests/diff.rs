mod common;

use std::sync::Arc;

use arrow_array::{Int32Array, Int64Array, StringArray};
use data_diff::{DiffError, DiffOptions, diff_tables};
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
    assert_eq!(
        value["summary"],
        json!({
            "optimal": true,
            "columns": [{
                "column": [2, 1],
                "type_changed": false,
                "values_changed": true
            }],
            "rows": []
        })
    );
}

#[test]
fn summary_combines_row_and_column_edits_minimally() {
    let old = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2, 3]))),
        ("a", Arc::new(Int64Array::from(vec![0, 0, 0]))),
        ("b", Arc::new(Int64Array::from(vec![0, 0, 0]))),
        ("c", Arc::new(Int64Array::from(vec![0, 0, 0]))),
    ]);
    let new = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2, 3]))),
        ("a", Arc::new(Int64Array::from(vec![1, 0, 0]))),
        ("b", Arc::new(Int64Array::from(vec![1, 0, 0]))),
        ("c", Arc::new(Int64Array::from(vec![0, 1, 1]))),
    ]);

    let value = serde_json::to_value(
        diff_tables(
            &old,
            &new,
            &DiffOptions {
                key: vec!["id".into()],
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        value["summary"],
        json!({
            "optimal": true,
            "columns": [{
                "column": 4,
                "type_changed": false,
                "values_changed": true
            }],
            "rows": [1]
        })
    );
}

#[test]
fn summary_forces_and_coalesces_type_edits() {
    let old = common::batch([
        ("id", Arc::new(Int32Array::from(vec![1, 2]))),
        ("value", Arc::new(Int32Array::from(vec![10, 20]))),
    ]);
    let new = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("value", Arc::new(Int64Array::from(vec![11, 20]))),
    ]);

    let value = serde_json::to_value(
        diff_tables(
            &old,
            &new,
            &DiffOptions {
                key: vec!["id".into()],
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        value["summary"],
        json!({
            "optimal": true,
            "columns": [
                {"column": 1, "type_changed": true, "values_changed": false},
                {"column": 2, "type_changed": true, "values_changed": true}
            ],
            "rows": []
        })
    );
    assert_eq!(value["cells"], json!([[1, 2]]));
}

#[test]
fn selected_row_retains_its_moved_coordinate() {
    let old = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("a", Arc::new(Int64Array::from(vec![10, 20]))),
        ("b", Arc::new(Int64Array::from(vec![30, 40]))),
    ]);
    let new = common::batch([
        ("id", Arc::new(Int64Array::from(vec![2, 1]))),
        ("a", Arc::new(Int64Array::from(vec![20, 11]))),
        ("b", Arc::new(Int64Array::from(vec![40, 31]))),
    ]);

    let value = serde_json::to_value(
        diff_tables(
            &old,
            &new,
            &DiffOptions {
                key: vec!["id".into()],
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(value["summary"]["columns"], json!([]));
    assert_eq!(value["summary"]["rows"], json!([[1, 2]]));
}

#[test]
fn default_options_guess_a_key_and_align_reordered_rows() {
    let old = common::batch([
        ("customer_id", Arc::new(Int64Array::from(vec![1, 2, 3]))),
        ("value", Arc::new(Int64Array::from(vec![10, 20, 30]))),
    ]);
    let new = common::batch([
        ("value", Arc::new(Int64Array::from(vec![30, 10, 21]))),
        ("customer_id", Arc::new(Int64Array::from(vec![3, 1, 2]))),
    ]);

    let value =
        serde_json::to_value(diff_tables(&old, &new, &DiffOptions::default()).unwrap()).unwrap();

    assert_eq!(
        value["key"],
        json!({
            "basis": "guessed",
            "columns": [[1, 2]],
            "overlap": 1.0,
        })
    );
    assert_eq!(value["rows"]["matched"], json!([[1, 2], [2, 3], [3, 1]]));
    assert_eq!(value["rows"]["added"], json!([]));
    assert_eq!(value["rows"]["dropped"], json!([]));
    assert_eq!(value["cells"], json!([[[2, 2], [3, 1]]]));
}

#[test]
fn an_explicit_key_overrides_a_stronger_eligible_guess() {
    let old = common::batch([
        ("weak", Arc::new(Int64Array::from(vec![1, 2, 3]))),
        ("strong", Arc::new(Int64Array::from(vec![10, 20, 30]))),
    ]);
    let new = common::batch([
        ("weak", Arc::new(Int64Array::from(vec![1, 4, 5]))),
        ("strong", Arc::new(Int64Array::from(vec![10, 20, 30]))),
    ]);
    let options = DiffOptions {
        key: vec!["weak".into()],
    };

    let value = serde_json::to_value(diff_tables(&old, &new, &options).unwrap()).unwrap();

    assert_eq!(value["key"], json!({"basis": "declared", "columns": [1]}));
    assert_eq!(value["rows"]["matched"], json!([1]));
}

#[test]
fn a_guessed_key_stays_out_of_top_level_changed_cells() {
    let old = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("value", Arc::new(Int64Array::from(vec![10, 20]))),
    ]);
    let new = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("value", Arc::new(Int64Array::from(vec![10, 21]))),
    ]);

    let value =
        serde_json::to_value(diff_tables(&old, &new, &DiffOptions::default()).unwrap()).unwrap();

    assert_eq!(value["key"]["basis"], json!("guessed"));
    assert_eq!(value["cells"], json!([[2, 2]]));
}

#[test]
fn automatic_resolution_without_an_eligible_key_is_an_error() {
    let empty = common::batch([("id", Arc::new(Int64Array::from(Vec::<i64>::new())))]);
    let rows = common::batch([("id", Arc::new(Int64Array::from(vec![1, 2])))]);
    let disjoint = common::batch([("id", Arc::new(Int64Array::from(vec![3, 4])))]);

    assert_eq!(
        diff_tables(&empty, &rows, &DiffOptions::default()).unwrap_err(),
        DiffError::MissingKey
    );
    assert_eq!(
        diff_tables(&rows, &empty, &DiffOptions::default()).unwrap_err(),
        DiffError::MissingKey
    );
    assert_eq!(
        diff_tables(&rows, &disjoint, &DiffOptions::default()).unwrap_err(),
        DiffError::MissingKey
    );
}

#[test]
fn repeated_guessed_comparisons_are_byte_identical() {
    let old = common::batch([
        ("a", Arc::new(Int64Array::from(vec![1, 2, 3]))),
        ("b", Arc::new(Int64Array::from(vec![7, 8, 9]))),
    ]);
    let new = common::batch([
        ("a", Arc::new(Int64Array::from(vec![2, 3, 4]))),
        ("b", Arc::new(Int64Array::from(vec![9, 8, 7]))),
    ]);
    let options = DiffOptions::default();

    let first = serde_json::to_vec(&diff_tables(&old, &new, &options).unwrap()).unwrap();
    let second = serde_json::to_vec(&diff_tables(&old, &new, &options).unwrap()).unwrap();

    assert_eq!(first, second);
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

#[test]
fn disjoint_keys_are_all_atomic_row_events() {
    let old = common::batch([("id", Arc::new(Int64Array::from(vec![1, 2])))]);
    let new = common::batch([("id", Arc::new(Int64Array::from(vec![3, 4])))]);

    let diff = diff_tables(
        &old,
        &new,
        &DiffOptions {
            key: vec!["id".into()],
        },
    )
    .unwrap();
    let value = serde_json::to_value(diff).unwrap();

    assert_eq!(value["rows"]["dropped"], json!([1, 2]));
    assert_eq!(value["rows"]["added"], json!([1, 2]));
    assert_eq!(value["rows"]["matched"], json!([]));
    assert_eq!(value["cells"], json!([]));
    assert_eq!(
        value["summary"],
        json!({"optimal": true, "columns": [], "rows": []})
    );
}

#[test]
fn empty_inputs_preserve_schema_and_classify_the_other_side() {
    let empty = common::batch([("id", Arc::new(Int64Array::from(Vec::<i64>::new())))]);
    let rows = common::batch([("id", Arc::new(Int64Array::from(vec![1, 2])))]);
    let options = DiffOptions {
        key: vec!["id".into()],
    };

    let added = serde_json::to_value(diff_tables(&empty, &rows, &options).unwrap()).unwrap();
    let dropped = serde_json::to_value(diff_tables(&rows, &empty, &options).unwrap()).unwrap();
    let both_empty = serde_json::to_value(diff_tables(&empty, &empty, &options).unwrap()).unwrap();

    assert_eq!(added["rows"]["added"], json!([1, 2]));
    assert_eq!(dropped["rows"]["dropped"], json!([1, 2]));
    assert_eq!(both_empty["schemas"]["old"][0]["name"], "id");
    assert_eq!(both_empty["rows"]["matched"], json!([]));
    assert_eq!(both_empty["cells"], json!([]));
    assert_eq!(
        both_empty["summary"],
        json!({"optimal": true, "columns": [], "rows": []})
    );
}
