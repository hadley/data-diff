//! Synthetic table pairs for the pipeline benchmarks.
//!
//! Each generator returns an `(old, new)` pair shaped like one scenario the
//! budgets are tuned against, parameterized by rows and columns. Every value
//! is a pure function of its coordinates — no random state anywhere — so a
//! benchmark measures the code and not the fixture, and two runs generate
//! byte-identical tables.
//!
//! Every pair carries an `id` column holding the row index, so a benchmark can
//! declare the key and measure reconciliation rather than key guessing.

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch};
use arrow_schema::{Field, Schema};

/// An identical pair: the floor every bound is compared against, the run
/// being one linear pass of cell comparison with nothing to infer.
pub fn identical(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let build = || {
        table(
            names("c", columns),
            (0..columns).map(|column| int_column(rows, |row| distinct(column, row))),
        )
    };
    (build(), build())
}

/// Every column renamed in place with distinct values: the positional
/// pre-pass's O(columns) case.
pub fn renamed_distinct(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let values = |prefix| {
        table(
            names(prefix, columns),
            (0..columns).map(|column| int_column(rows, move |row| distinct(column, row))),
        )
    };
    (values("old"), values("new"))
}

/// Every column renamed in place with one shared constant value: the
/// digest-collision adversary, where every candidate digests like every other
/// and only budgeted verification separates them.
pub fn renamed_constant(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let values = |prefix| {
        table(
            names(prefix, columns),
            (0..columns).map(|_| int_column(rows, |_| 0)),
        )
    };
    (values("old"), values("new"))
}

/// Dropped columns against added ones related by rename-and-modify: the
/// quadratic approximate case, no pair exact and every pair measured.
pub fn rename_and_modify(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let old = table(
        names("old", columns),
        (0..columns).map(|column| int_column(rows, move |row| distinct(column, row))),
    );
    // One row in twenty edited, which keeps each true pair above the 90%
    // agreement bar while denying the exact stage every candidate.
    let new = table(
        names("new", columns),
        (0..columns).map(|column| {
            int_column(rows, move |row| {
                let value = distinct(column, row);
                if row % 20 == 0 { value + 1 } else { value }
            })
        }),
    );
    (old, new)
}

/// Same-named column pairs whose contents were exchanged: the swap adversary,
/// every identity rewritten under its own name and every crossing measured.
pub fn swapped(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let old = table(
        names("c", columns),
        (0..columns).map(|column| int_column(rows, move |row| distinct(column, row))),
    );
    // Exchange within each adjacent pair; an odd last column keeps its values.
    let new = table(
        names("c", columns),
        (0..columns).map(|column| {
            let partner = if column % 2 == 0 {
                (column + 1).min(columns - 1)
            } else {
                column - 1
            };
            int_column(rows, move |row| distinct(partner, row))
        }),
    );
    (old, new)
}

/// Every non-key value changed: the summarization adversary, one edge per
/// cell in the minimum-cover graph.
pub fn full_rewrite(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let old = table(
        names("c", columns),
        (0..columns).map(|column| int_column(rows, move |row| distinct(column, row))),
    );
    let new = table(
        names("c", columns),
        (0..columns).map(|column| int_column(rows, move |row| distinct(column, row) + 1)),
    );
    (old, new)
}

/// A value distinct across both coordinates, so unrelated columns never agree
/// and a column's values never repeat: informative everywhere, colliding
/// nowhere. Deterministic by construction.
fn distinct(column: usize, row: usize) -> i64 {
    (row as i64) * 1_000_003 + (column as i64)
}

fn names(prefix: &str, columns: usize) -> Vec<String> {
    std::iter::once("id".to_owned())
        .chain((0..columns).map(|column| format!("{prefix}{column}")))
        .collect()
}

fn int_column(rows: usize, value: impl Fn(usize) -> i64) -> ArrayRef {
    Arc::new(Int64Array::from_iter_values((0..rows).map(value)))
}

/// Assemble the columns behind an `id` key column holding the row index.
fn table(names: Vec<String>, columns: impl Iterator<Item = ArrayRef>) -> RecordBatch {
    let rows_then_columns = columns.collect::<Vec<_>>();
    let rows = rows_then_columns
        .first()
        .map(|column| column.len())
        .unwrap_or(0);
    let mut arrays = vec![int_column(rows, |row| row as i64)];
    arrays.extend(rows_then_columns);
    let fields = names
        .iter()
        .zip(&arrays)
        .map(|(name, array)| Field::new(name, array.data_type().clone(), true))
        .collect::<Vec<_>>();
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
        .expect("generated columns share one row count")
}
