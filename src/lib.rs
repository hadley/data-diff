//! Semantic diffs for tabular data.

mod cells;
mod compare;
mod human;
mod input;
mod json;
mod key;
mod model;
mod order;
mod rows;
mod schema;
mod summary;

use arrow_array::RecordBatch;

pub use human::write_human;
pub use input::{read_parquet, validate_tables};
pub use json::write_json;
pub use model::{
    CellCoordinate, ColumnEdit, ColumnSchema, ColumnsDiff, Coordinate, Diff, DiffError,
    DiffOptions, DuplicateColumnName, EditSummary, KeyBasis, KeyDiff, NormalizedType, OrderDiff,
    RowsDiff, Schemas, Side,
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
    let schemas = validate_tables(old, new)?;
    let key = key::resolve_key(old, new, options)?;
    let rows = rows::match_rows(&key);
    let schema = schema::reconcile_schema(old, new, &key)?;
    let order = order::detect_order(&schema, &rows);
    let cells = cells::compare_cells(old, new, &schema, &rows);
    let summary = summary::summarize(&cells);

    Ok(Diff {
        schemas,
        columns: ColumnsDiff {
            identities: schema
                .identities
                .iter()
                .map(|column| Coordinate::from_zero_based(column.old, column.new))
                .collect(),
            added: one_based(&schema.added),
            dropped: one_based(&schema.dropped),
            edited: cells
                .columns
                .iter()
                .map(|column| ColumnEdit {
                    column: Coordinate::from_zero_based(column.old, column.new),
                    type_changed: column.type_changed,
                    values_changed: column.values_changed,
                })
                .collect(),
        },
        key: KeyDiff {
            basis: KeyBasis::Declared,
            columns: key
                .columns
                .iter()
                .map(|column| Coordinate::from_zero_based(column.old, column.new))
                .collect(),
        },
        rows: RowsDiff {
            added: one_based(&rows.added),
            dropped: one_based(&rows.dropped),
            matched: rows
                .matched
                .iter()
                .map(|&(old, new)| Coordinate::from_zero_based(old, new))
                .collect(),
        },
        order: OrderDiff {
            columns: order
                .columns
                .iter()
                .map(|&(old, new)| Coordinate::from_zero_based(old, new))
                .collect(),
            rows: order
                .rows
                .iter()
                .map(|&(old, new)| Coordinate::from_zero_based(old, new))
                .collect(),
        },
        cells: cells
            .cells
            .iter()
            .map(|cell| {
                CellCoordinate::from_zero_based(
                    cell.old_row,
                    cell.old_column,
                    cell.new_row,
                    cell.new_column,
                )
            })
            .collect(),
        summary: EditSummary {
            optimal: summary.optimal,
            columns: summary
                .columns
                .iter()
                .map(|column| ColumnEdit {
                    column: Coordinate::from_zero_based(column.old, column.new),
                    type_changed: column.type_changed,
                    values_changed: column.values_changed,
                })
                .collect(),
            rows: summary
                .rows
                .iter()
                .map(|&(old, new)| Coordinate::from_zero_based(old, new))
                .collect(),
        },
    })
}

fn one_based(indices: &[usize]) -> Vec<usize> {
    indices.iter().map(|index| index + 1).collect()
}
