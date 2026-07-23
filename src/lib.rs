//! Semantic diffs for tabular data.

mod cells;
mod compare;
mod input;
mod key;
mod model;
mod order;
mod rows;
mod schema;

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
    let rows = rows::match_rows(&key);
    let schema = schema::reconcile_schema(old, new, &key)?;
    let _order = order::detect_order(&schema, &rows);
    let _cells = cells::compare_cells(old, new, &schema, &rows);
    Err(DiffError::NotImplemented)
}
