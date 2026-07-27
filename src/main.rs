use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use data_diff::{DiffOptions, diff_tables, read_parquet, write_human, write_json};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum OutputFormat {
    Json,
    #[default]
    Human,
}

#[derive(Debug, Parser)]
#[command(name = "data-diff", version, about = "Compare two tabular data files")]
struct Cli {
    /// Original Parquet file.
    old: PathBuf,
    /// Modified Parquet file.
    new: PathBuf,
    /// Comma-separated, same-name key columns; when omitted, a single-column key is guessed.
    #[arg(long, value_delimiter = ',')]
    key: Vec<String>,
    /// Output representation.
    #[arg(long, value_enum, default_value_t)]
    format: OutputFormat,
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
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    match cli.format {
        OutputFormat::Json => {
            write_json(&mut stdout, &diff)
                .map_err(|error| format!("cannot write JSON: {error}"))?;
        }
        OutputFormat::Human => {
            write_human(&mut stdout, &diff)
                .map_err(|error| format!("cannot write human output: {error}"))?;
        }
    }
    println!();
    Ok(())
}
