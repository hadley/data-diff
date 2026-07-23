use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

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
    let _cli = Cli::parse();
    eprintln!("reconciliation is not implemented yet");
    ExitCode::FAILURE
}
