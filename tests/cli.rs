mod common;

use std::process::Command;

use test_support::table;

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
    let old = table! {
        "id" => [1, 2],
        "label" => ["a", "b"],
    };
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
    let old = table! {
        "id" => [1, 2, 3],
        "label" => ["a", "b", "c"],
    };
    let new = table! {
        "id" => [1, 2, 4],
        "label" => ["a", "B", "d"],
    };
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
    let old = table! { "id" => [1, 2] };
    let new = table! { "id" => [3, 4] };
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
    let old = table! {
        "id" => [1, 2, 4],
        "value" => [10, 20, 40],
        "drop" => ["x", "y", "z"],
    };
    let new = table! {
        "value" => [21, 11, 30],
        "id" => [2, 1, 3],
        "add" => ["a", "b", "c"],
    };
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
fn reports_a_bounded_fanout() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        "value" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
        "value" => [0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0],
    };
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
    row_fanout(4 -> [4, 5], values)
    "#);
}

#[test]
fn guesses_a_key_that_fans_out() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        "status" => ["x", "x", "x", "x", "x", "x", "x", "x", "x", "x"],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
        "status" => ["x", "x", "x", "x", "y", "x", "x", "x", "x", "x", "x"],
    };
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @r#"
    col_key(guessed: ["id"], overlap: 1.00)
    row_fanout(4 -> [4, 5], values)
    "#);
}

#[test]
fn rejects_a_declared_key_that_fans_out_too_broadly() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    common::write_parquet(&old_path, &table! { "id" => [1, 2] });
    common::write_parquet(&new_path, &table! { "id" => [1, 1, 2] });

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "id"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "declared key fans out for 1 of 2 shared key values, above the 10% limit; \
         supply a different --key\n"
    );
}

#[test]
fn empty_files_still_report_type_only_schema_changes() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = table! {
        "id" => i32[],
        "value" => i32[],
    };
    let new = table! {
        "id" => i64[],
        "value" => i64[],
    };
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
