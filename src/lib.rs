//! Semantic diffs for tabular data.

mod input;
mod model;

use arrow_array::RecordBatch;

pub use input::{read_parquet, validate_tables};
pub use model::{
    CellCoordinate, ColumnEdit, ColumnSchema, ColumnsDiff, Coordinate, Diff, DiffError,
    DiffOptions, DuplicateColumnName, KeyBasis, KeyDiff, NormalizedType, OrderDiff, RowsDiff,
    Schemas, Side,
};

/// Compare two in-memory tables.
///
/// The API boundary is in place before reconciliation so tests and callers can
/// be built against the final ownership model.
pub fn diff_tables(
    old: &RecordBatch,
    new: &RecordBatch,
    _options: &DiffOptions,
) -> Result<Diff, DiffError> {
    validate_tables(old, new)?;
    Err(DiffError::NotImplemented)
}
