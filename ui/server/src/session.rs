//! The session: one pair of input tables, their diff, and the options that
//! produced it, so applying hints can re-run in place.

use std::path::{Path, PathBuf};

use arrow_array::RecordBatch;
use data_diff::{diff_tables, read_parquet, Diff, DiffError, DiffOptions, Lookup};

pub struct Session {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
    pub key: Vec<String>,
    pub hints: Vec<String>,
    pub old: RecordBatch,
    pub new: RecordBatch,
    pub diff: Diff,
}

impl Session {
    pub fn open(
        old_path: &Path,
        new_path: &Path,
        key: Vec<String>,
        hints: Vec<String>,
    ) -> Result<Session, DiffError> {
        let old = read_parquet(old_path)?;
        let new = read_parquet(new_path)?;
        let diff = diff_tables(
            &old,
            &new,
            &DiffOptions {
                key: key.clone(),
                hints: hints.clone(),
                ..DiffOptions::default()
            },
        )?;
        Ok(Session {
            old_path: old_path.to_path_buf(),
            new_path: new_path.to_path_buf(),
            key,
            hints,
            old,
            new,
            diff,
        })
    }

    /// Re-run the diff with a new hint set, replacing the session's result.
    pub fn apply_hints(&mut self, hints: Vec<String>) -> Result<(), DiffError> {
        let diff = diff_tables(
            &self.old,
            &self.new,
            &DiffOptions {
                key: self.key.clone(),
                hints: hints.clone(),
                ..DiffOptions::default()
            },
        )?;
        self.hints = hints;
        self.diff = diff;
        Ok(())
    }

    pub fn lookup(&self) -> Lookup<'_> {
        Lookup::new(&self.old, &self.new)
    }
}
