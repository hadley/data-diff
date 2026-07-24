mod common;

use std::process::Command;
use std::sync::Arc;

use arrow_array::{Int32Array, Int64Array, StringArray};
use serde_json::json;

#[test]
fn help_describes_the_initial_interface() {
    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .arg("--help")
        .output()
        .expect("failed to run data-diff");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    insta::assert_snapshot!(stdout, @r"
    Compare two tabular data files

    Usage: data-diff [OPTIONS] --key <KEY> <OLD> <NEW>

    Arguments:
      <OLD>  Original Parquet file
      <NEW>  Modified Parquet file

    Options:
          --key <KEY>        Comma-separated, same-name key columns
          --format <FORMAT>  Output representation [default: human] [possible values: json, human]
      -h, --help             Print help
      -V, --version          Print version
    ");
}

#[test]
fn compares_two_parquet_files_as_json() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("label", Arc::new(StringArray::from(vec!["a", "b"]))),
    ]);
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &old);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "id", "--format", "json"])
        .output()
        .expect("failed to run data-diff");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    insta::assert_json_snapshot!(json, @r#"
    {
      "cells": [],
      "columns": {
        "added": [],
        "dropped": [],
        "edited": [],
        "identities": [
          1,
          2
        ]
      },
      "key": {
        "basis": "declared",
        "columns": [
          1
        ]
      },
      "order": {
        "columns": [],
        "rows": []
      },
      "rows": {
        "added": [],
        "dropped": [],
        "matched": [
          1,
          2
        ]
      },
      "schemas": {
        "new": [
          {
            "name": "id",
            "normalized_type": "int64",
            "source_type": "Int64"
          },
          {
            "name": "label",
            "normalized_type": "string",
            "source_type": "Utf8"
          }
        ],
        "old": [
          {
            "name": "id",
            "normalized_type": "int64",
            "source_type": "Int64"
          },
          {
            "name": "label",
            "normalized_type": "string",
            "source_type": "Utf8"
          }
        ]
      },
      "summary": {
        "columns": [],
        "optimal": true,
        "rows": []
      }
    }
    "#);
}

#[test]
fn failure_writes_context_only_to_stderr() {
    let dir = common::TempDir::new();
    let missing = dir.path().join("missing.parquet");
    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([missing.as_os_str(), missing.as_os_str()])
        .args(["--key", "id"])
        .output()
        .expect("failed to run data-diff");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("missing.parquet")
    );
}

#[test]
fn reports_mixed_changes_from_real_parquet_files() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
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
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "id", "--format", "json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(value["columns"]["added"], json!([3]));
    assert_eq!(value["columns"]["dropped"], json!([3]));
    assert_eq!(value["rows"]["added"], json!([3]));
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
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(r#""cells": [[[1, 2], [2, 1]], [[2, 2], [1, 1]]]"#));
}

#[test]
fn reports_mixed_changes_in_human_format() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2, 4]))),
        ("value", Arc::new(Int64Array::from(vec![10, 20, 40]))),
        ("drop", Arc::new(StringArray::from(vec!["x", "y", "z"]))),
    ]);
    let new = common::batch([
        ("value", Arc::new(Int64Array::from(vec![21, 11, 30]))),
        ("id", Arc::new(Int64Array::from(vec![2, 1, 3]))),
        ("add", Arc::new(StringArray::from(vec!["a", "b", "c"]))),
    ]);
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "id"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @r#"
    col_drop("drop")
    col_add("add")
    col_order("value", 2 -> 1)
    col_edit("value", values)
    row_drop(3)
    row_add(3)
    row_order(2 -> 1)
    "#);
}

#[test]
fn empty_files_still_report_type_only_schema_changes() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = common::batch([
        ("id", Arc::new(Int32Array::from(Vec::<i32>::new()))),
        ("value", Arc::new(Int32Array::from(Vec::<i32>::new()))),
    ]);
    let new = common::batch([
        ("id", Arc::new(Int64Array::from(Vec::<i64>::new()))),
        ("value", Arc::new(Int64Array::from(Vec::<i64>::new()))),
    ]);
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "id", "--format", "json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(value["rows"]["matched"], json!([]));
    assert_eq!(value["cells"], json!([]));
    assert_eq!(
        value["columns"]["edited"],
        json!([
            {"column": 1, "type_changed": true, "values_changed": false},
            {"column": 2, "type_changed": true, "values_changed": false}
        ])
    );
    assert_eq!(
        value["summary"],
        json!({
            "optimal": true,
            "columns": [
                {"column": 1, "type_changed": true, "values_changed": false},
                {"column": 2, "type_changed": true, "values_changed": false}
            ],
            "rows": []
        })
    );
}
