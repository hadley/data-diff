//! Semantic diffs for tabular data.

mod agreement;
mod cells;
mod compare;
mod hint;
mod human;
mod input;
mod key;
mod maps;
mod model;
mod order;
mod reconsider;
mod rename;
mod rows;
mod schema;
mod summary;
mod swap;

use arrow_array::RecordBatch;

pub use human::{write_human, write_human_one_sided};
pub use input::{MISSING_FILE, read_parquet, validate_tables};
pub use key::POSITIONAL_COMPONENT;
pub use model::{
    Budgets, CellCoordinate, ChangeMass, ColumnEdit, ColumnIdentity, ColumnSchema, ColumnsDiff,
    Coordinate, Diff, DiffError, DiffOptions, DuplicateColumnName, EditSummary, FanoutEvent,
    HintClaim, HintKind, HintNames, IdentityBasis, IncompleteStage, Issue, IssueKind, KeyBasis,
    KeyComponent, KeyDiff, KeyOverlap, KeyRejection, KeyRetraction, KeySubject, NormalizedType,
    OneSidedDiff, OrderDiff, RejectionReason, RowBudget, RowEdit, RowsDiff, Schemas, Side,
};

use crate::agreement::RowSample;
use crate::cells::CellChanges;
use crate::hint::{EditHint, PendingIssue};
use crate::key::ResolvedKey;
use crate::order::OrderMatches;
use crate::rows::RowMatches;
use crate::schema::ColumnMap;
use crate::summary::SummaryChanges;

/// Describe a table that was added, with no old side to compare against.
pub fn diff_added(new: &RecordBatch) -> Result<OneSidedDiff, DiffError> {
    one_sided(new, Side::New)
}

/// Describe a table that was removed, with no new side to compare against.
pub fn diff_removed(old: &RecordBatch) -> Result<OneSidedDiff, DiffError> {
    one_sided(old, Side::Old)
}

/// Two named entry points rather than one taking a side, so a caller cannot
/// pass the wrong one; two rather than an optional pair of tables in
/// `diff_tables`, which would make comparing nothing to nothing representable.
/// Validation is the same the two-sided path runs on this side.
fn one_sided(table: &RecordBatch, side: Side) -> Result<OneSidedDiff, DiffError> {
    Ok(OneSidedDiff {
        side,
        columns: input::validate_table(table, side)?,
        rows: table.num_rows(),
    })
}

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
    // key column the two files call different things. They resolve once: a hint
    // is a claim about columns, not about the key, and both passes below start
    // from the same claims.
    let declared = key::declared_components(&options.key)?;
    let hints = hint::resolve(
        old.schema_ref(),
        new.schema_ref(),
        &options.hints,
        key::claimed_identities(old.schema_ref(), new.schema_ref(), declared.components()),
    )?;
    let hint::Hints {
        map: hinted,
        edits,
        mut issues,
    } = hints;

    // Resolution cannot fail: a declared key this data will not support is
    // refused as a value, and the chain falls back to a guess and then to row
    // position, so the identities the declaration asserted survive it.
    let key = key::resolve_key(old, new, &declared, &hinted);
    let first = run_pass(old, new, key, hinted.clone(), &edits, &options.budgets);

    // Reconsider the key at most once: a straight second pass, never a loop.
    // Pass one's diff is evidence about its own key, and when that evidence
    // condemns a guess or surfaces a better candidate, the pipeline reruns
    // over the new key — from the hint map plus only the identities the
    // adopted key rests on, everything else being re-derived rather than
    // carried out of a matching that has just been set aside. Whatever the
    // second pass produces is final.
    let reconsider::Reconsideration {
        key: second,
        retraction,
    } = reconsider::reconsider(old, new, &first.key, &first.map, &first.rows, &first.cells);
    let pass = match second {
        Some(second) => {
            let mut map = hinted;
            for (old_index, new_index, basis) in
                reconsider::adopted_claims(old, new, &second, &first.map)
            {
                map.claim(old_index, new_index, basis);
            }
            run_pass(old, new, second, map, &edits, &options.budgets)
        }
        None => first,
    };

    // A final diff that accounts more of the files as changed than unchanged,
    // under a key the tool chose, is not credible as a story of edits. The
    // model below still holds everything; the marker is what tells the human
    // format to withhold the row-level findings it would misdescribe. A
    // declared key is the user's own matching, so its edits are real however
    // many there are.
    let regeneration = match pass.key.basis {
        KeyBasis::Declared => None,
        KeyBasis::Guessed | KeyBasis::Fallback => {
            let mass = reconsider::change_mass(old, new, pass.key.basis, &pass.rows, &pass.cells);
            reconsider::implausible(mass).then_some(mass)
        }
    };

    // Issues arise on both sides of the comparison, and the seam is nothing a
    // reader should have to see. Ordering by the hint each one concerns puts
    // them in the order the instructions were written.
    issues.extend(pass.edit_issues);
    issues.sort_by_key(|pending| pending.at);
    let issues = issues
        .into_iter()
        .map(|pending| pending.issue)
        .collect::<Vec<_>>();

    let changed_cells = pass.cells.changed_cells();
    Ok(Diff {
        schemas,
        columns: ColumnsDiff {
            identities: pass
                .map
                .pairs()
                .iter()
                .map(|pair| ColumnIdentity {
                    column: Coordinate::from_zero_based(pair.old, pair.new),
                    basis: pair.basis,
                })
                .collect(),
            added: one_based(&pass.map.added()),
            dropped: one_based(&pass.map.dropped()),
            edited: pass
                .cells
                .columns
                .iter()
                .map(|column| ColumnEdit {
                    column: Coordinate::from_zero_based(column.old, column.new),
                    type_changed: column.type_changed,
                    changes: column.rows.len(),
                })
                .collect(),
        },
        key: KeyDiff {
            basis: pass.key.basis,
            columns: pass
                .key
                .columns
                .iter()
                .map(|column| Coordinate::from_zero_based(column.old, column.new))
                .collect(),
            overlap: pass.key.overlap,
            rejection: pass.key.rejection,
            retraction,
        },
        rows: RowsDiff {
            added: one_based(&pass.rows.added),
            dropped: one_based(&pass.rows.dropped),
            matched: pass
                .rows
                .matched
                .iter()
                .map(|&(old, new)| Coordinate::from_zero_based(old, new))
                .collect(),
            fanout: pass
                .cells
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
            columns: pass
                .order
                .columns
                .iter()
                .map(|&(old, new)| Coordinate::from_zero_based(old, new))
                .collect(),
            rows: pass
                .order
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
        incomplete: pass.incomplete,
        issues,
        summary: EditSummary {
            optimal: pass.summary.optimal,
            columns: pass
                .summary
                .columns
                .iter()
                .map(|column| ColumnEdit {
                    column: Coordinate::from_zero_based(column.old, column.new),
                    type_changed: column.type_changed,
                    changes: column.changes,
                })
                .collect(),
            rows: pass
                .summary
                .rows
                .iter()
                .map(|row| RowEdit {
                    row: Coordinate::from_zero_based(row.old, row.new),
                    changes: row.changes,
                })
                .collect(),
        },
        regeneration,
    })
}

/// Everything one run of the pipeline learned below key resolution.
struct Pass {
    key: ResolvedKey,
    map: ColumnMap,
    rows: RowMatches,
    order: OrderMatches,
    cells: CellChanges,
    edit_issues: Vec<PendingIssue>,
    summary: SummaryChanges,
    incomplete: Vec<IncompleteStage>,
}

/// Run every stage below key resolution over one key.
///
/// The map leaves the hints here and does not go back: from now on it is this
/// pass's account of column identity, which every stage below both reads and
/// adds to. What remains of the hints is the edits, which are waiting for an
/// identity to attach to and are judged afresh in each pass against its cells.
///
/// Each pass runs under fresh budget counters, so a bounded first pass cannot
/// starve reconsideration's second, and the incompleteness the diff reports is
/// the final pass's own.
fn run_pass(
    old: &RecordBatch,
    new: &RecordBatch,
    key: ResolvedKey,
    mut map: ColumnMap,
    edits: &[EditHint],
    budgets: &Budgets,
) -> Pass {
    let rows = rows::match_rows(&key);
    schema::reconcile_schema(old, new, &key, &mut map);

    // One sample serves both inference stages: it is a function of the key
    // and the matching, not of the stage asking.
    let sample = RowSample::select(&key, &rows, budgets.agreement_rows);
    let mut incomplete = Vec::new();

    // The proportional budgets resolve against the table the linear pass
    // reads — matched rows times the wider side's columns — afresh each pass,
    // so a bounded first pass cannot starve reconsideration's second.
    let cells = rows
        .matched
        .len()
        .saturating_mul(old.num_columns().max(new.num_columns()));
    let rename_rows = budgets.rename_rows.resolve(cells);
    let swap_rows = budgets.swap_rows.resolve(cells);

    // Both resolve column identity, before ordering and cells go on to read it
    if !rename::infer(old, new, &mut map, &rows, &sample, rename_rows) {
        incomplete.push(IncompleteStage::Renames);
    }
    if !swap::infer(old, new, &mut map, &rows, edits, &sample, swap_rows) {
        incomplete.push(IncompleteStage::Swaps);
    }

    let order = order::detect_order(&map, &rows);
    let cells = cells::compare_cells(old, new, &map, &rows);
    // Edit hints are judged here rather than with the rest: whether the identity
    // they name exists needs inference, and whether it changed needs the cells.
    let (edit_issues, forced) = hint::validate_edits(edits, &map, &cells);
    let summary = summary::summarize(&cells, &forced, budgets.summary_cells);
    if !summary.optimal {
        incomplete.push(IncompleteStage::Summary);
    }
    Pass {
        key,
        map,
        rows,
        order,
        cells,
        edit_issues,
        summary,
        incomplete,
    }
}

fn one_based(indices: &[usize]) -> Vec<usize> {
    indices.iter().map(|index| index + 1).collect()
}
