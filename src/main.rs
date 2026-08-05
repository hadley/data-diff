use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use data_diff::{
    DiffOptions, MISSING_FILE, diff_added, diff_removed, diff_tables, read_parquet, write_human,
    write_human_one_sided,
};

#[derive(Debug, Parser)]
#[command(name = "data-diff", version, about = "Compare two tabular data files")]
struct Cli {
    /// Original Parquet file; '#missing' when the file does not exist.
    old: PathBuf,
    /// Modified Parquet file; '#missing' when the file does not exist.
    new: PathBuf,
    /// Comma-separated key columns, each a shared name or an old/new pair; '#row' matches rows by position; when omitted, a single-column key is guessed.
    #[arg(long, value_delimiter = ',')]
    key: Vec<String>,
    /// A hint, written as the output prints it, such as 'col_rename(old -> new)'; repeatable.
    #[arg(long)]
    hint: Vec<String>,
    /// A file of hints, one per line, skipping blank lines and those starting with #.
    #[arg(long)]
    hints: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    // The sentinel is the exact bare argument: a path with anything more in
    // it, `./#missing` included, is an ordinary file.
    let old_missing = cli.old.as_os_str() == MISSING_FILE;
    let new_missing = cli.new.as_os_str() == MISSING_FILE;
    if old_missing || new_missing {
        return run_one_sided(cli, old_missing, new_missing);
    }

    let old = read_parquet(&cli.old).map_err(|error| error.to_string())?;
    let new = read_parquet(&cli.new).map_err(|error| error.to_string())?;
    // File hints first, then inline ones, so a repeated hint reports at the
    // position a reader would look for it. Order decides nothing else: the
    // library collapses duplicates and rejects contradictions as a group.
    let mut hints = match &cli.hints {
        Some(path) => read_hints(path)?,
        None => Vec::new(),
    };
    hints.extend(cli.hint);

    let diff = diff_tables(
        &old,
        &new,
        &DiffOptions {
            key: cli.key,
            hints,
            ..DiffOptions::default()
        },
    )
    .map_err(|error| error.to_string())?;
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    write_human(&mut stdout, &diff)
        .map_err(|error| format!("cannot write human output: {error}"))?;
    println!();
    Ok(())
}

/// Summarize the one file that exists.
///
/// The refusals are faults in the instruction, like a `--key` that cannot be
/// read, and come before anything is opened: two missing sides ask no
/// answerable question, and a key or hint is about a reconciliation that
/// cannot run.
fn run_one_sided(cli: Cli, old_missing: bool, new_missing: bool) -> Result<(), String> {
    if old_missing && new_missing {
        return Err(format!(
            "both sides are {MISSING_FILE:?}, so there is nothing to compare"
        ));
    }
    if !cli.key.is_empty() {
        return Err(format!(
            "a key cannot apply when one side is {MISSING_FILE:?}"
        ));
    }
    if !cli.hint.is_empty() || cli.hints.is_some() {
        return Err(format!(
            "hints cannot apply when one side is {MISSING_FILE:?}"
        ));
    }

    let diff = if old_missing {
        diff_added(&read_parquet(&cli.new).map_err(|error| error.to_string())?)
    } else {
        diff_removed(&read_parquet(&cli.old).map_err(|error| error.to_string())?)
    }
    .map_err(|error| error.to_string())?;
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    write_human_one_sided(&mut stdout, &diff)
        .map_err(|error| format!("cannot write human output: {error}"))?;
    println!();
    Ok(())
}

/// Read one hint per line, ignoring blanks and comments.
fn read_hints(path: &PathBuf) -> Result<Vec<String>, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect())
}
