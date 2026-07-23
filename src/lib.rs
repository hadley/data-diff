//! Semantic diffs for tabular data.

mod model;

use arrow_array::RecordBatch;

pub use model::{
    CellCoordinate, ColumnEdit, ColumnSchema, ColumnsDiff, Coordinate, Diff, DiffError,
    DiffOptions, KeyBasis, KeyDiff, NormalizedType, OrderDiff, RowsDiff, Schemas,
};

/// Compare two in-memory tables.
///
/// The API boundary is in place before reconciliation so tests and callers can
/// be built against the final ownership model.
pub fn diff_tables(
    _old: &RecordBatch,
    _new: &RecordBatch,
    _options: &DiffOptions,
) -> Result<Diff, DiffError> {
    Err(DiffError::NotImplemented)
}
