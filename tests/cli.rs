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
    insta::assert_snapshot!(stdout, @"
    Compare two tabular data files

    Usage: data-diff [OPTIONS] <OLD> <NEW>

    Arguments:
      <OLD>  Original Parquet file
      <NEW>  Modified Parquet file

    Options:
          --key <KEY>      Comma-separated key columns, each a shared name or an old/new pair; '#row' matches rows by position; when omitted, a single-column key is guessed
          --hint <HINT>    A hint, written as the output prints it, such as 'col_rename(old -> new)'; repeatable
          --hints <HINTS>  A file of hints, one per line, skipping blank lines and those starting with #
      -h, --help           Print help
      -V, --version        Print version
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
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: declared)
    no_changes()
    ");
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
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: guessed, overlap: 0.67)
    row_drop(3)
    row_add(3)
    row_edit(2, changes: 1)
    ");
}

#[test]
fn falls_back_to_row_position_when_nothing_can_be_guessed() {
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

    // The two files share no key value, so nothing can be guessed and rows are
    // paired by position instead. Nothing went wrong, so there is no separator.
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([#row], basis: fallback)
    col_edit(id, changes: 2)
    ");
}

#[test]
fn declaring_row_position_reaches_the_same_key_deliberately() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    common::write_parquet(&old_path, &table! { "id" => [1, 2] });
    common::write_parquet(&new_path, &table! { "id" => [3, 4] });

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "#row"])
        .output()
        .unwrap();

    // The same key as the fallback above, and only the basis differs: one was
    // asked for and the other was arrived at.
    assert!(output.status.success());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([#row], basis: declared)
    col_edit(id, changes: 2)
    ");
}

#[test]
fn row_position_cannot_be_compounded_with_a_column() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    common::write_parquet(&old_path, &table! { "id" => [1, 2] });
    common::write_parquet(&new_path, &table! { "id" => [1, 2] });

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "id,#row"])
        .output()
        .unwrap();

    // A fault in the --key string itself, which stays fatal: there is no key to
    // fall back from.
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "key component \"#row\" matches rows by position and cannot be combined \
         with a column\n"
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
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: declared)
    col_drop(drop)
    col_add(add)
    col_order(value, 2 -> 1)
    col_edit(value, changes: 2)
    row_drop(3)
    row_add(3)
    row_order(2 -> 1)
    ");
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
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: declared)
    row_fanout(4 -> [4, 5], changes: 1)
    ");
}

#[test]
fn infers_a_rename_from_the_values() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = table! {
        "id" => [1, 2, 3],
        "amount" => [10, 20, 30],
        "note" => ["a", "b", "c"],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "total" => [10, 20, 30],
        "note" => ["a", "B", "c"],
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
    // No new rendering was needed: the rename falls out of an identity whose
    // two ends carry different names.
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: declared)
    col_rename(amount -> total, basis: exact)
    row_edit(2, changes: 1)
    ");
}

#[test]
fn infers_a_rename_that_carried_an_edit() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "amount" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "total" => [10, 20, 30, 40, 50, 60, 99, 80, 90, 100, 110],
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
    // The columns disagree in one row, which exact inference read as proof
    // that they were unrelated. The rename now absorbs the row it explains.
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: declared)
    col_rename(amount -> total, basis: approximate)
    row_edit(7, changes: 1)
    ");
}

#[test]
fn infers_a_swap_between_two_rewritten_columns() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = table! {
        "id" => [1, 2, 3],
        "price" => [10, 20, 30],
        "cost" => [1000, 2000, 3000],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "price" => [1000, 2000, 3000],
        "cost" => [10, 20, 30],
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
    // Two columns that each changed in every row become one exchange, and the
    // move falls out of it: the column holding the prices is now second.
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: declared)
    col_rename(price -> cost, basis: swapped)
    col_rename(cost -> price, basis: swapped)
    col_order(price, 3 -> 2)
    ");
}

#[test]
fn accepts_a_paired_key_component() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = table! {
        "customer_id" => [1, 2, 3],
        "value" => [10, 20, 30],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "value" => [10, 21, 30],
    };
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "customer_id/id"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([customer_id -> id], basis: declared)
    col_rename(customer_id -> id, basis: declared)
    row_edit(2, changes: 1)
    ");
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
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: guessed, overlap: 1.00)
    row_fanout(4 -> [4, 5], changes: 1)
    ");
}

#[test]
fn reports_a_declared_key_that_fans_out_too_broadly() {
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

    // The declared key is refused rather than fatal. Uniqueness and fanout
    // belong to the whole key, so the subject is bracketed, and the comparison
    // continues on what can identify rows instead.
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    key_invalid([id], reason: excessive_fanout)
    ----
    col_key([#row], basis: fallback)
    row_add(3)
    row_edit(2, changes: 1)
    ");
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
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: declared)
    col_edit(id, type: Int32 -> Int64)
    col_edit(value, type: Int32 -> Int64)
    ");
}

#[test]
fn accepts_a_hint_for_a_rename_no_evidence_could_show() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = table! {
        "id" => [1, 2, 3],
        "discount" => [10, 20, 30],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "markdown" => [99, 98, 97],
    };
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "id"])
        .args(["--hint", r#"col_rename("discount" -> "markdown")"#])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    // The hint is written the way this very output prints it, and asserts
    // identity only: the values that changed are still reported.
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: declared)
    col_rename(discount -> markdown, basis: hinted)
    col_edit(markdown, changes: 3)
    ");
}

#[test]
fn reads_hints_from_a_file_with_comments_and_blank_lines() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let hints_path = dir.path().join("hints.txt");
    let old = table! {
        "id" => [1, 2],
        "discount" => [10, 20],
        "note" => ["a", "b"],
    };
    let new = table! {
        "id" => [1, 2],
        "markdown" => [99, 98],
        "comment" => ["x", "y"],
    };
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);
    std::fs::write(
        &hints_path,
        "# renamed when the discount scheme changed\ncol_rename(discount -> markdown)\n\ncol_rename(note -> comment)\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "id"])
        .args([std::ffi::OsStr::new("--hints"), hints_path.as_os_str()])
        .output()
        .unwrap();

    assert!(output.status.success());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: declared)
    col_rename(discount -> markdown, basis: hinted)
    col_rename(note -> comment, basis: hinted)
    row_edit(1, changes: 2)
    row_edit(2, changes: 2)
    ");
}

#[test]
fn reports_an_ignored_hint_beside_one_that_applied() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = table! {
        "id" => [1, 2],
        "discount" => [10, 20],
        "note" => ["a", "b"],
    };
    let new = table! {
        "id" => [1, 2],
        "markdown" => [99, 98],
        "comment" => ["x", "y"],
    };
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "id"])
        .args(["--hint", "col_rename(discount -> mrkdown)"])
        .args(["--hint", "col_rename(note -> comment)"])
        .output()
        .unwrap();

    // A hint the data contradicts is reported and skipped; it is not a failure,
    // so the status stays zero and the rest of the run stands.
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    hint_ignored(col_rename(discount -> mrkdown), missing: mrkdown)
    ----
    col_key([id], basis: declared)
    col_rename(note -> comment, basis: hinted)
    col_drop(discount)
    col_add(markdown)
    col_edit(comment, changes: 2)
    ");
}

#[test]
fn chooses_replacement_over_a_rename_when_told_to() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = table! {
        "id" => [1, 2, 3],
        "region" => ["north", "south", "east"],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "zone" => ["north", "south", "east"],
    };
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "id"])
        .args(["--hint", "col_drop(region)"])
        .args(["--hint", "col_add(zone)"])
        .output()
        .unwrap();

    // The values agree everywhere, so inference would call this one renamed
    // column. Reserving both endpoints says it is two.
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: declared)
    col_drop(region)
    col_add(zone)
    ");
}

#[test]
fn withdraws_a_swap_when_told_the_column_was_edited() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = table! {
        "id" => [1, 2],
        "price" => [10, 20],
        "cost" => [30, 40],
    };
    let new = table! {
        "id" => [1, 2],
        "price" => [30, 40],
        "cost" => [10, 20],
    };
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "id"])
        .args(["--hint", "col_edit(price)"])
        .output()
        .unwrap();

    // Without the hint each column holds what the other used to, which reads
    // as an exchange and prints two col_rename() lines.
    assert!(output.status.success());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    col_key([id], basis: declared)
    col_edit(price, changes: 2)
    col_edit(cost, changes: 2)
    ");
}

#[test]
fn reports_an_edit_hint_the_data_does_not_bear_out() {
    let dir = common::TempDir::new();
    let old_path = dir.path().join("old.parquet");
    let new_path = dir.path().join("new.parquet");
    let old = table! {
        "id" => [1, 2],
        "value" => [10, 20],
        "note" => ["a", "b"],
    };
    let new = table! {
        "id" => [1, 2],
        "value" => [10, 20],
        "note" => ["a", "z"],
    };
    common::write_parquet(&old_path, &old);
    common::write_parquet(&new_path, &new);

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args([old_path.as_os_str(), new_path.as_os_str()])
        .args(["--key", "id"])
        .args(["--hint", "col_edit(value)"])
        .args(["--hint", "col_edit(note)"])
        .output()
        .unwrap();

    // Nothing about "value" changed, so that instruction is dropped and
    // reported while the one beside it applies. Both are judged after the
    // comparison, and the exit status is unaffected either way.
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"
    hint_ignored(col_edit(value), reason: unchanged)
    ----
    col_key([id], basis: declared)
    col_edit(note, changes: 1)
    ");
}
