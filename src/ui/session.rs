//! The session: one pair of input tables, their diff, and the options that
//! produced it. The two maps every command derives from the diff — changed
//! cells by position, matched rows by new position — are built once here
//! rather than per request.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{Diff, DiffError, DiffOptions, Lookup, diff_tables, read_parquet};
use arrow_array::RecordBatch;

pub struct Session {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
    pub key: Vec<String>,
    pub hints: Vec<String>,
    pub old: RecordBatch,
    pub new: RecordBatch,
    pub diff: Diff,
    /// The changed cells as `(new_row, new_col) -> (old_row, old_col)`, all
    /// zero-based.
    pub cells: BTreeMap<(usize, usize), (usize, usize)>,
    /// Matched rows as `new_row -> old_row`, zero-based.
    pub matched: BTreeMap<usize, usize>,
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
        Ok(Session::new(old_path, new_path, key, hints, old, new, diff))
    }

    /// Assemble the session and its derived maps from the parts.
    pub fn new(
        old_path: &Path,
        new_path: &Path,
        key: Vec<String>,
        hints: Vec<String>,
        old: RecordBatch,
        new: RecordBatch,
        diff: Diff,
    ) -> Session {
        let cells = diff
            .cells
            .iter()
            .map(|cell| {
                let (old, new) = cell.positions();
                (
                    (new[0] as usize - 1, new[1] as usize - 1),
                    (old[0] as usize - 1, old[1] as usize - 1),
                )
            })
            .collect();
        let matched = diff
            .rows
            .matched
            .iter()
            .map(|row| {
                let (old, new) = row.positions();
                (new - 1, old - 1)
            })
            .collect();
        Session {
            old_path: old_path.to_path_buf(),
            new_path: new_path.to_path_buf(),
            key,
            hints,
            old,
            new,
            diff,
            cells,
            matched,
        }
    }

    pub fn lookup(&self) -> Lookup<'_> {
        Lookup::new(&self.old, &self.new)
    }
}
