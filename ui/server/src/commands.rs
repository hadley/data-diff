//! The command logic behind the HTTP API.
//!
//! These are plain functions over `&Session`; the routes in `http.rs` are
//! thin wrappers, so the whole surface is testable without a server. Every
//! list is paginated and every value is fetched through the session's
//! `Lookup` only for the page being returned.

use std::collections::{BTreeMap, BTreeSet};

use data_diff::{Diff, Side};

use crate::dto::{
    self, CellRowDto, ColumnCellDto, ColumnHeaderDto, ColumnRowDto, ColumnViewDto,
    EditedGroupSummaryDto, EditedGroupsDto, FanoutGroupDto, PageDto, RowLineDto, RowViewDto,
    SchemaRowDto, SessionSummaryDto, ValueDto,
};
use crate::session::Session;

/// One identity pair, zero-based positions converted once.
#[derive(Clone, Copy)]
struct Pair {
    old: usize,
    new: usize,
}

fn identities(diff: &Diff) -> Vec<Pair> {
    diff.columns
        .identities
        .iter()
        .map(|identity| {
            let (old, new) = identity.column.positions();
            Pair {
                old: old - 1,
                new: new - 1,
            }
        })
        .collect()
}

fn key_positions(diff: &Diff, side: Side) -> Vec<usize> {
    diff.key
        .columns
        .iter()
        .map(|column| {
            let (old, new) = column.positions();
            match side {
                Side::Old => old - 1,
                Side::New => new - 1,
            }
        })
        .collect()
}

/// The key values of one row as values, or the row's own position when the
/// key is positional — a positional key's identity is the position.
fn raw_key_values(session: &Session, side: Side, row: usize) -> Vec<data_diff::Value> {
    let positions = key_positions(&session.diff, side);
    if positions.is_empty() {
        return vec![data_diff::Value::Int64(row as i64 + 1)];
    }
    positions
        .iter()
        .map(|&column| {
            session
                .lookup()
                .value(side, row as u32 + 1, column as u32 + 1)
                .expect("model positions are in range")
        })
        .collect()
}

/// The key values of one row, as the frontend renders them.
fn key_values(session: &Session, side: Side, row: usize) -> Vec<ValueDto> {
    raw_key_values(session, side, row)
        .iter()
        .map(dto::value)
        .collect()
}

fn value_at(session: &Session, side: Side, row: usize, column: usize) -> ValueDto {
    dto::value(
        &session
            .lookup()
            .value(side, row as u32 + 1, column as u32 + 1)
            .expect("model positions are in range"),
    )
}

pub fn schema_panel(session: &Session, changed_only: bool) -> Vec<SchemaRowDto> {
    let diff = &session.diff;
    let key_pairs: BTreeSet<(usize, usize)> = diff
        .key
        .columns
        .iter()
        .map(|column| column.positions())
        .collect();

    // Rows sort by new-file position; a drop has none and interleaves at its
    // old position, ordered just after the row that held it.
    let mut rows: Vec<(f64, SchemaRowDto)> = Vec::new();
    for identity in &diff.columns.identities {
        let (old_pos, new_pos) = identity.column.positions();
        let old_schema = &diff.schemas.old[old_pos - 1];
        let new_schema = &diff.schemas.new[new_pos - 1];
        let renamed = old_schema.name != new_schema.name;
        let type_changed = old_schema.source_type != new_schema.source_type;
        let moved = old_pos != new_pos;
        let is_key = key_pairs.contains(&(old_pos, new_pos));
        // The key shows even in the changed-only view: it is the reader's
        // orientation for every other panel.
        if changed_only && !renamed && !type_changed && !moved && !is_key {
            continue;
        }
        rows.push((
            new_pos as f64,
            SchemaRowDto {
                status: "identity".to_owned(),
                key: is_key,
                old_pos: Some(old_pos as u32),
                old_name: Some(old_schema.name.clone()),
                new_pos: Some(new_pos as u32),
                new_name: Some(new_schema.name.clone()),
                moved,
                basis: renamed.then(|| identity.basis.name().to_owned()),
                type_change: type_changed.then(|| {
                    (
                        old_schema.source_type.clone(),
                        new_schema.source_type.clone(),
                    )
                }),
                source_type: Some(new_schema.source_type.clone()),
            },
        ));
    }
    for &added in &diff.columns.added {
        rows.push((
            added as f64,
            SchemaRowDto {
                status: "added".to_owned(),
                key: false,
                old_pos: None,
                old_name: None,
                new_pos: Some(added as u32),
                new_name: Some(diff.schemas.new[added - 1].name.clone()),
                moved: false,
                basis: None,
                type_change: None,
                source_type: Some(diff.schemas.new[added - 1].source_type.clone()),
            },
        ));
    }
    for &dropped in &diff.columns.dropped {
        rows.push((
            dropped as f64 + 0.5,
            SchemaRowDto {
                status: "dropped".to_owned(),
                key: false,
                old_pos: Some(dropped as u32),
                old_name: Some(diff.schemas.old[dropped - 1].name.clone()),
                new_pos: None,
                new_name: None,
                moved: false,
                basis: None,
                type_change: None,
                source_type: Some(diff.schemas.old[dropped - 1].source_type.clone()),
            },
        ));
    }
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("positions are finite"));
    rows.into_iter().map(|(_, row)| row).collect()
}

pub fn session_summary(session: &Session) -> SessionSummaryDto {
    let diff = &session.diff;
    SessionSummaryDto {
        old_path: session.old_path.display().to_string(),
        new_path: session.new_path.display().to_string(),
        cells: diff.cells.len(),
        optimal: diff.summary.optimal,
        edited_columns: diff
            .cells
            .iter()
            .map(|cell| cell.positions().1[1])
            .collect::<BTreeSet<_>>()
            .len(),
        edited_rows: diff
            .cells
            .iter()
            .map(|cell| cell.positions().1[0])
            .collect::<BTreeSet<_>>()
            .len(),
        cover_columns: diff.summary.columns.len(),
        cover_rows: diff.summary.rows.len(),
        key_columns: {
            let positions = key_positions(diff, Side::New);
            if positions.is_empty() {
                vec!["row".to_owned()]
            } else {
                positions
                    .iter()
                    .map(|&column| diff.schemas.new[column].name.clone())
                    .collect()
            }
        },
        added_rows: diff.rows.added.len(),
        dropped_rows: diff.rows.dropped.len(),
        moved_rows: diff.order.rows.len(),
        fanout_groups: diff.rows.fanout.len(),
        schema: schema_panel(session, true),
    }
}

/// Compare two key values as values, for the cell view's key sort:
/// numerically where both are numeric — so 9 sorts before 10 — and by
/// kind then display text across variants, which a type-changed key
/// column can produce.
fn value_cmp(a: &data_diff::Value, b: &data_diff::Value) -> std::cmp::Ordering {
    use data_diff::Value;
    use std::cmp::Ordering;
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Boolean(x), Value::Boolean(y)) => x.cmp(y),
        (Value::Int64(x), Value::Int64(y)) => x.cmp(y),
        (Value::Double(x), Value::Double(y)) => x.total_cmp(y),
        (Value::String(x), Value::String(y)) => x.cmp(y),
        (Value::Timestamp { value: x, .. }, Value::Timestamp { value: y, .. }) => x.cmp(y),
        (Value::Date32(x), Value::Date32(y)) => x.cmp(y),
        (Value::Date64(x), Value::Date64(y)) => x.cmp(y),
        (
            Value::Decimal128 {
                value: x,
                scale: sx,
                ..
            },
            Value::Decimal128 {
                value: y,
                scale: sy,
                ..
            },
        ) if sx == sy => x.cmp(y),
        (Value::Opaque(x), Value::Opaque(y)) => x.cmp(y),
        _ => {
            let (a, b) = (dto::value(a), dto::value(b));
            a.kind.cmp(&b.kind).then_with(|| a.text.cmp(&b.text))
        }
    }
}

/// Lexicographic order over compound keys.
fn key_cmp(a: &[data_diff::Value], b: &[data_diff::Value]) -> std::cmp::Ordering {
    a.iter()
        .zip(b)
        .map(|(x, y)| value_cmp(x, y))
        .find(|&order| order != std::cmp::Ordering::Equal)
        .unwrap_or_else(|| a.len().cmp(&b.len()))
}

/// The flat evidence table, one page: exactly `Diff::cells`, ordered by the
/// rows' key values so a row's cells stay together under its identity.
pub fn cells_page(
    session: &Session,
    sort: &str,
    page: usize,
    page_size: usize,
) -> PageDto<CellRowDto> {
    let diff = &session.diff;
    let mut entries: Vec<((usize, usize), (usize, usize))> = session
        .cells
        .iter()
        .map(|(&cell, &at)| (cell, at))
        .collect();

    match sort {
        "column" => entries.sort_by_key(|((row, col), _)| (*col, *row)),
        // By the row's key values, then the column, so a row's cells stay
        // together under its identity.
        _ => {
            let mut keys: BTreeMap<usize, Vec<data_diff::Value>> = BTreeMap::new();
            for &((new_row, _), _) in &entries {
                keys.entry(new_row)
                    .or_insert_with(|| raw_key_values(session, Side::New, new_row));
            }
            entries.sort_by(|((row_a, col_a), _), ((row_b, col_b), _)| {
                key_cmp(&keys[row_a], &keys[row_b]).then(col_a.cmp(col_b))
            });
        }
    }

    let total = entries.len();
    let items = entries
        .into_iter()
        .skip(page * page_size)
        .take(page_size)
        .map(|((new_row, new_col), (old_row, old_col))| {
            // Values, not DTOs, until the delta has had its look at them.
            let old = session
                .lookup()
                .value(Side::Old, old_row as u32 + 1, old_col as u32 + 1)
                .expect("model positions are in range");
            let new = session
                .lookup()
                .value(Side::New, new_row as u32 + 1, new_col as u32 + 1)
                .expect("model positions are in range");
            CellRowDto {
                key: key_values(session, Side::New, new_row),
                column: diff.schemas.new[new_col].name.clone(),
                delta: dto::delta(&old, &new),
                old: dto::value(&old),
                new: dto::value(&new),
            }
        })
        .collect();
    dto::page(items, total, page, page_size)
}

pub fn column_view(
    session: &Session,
    all_columns: bool,
    all_rows: bool,
    include_added_dropped: bool,
    page: usize,
    page_size: usize,
) -> ColumnViewDto {
    let diff = &session.diff;
    let cells = &session.cells;
    let matched = &session.matched;

    // The column view shows the cover's column edits, not every changed
    // column: a column the summary covers by its rows (a rectangle's r1–r5)
    // is the row view's story, and showing it here too would tell it twice.
    let edited: BTreeSet<usize> = diff
        .summary
        .columns
        .iter()
        .map(|edit| edit.column.positions().1 - 1)
        .collect();

    // Headers: edited identities as old/new pairs, unchanged identities and
    // added/dropped columns joining as singles, each group in schema order.
    let mut headers: Vec<ColumnHeaderDto> = Vec::new();
    enum Source {
        Pair(usize, usize),
        Single(Side, usize),
    }
    // Key columns are frozen at the left edge of every row, so filling in
    // unchanged columns must not repeat them as singles.
    let key_new: BTreeSet<usize> = key_positions(diff, Side::New).into_iter().collect();
    let mut sources: Vec<Source> = Vec::new();
    for pair in identities(diff) {
        if edited.contains(&pair.new) {
            sources.push(Source::Pair(pair.old, pair.new));
        } else if all_columns && !key_new.contains(&pair.new) {
            sources.push(Source::Single(Side::New, pair.new));
        }
    }
    if include_added_dropped {
        for &added in &diff.columns.added {
            sources.push(Source::Single(Side::New, added - 1));
        }
        for &dropped in &diff.columns.dropped {
            sources.push(Source::Single(Side::Old, dropped - 1));
        }
    }
    let added: BTreeSet<usize> = diff.columns.added.iter().map(|&c| c - 1).collect();
    let dropped: BTreeSet<usize> = diff.columns.dropped.iter().map(|&c| c - 1).collect();
    for source in &sources {
        headers.push(match *source {
            Source::Pair(_, new) => ColumnHeaderDto {
                name: diff.schemas.new[new].name.clone(),
                span: "pair".to_owned(),
                side: None,
                origin: "edited".to_owned(),
            },
            Source::Single(Side::New, new) => ColumnHeaderDto {
                name: diff.schemas.new[new].name.clone(),
                span: "single".to_owned(),
                side: Some("new".to_owned()),
                origin: if added.contains(&new) {
                    "added"
                } else {
                    "context"
                }
                .to_owned(),
            },
            Source::Single(Side::Old, old) => ColumnHeaderDto {
                name: diff.schemas.old[old].name.clone(),
                span: "single".to_owned(),
                side: Some("old".to_owned()),
                origin: if dropped.contains(&old) {
                    "dropped"
                } else {
                    "context"
                }
                .to_owned(),
            },
        });
    }

    let mut rows: Vec<usize> = if all_rows {
        matched.keys().copied().collect()
    } else {
        // Rows changed in a shown column; a row whose changes are all covered
        // by row edits has nothing to show here.
        cells
            .keys()
            .filter(|&(_, col)| edited.contains(col))
            .map(|&(row, _)| row)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    rows.sort_unstable();

    let total = rows.len();
    let items = rows
        .into_iter()
        .skip(page * page_size)
        .take(page_size)
        .map(|new_row| {
            let old_row = matched[&new_row];
            ColumnRowDto {
                key: key_values(session, Side::New, new_row),
                cells: sources
                    .iter()
                    .map(|source| match *source {
                        Source::Pair(old_col, new_col) => ColumnCellDto {
                            old: Some(value_at(session, Side::Old, old_row, old_col)),
                            new: Some(value_at(session, Side::New, new_row, new_col)),
                            changed: cells.contains_key(&(new_row, new_col)),
                        },
                        Source::Single(Side::New, new_col) => ColumnCellDto {
                            old: None,
                            new: Some(value_at(session, Side::New, new_row, new_col)),
                            changed: false,
                        },
                        Source::Single(Side::Old, old_col) => ColumnCellDto {
                            old: Some(value_at(session, Side::Old, old_row, old_col)),
                            new: None,
                            changed: false,
                        },
                    })
                    .collect(),
            }
        })
        .collect();
    ColumnViewDto {
        columns: headers,
        rows: dto::page(items, total, page, page_size),
    }
}

/// The columns a row-view section shows: those changed in the given rows, or
/// every identity, as new-side positions and names.
fn section_columns(
    session: &Session,
    rows: &BTreeSet<usize>,
    all_columns: bool,
) -> Vec<(usize, String)> {
    let diff = &session.diff;
    if all_columns {
        identities(diff)
            .into_iter()
            .map(|pair| (pair.new, diff.schemas.new[pair.new].name.clone()))
            .collect()
    } else {
        session
            .cells
            .keys()
            .filter(|&(row, _)| rows.contains(row))
            .map(|&(_, col)| col)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|col| (col, diff.schemas.new[col].name.clone()))
            .collect()
    }
}

/// One edited-row group: rows sharing one changed-column set.
struct EditedGroup {
    /// The shared changed columns, one-based new-side positions, as the
    /// model's `RowEdit` states them.
    columns: Vec<usize>,
    /// The group's rows, zero-based new-side and ascending.
    rows: Vec<usize>,
}

/// The edited-row grouping the sidebar's sub-entries and the edited view
/// both read, so their counts cannot disagree: the cover's row edits
/// ordered by new-side position, and rows with identical changed-column
/// sets collapsed into one group each — the summary's grouped `row_edit()`
/// line. Groups are ordered by where their first row occurs.
fn edited_grouping(session: &Session) -> (Vec<usize>, Vec<EditedGroup>) {
    let diff = &session.diff;
    let mut edits: Vec<&data_diff::RowEdit> = diff.summary.rows.iter().collect();
    edits.sort_by_key(|edit| edit.row.positions().1);
    let mut order = Vec::new();
    let mut groups: Vec<EditedGroup> = Vec::new();
    let mut by_columns: BTreeMap<&[usize], usize> = BTreeMap::new();
    for edit in edits {
        let new_row = edit.row.positions().1 - 1;
        order.push(new_row);
        let index = *by_columns.entry(&edit.columns).or_insert_with(|| {
            groups.push(EditedGroup {
                columns: edit.columns.clone(),
                rows: Vec::new(),
            });
            groups.len() - 1
        });
        groups[index].rows.push(new_row);
    }
    (order, groups)
}

/// The sidebar's "rows edited" sub-entries: one per group, titled by the
/// shared changed columns' names and counted in rows. Cheap — no values
/// are looked up, the grouping being a pure fact of the summary.
pub fn edited_groups(session: &Session) -> EditedGroupsDto {
    let (_, groups) = edited_grouping(session);
    EditedGroupsDto {
        groups: groups
            .into_iter()
            .map(|group| EditedGroupSummaryDto {
                columns: group
                    .columns
                    .iter()
                    .map(|&column| session.diff.schemas.new[column - 1].name.clone())
                    .collect(),
                rows: group.rows.len(),
            })
            .collect(),
    }
}

pub fn row_view_section(
    session: &Session,
    kind: &str,
    all_columns: bool,
    group: Option<usize>,
    page: usize,
    page_size: usize,
) -> RowViewDto {
    let diff = &session.diff;
    let cells = &session.cells;
    let matched = &session.matched;

    match kind {
        "edited" => {
            // The cover's row edits only: a row whose changes are all covered
            // by a `col_edit()` is the column view's story, and each event is
            // shown in exactly one place.
            let (order, groups) = edited_grouping(session);
            // The sidebar's sub-entries select one group by its index in the
            // shared grouping; the parent entry shows every edited row.
            let rows: &[usize] = match group {
                Some(index) => groups.get(index).map_or(&[], |group| &group.rows),
                None => &order,
            };
            let columns: Vec<(usize, String)> = if all_columns {
                identities(diff)
                    .into_iter()
                    .map(|pair| (pair.new, diff.schemas.new[pair.new].name.clone()))
                    .collect()
            } else if let Some(index) = group {
                groups
                    .get(index)
                    .map(|group| {
                        group
                            .columns
                            .iter()
                            .map(|&column| (column - 1, diff.schemas.new[column - 1].name.clone()))
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                section_columns(session, &rows.iter().copied().collect(), false)
            };
            // New-side column position to its identity pair, for the old line.
            let by_new: BTreeMap<usize, Pair> = identities(diff)
                .into_iter()
                .map(|pair| (pair.new, pair))
                .collect();

            // The table is the stacked old/new lines, two per edited row,
            // paginated by line so the windowed frontend sees one flat list.
            // Values are looked up only for the page's lines.
            let total = rows.len() * 2;
            let items = (page * page_size).min(total)..(page * page_size + page_size).min(total);
            let items = items
                .map(|line| {
                    let new_row = rows[line / 2];
                    let old_row = matched[&new_row];
                    let key = key_values(session, Side::New, new_row);
                    let changed: Vec<bool> = columns
                        .iter()
                        .map(|&(col, _)| cells.contains_key(&(new_row, col)))
                        .collect();
                    if line % 2 == 0 {
                        RowLineDto {
                            label: "old".to_owned(),
                            key,
                            values: columns
                                .iter()
                                .map(|&(col, _)| {
                                    value_at(session, Side::Old, old_row, by_new[&col].old)
                                })
                                .collect(),
                            changed,
                        }
                    } else {
                        RowLineDto {
                            label: "new".to_owned(),
                            key,
                            values: columns
                                .iter()
                                .map(|&(col, _)| {
                                    value_at(session, Side::New, new_row, by_new[&col].new)
                                })
                                .collect(),
                            changed,
                        }
                    }
                })
                .collect();
            RowViewDto {
                columns: columns.into_iter().map(|(_, name)| name).collect(),
                rows: Some(dto::page(items, total, page, page_size)),
                groups: None,
            }
        }
        "added" | "dropped" => {
            let (side, positions, schemas) = if kind == "added" {
                (Side::New, &diff.rows.added, &diff.schemas.new)
            } else {
                (Side::Old, &diff.rows.dropped, &diff.schemas.old)
            };
            // The key columns are frozen at the left edge of every row;
            // leaving them out of the values keeps them from showing twice.
            let key: BTreeSet<usize> = key_positions(diff, side).into_iter().collect();
            let shown: Vec<usize> = (0..schemas.len()).filter(|c| !key.contains(c)).collect();
            let total = positions.len();
            let items = positions
                .iter()
                .skip(page * page_size)
                .take(page_size)
                .map(|&position| {
                    let row = position - 1;
                    RowLineDto {
                        label: kind.to_owned(),
                        key: key_values(session, side, row),
                        values: shown
                            .iter()
                            .map(|&column| value_at(session, side, row, column))
                            .collect(),
                        changed: vec![false; shown.len()],
                    }
                })
                .collect();
            RowViewDto {
                columns: shown
                    .iter()
                    .map(|&column| schemas[column].name.clone())
                    .collect(),
                rows: Some(dto::page(items, total, page, page_size)),
                groups: None,
            }
        }
        "moved" => {
            let total = diff.order.rows.len();
            let items = diff
                .order
                .rows
                .iter()
                .skip(page * page_size)
                .take(page_size)
                .map(|coordinate| {
                    let (old, new) = coordinate.positions();
                    let position = |pos: usize| ValueDto {
                        kind: "int64".to_owned(),
                        text: pos.to_string(),
                    };
                    RowLineDto {
                        label: "moved".to_owned(),
                        key: key_values(session, Side::New, new - 1),
                        values: vec![position(old), position(new)],
                        changed: vec![false, false],
                    }
                })
                .collect();
            RowViewDto {
                columns: vec!["old position".to_owned(), "new position".to_owned()],
                rows: Some(dto::page(items, total, page, page_size)),
                groups: None,
            }
        }
        "fanout" => {
            let pairs = identities(diff);
            let by_new: BTreeMap<usize, Pair> =
                pairs.into_iter().map(|pair| (pair.new, pair)).collect();
            let columns: Vec<String> = by_new
                .values()
                .map(|pair| diff.schemas.new[pair.new].name.clone())
                .collect();

            let total = diff.rows.fanout.len();
            let groups = diff
                .rows
                .fanout
                .iter()
                .skip(page * page_size)
                .take(page_size)
                .map(|event| {
                    let changed: BTreeSet<(usize, usize)> = event
                        .cells
                        .iter()
                        .map(|cell| {
                            let (_, new) = cell.positions();
                            (new[0] as usize - 1, new[1] as usize - 1)
                        })
                        .collect();
                    let old_row = event.old - 1;
                    let mut lines = Vec::new();
                    lines.push(RowLineDto {
                        label: "old".to_owned(),
                        key: key_values(session, Side::Old, old_row),
                        values: by_new
                            .values()
                            .map(|pair| value_at(session, Side::Old, old_row, pair.old))
                            .collect(),
                        changed: vec![false; by_new.len()],
                    });
                    for (index, &new_position) in event.new.iter().enumerate() {
                        let new_row = new_position - 1;
                        lines.push(RowLineDto {
                            label: format!("new {}", index + 1),
                            key: key_values(session, Side::New, new_row),
                            values: by_new
                                .values()
                                .map(|pair| value_at(session, Side::New, new_row, pair.new))
                                .collect(),
                            changed: by_new
                                .keys()
                                .map(|&col| changed.contains(&(new_row, col)))
                                .collect(),
                        });
                    }
                    FanoutGroupDto {
                        old_row: event.old as u32,
                        new_rows: event.new.iter().map(|&row| row as u32).collect(),
                        lines,
                    }
                })
                .collect();
            RowViewDto {
                columns,
                rows: None,
                groups: Some(dto::page(groups, total, page, page_size)),
            }
        }
        _ => panic!("unknown row view section {kind:?}"),
    }
}
