use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use data_diff::{DiffOptions, diff_tables, read_parquet, write_human};

#[derive(Debug, Parser)]
#[command(name = "data-diff", version, about = "Compare two tabular data files")]
struct Cli {
    /// Original Parquet file.
    old: PathBuf,
    /// Modified Parquet file.
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

/// Read one hint per line, ignoring blanks and comments.
///
/// Comments earn their place because a hints file is the form a generated or
/// committed set takes, where saying why a rename was asserted is worth more
/// than the line it costs.
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
