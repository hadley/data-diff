//! Semantic diffs for tabular data.

mod agreement;
mod cells;
mod compare;
mod human;
mod input;
mod key;
mod model;
mod order;
mod rename;
mod rows;
mod schema;
mod summary;
mod swap;

use arrow_array::RecordBatch;

pub use human::write_human;
pub use input::{read_parquet, validate_tables};
pub use model::{
    CellCoordinate, ColumnEdit, ColumnSchema, ColumnsDiff, Coordinate, Diff, DiffError,
    DiffOptions, DuplicateColumnName, EditSummary, FanoutEvent, KeyBasis, KeyDiff, KeyOverlap,
    NormalizedType, OrderDiff, RowsDiff, Schemas, Side,
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
    let mut schema = schema::reconcile_schema(old, new, &key)?;

    // Both resolve column identity, before ordering and cells go on to read it
    rename::infer(old, new, &mut schema, &rows);
    swap::infer(old, new, &mut schema, &rows);

    let order = order::detect_order(&schema, &rows);
    let cells = cells::compare_cells(old, new, &schema, &rows);
    let summary = summary::summarize(&cells);
    let changed_cells = cells.changed_cells();

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
                    values_changed: column.values_changed(),
                })
                .collect(),
        },
        key: KeyDiff {
            basis: key.basis,
            columns: key
                .columns
                .iter()
                .map(|column| Coordinate::from_zero_based(column.old, column.new))
                .collect(),
            overlap: key.overlap,
        },
        rows: RowsDiff {
            added: one_based(&rows.added),
            dropped: one_based(&rows.dropped),
            matched: rows
                .matched
                .iter()
                .map(|&(old, new)| Coordinate::from_zero_based(old, new))
                .collect(),
            fanout: cells
                .fanout
                .iter()
                .map(|group| FanoutEvent {
                    old: group.old + 1,
                    new: one_based(&group.new),
                    cells: group
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
                })
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
        cells: changed_cells
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
