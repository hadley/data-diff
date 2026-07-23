use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use data_diff::{DiffOptions, diff_tables, read_parquet};

#[derive(Debug, Parser)]
#[command(name = "data-diff", version, about = "Compare two tabular data files")]
struct Cli {
    /// Original Parquet file.
    old: PathBuf,
    /// Modified Parquet file.
    new: PathBuf,
    /// Comma-separated, same-name key columns.
    #[arg(long, value_delimiter = ',', required = true)]
    key: Vec<String>,
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
    let diff = diff_tables(&old, &new, &DiffOptions { key: cli.key })
        .map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(std::io::stdout().lock(), &diff)
        .map_err(|error| format!("cannot write JSON: {error}"))?;
    println!();
    Ok(())
}
