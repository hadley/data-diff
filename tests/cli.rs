use std::process::Command;

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

    Usage: data-diff --key <KEY> <OLD> <NEW>

    Arguments:
      <OLD>  Original Parquet file
      <NEW>  Modified Parquet file

    Options:
          --key <KEY>  Comma-separated, same-name key columns
      -h, --help       Print help
      -V, --version    Print version
    ");
}
