//! Semantic diffs for tabular data.

mod agreement;
mod cells;
mod compare;
mod hint;
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
    CellCoordinate, ColumnEdit, ColumnIdentity, ColumnSchema, ColumnsDiff, Coordinate, Diff,
    DiffError, DiffOptions, DuplicateColumnName, EditSummary, FanoutEvent, HintClaim, HintKind,
    HintNames, IdentityBasis, Issue, IssueKind, KeyBasis, KeyDiff, KeyOverlap, NormalizedType,
    OrderDiff, RowsDiff, Schemas, Side,
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
    // Hints resolve first, and against what the key already claims rather than
    // beside it, so a rename can be asserted in time to identify rows through a
    // key column the two files call different things.
    let components = key::declared_components(&options.key)?;
    let hints = hint::resolve(
        old.schema_ref(),
        new.schema_ref(),
        &options.hints,
        key::claimed_identities(old.schema_ref(), new.schema_ref(), &components),
    )?;
    let key = key::resolve_key(old, new, &components, &hints.map)?;
    let rows = rows::match_rows(&key);
    // The map leaves the hints here and does not go back: from now on it is
    // reconciliation's account of column identity, which every stage below both
    // reads and adds to. What remains of the hints is the edits, which are
    // waiting for an identity to attach to, and the issues raised so far.
    let hint::Hints {
        mut map,
        edits,
        mut issues,
    } = hints;
    schema::reconcile_schema(old, new, &key, &mut map)?;

    // Both resolve column identity, before ordering and cells go on to read it
    rename::infer(old, new, &mut map, &rows);
    swap::infer(old, new, &mut map, &rows, &edits);

    let order = order::detect_order(&map, &rows);
    let cells = cells::compare_cells(old, new, &map, &rows);
    // Edit hints are judged here rather than with the rest: whether the identity
    // they name exists needs inference, and whether it changed needs the cells.
    let (edit_issues, forced) = hint::validate_edits(&edits, &map, &cells);
    let summary = summary::summarize(&cells, &forced);
    let changed_cells = cells.changed_cells();

    // Issues arise on both sides of the comparison, and the seam is nothing a
    // reader should have to see. Ordering by the hint each one concerns puts
    // them in the order the instructions were written.
    issues.extend(edit_issues);
    issues.sort_by_key(|pending| pending.at);
    let issues = issues
        .into_iter()
        .map(|pending| pending.issue)
        .collect::<Vec<_>>();

    Ok(Diff {
        schemas,
        columns: ColumnsDiff {
            identities: map
                .pairs()
                .iter()
                .map(|pair| ColumnIdentity {
                    column: Coordinate::from_zero_based(pair.old, pair.new),
                    basis: pair.basis,
                })
                .collect(),
            added: one_based(&map.added()),
            dropped: one_based(&map.dropped()),
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
        issues,
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
