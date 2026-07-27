mod common;

use std::process::Command;
use std::sync::Arc;

use arrow_array::{Int32Array, Int64Array, StringArray};

#[test]
fn help_describes_the_initial_interface() {
    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .arg("--help")
        .output()
        .expect("failed to run data-diff");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)
        .expect("stdout is UTF-8")
        .replace("data-diff.exe", "data-diff");
    insta::assert_snapshot!(stdout, @r"
    Compare two tabular data files

    Usage: data-diff [OPTIONS] <OLD> <NEW>

    Arguments:
      <OLD>  Original Parquet file
      <NEW>  Modified Parquet file

    Options:
          --key <KEY>  Comma-separated, same-name key columns; when omitted, a single-column key is guessed
      -h, --help       Print help
      -V, --version    Print version
    ");
}

#[test]
fn compares_two_identical_parquet_files() {
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
        .args(["--key", "id"])
        .output()
        .expect("failed to run data-diff");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @r#"
    col_key(declared: ["id"])
    no_changes()
    "#);
}

#[test]
fn guesses_a_key_when_the_flag_is_omitted() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2, 3]))),
        ("label", Arc::new(StringArray::from(vec!["a", "b", "c"]))),
    ]);
    let new = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2, 4]))),
        ("label", Arc::new(StringArray::from(vec!["a", "B", "d"]))),
    ]);
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .output()
        .expect("failed to run data-diff");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @r#"
    col_key(guessed: ["id"], overlap: 0.67)
    row_drop(3)
    row_add(3)
    row_edit(2)
    "#);
}

#[test]
fn reports_a_missing_key_when_nothing_can_be_guessed() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = common::batch([("id", Arc::new(Int64Array::from(vec![1, 2])))]);
    let new = common::batch([("id", Arc::new(Int64Array::from(vec![3, 4])))]);
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .output()
        .expect("failed to run data-diff");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "no key was supplied and no eligible key could be guessed; supply --key\n"
    );
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
    col_key(declared: ["id"])
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
        .args(["--key", "id"])
        .output()
        .unwrap();

    assert!(output.status.success());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @r#"
    col_key(declared: ["id"])
    col_edit("id", type "Int32" -> "Int64")
    col_edit("value", type "Int32" -> "Int64")
    "#);
}
