//! Semantic diffs for tabular data.

mod compare;
mod input;
mod key;
mod model;
mod rows;

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
    options: &DiffOptions,
) -> Result<Diff, DiffError> {
    validate_tables(old, new)?;
    let key = key::resolve_key(old, new, options)?;
    let _rows = rows::match_rows(&key);
    Err(DiffError::NotImplemented)
}
