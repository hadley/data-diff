//! The command logic behind the HTTP API.
//!
//! These are plain functions over `&Session`; the routes in `http.rs` are
//! thin wrappers, so the whole surface is testable without a server. Every
//! list is paginated and every value is fetched through the session's
//! `Lookup` only for the page being returned.

use std::collections::{BTreeMap, BTreeSet};

use data_diff::{Diff, Side};

use crate::dto::{
    self, CellRowDto, ColumnCellDto, ColumnHeaderDto, ColumnRowDto, ColumnViewDto, FanoutGroupDto,
    PageDto, RowLineDto, RowViewDto, SchemaRowDto, SessionSummaryDto, ValueDto,
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

/// The changed cells as `(new_row, new_col) -> (old_row, old_col)`, all
/// zero-based.
fn cell_map(diff: &Diff) -> BTreeMap<(usize, usize), (usize, usize)> {
    diff.cells
        .iter()
        .map(|cell| {
            let (old, new) = cell.positions();
            (
                (new[0] as usize - 1, new[1] as usize - 1),
                (old[0] as usize - 1, old[1] as usize - 1),
            )
        })
        .collect()
}

/// Matched rows as `new_row -> old_row`, zero-based.
fn matched_by_new(diff: &Diff) -> BTreeMap<usize, usize> {
    diff.rows
        .matched
        .iter()
        .map(|row| {
            let (old, new) = row.positions();
            (new - 1, old - 1)
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

/// The key values of one row, or the row's own position when the key is
/// positional — a positional key's identity is the position.
fn key_values(session: &Session, side: Side, row: usize) -> Vec<ValueDto> {
    let positions = key_positions(&session.diff, side);
    if positions.is_empty() {
        return vec![ValueDto {
            kind: "int64".to_owned(),
            text: (row + 1).to_string(),
        }];
    }
    positions
        .iter()
        .map(|&column| {
            dto::value(
                &session
                    .lookup()
                    .value(side, row as u32 + 1, column as u32 + 1)
                    .expect("model positions are in range"),
            )
        })
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
        if changed_only && !renamed && !type_changed && !moved {
            continue;
        }
        rows.push((
            new_pos as f64,
            SchemaRowDto {
                status: "identity".to_owned(),
                key: key_pairs.contains(&(old_pos, new_pos)),
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

/// A deterministic order for key values, for the cell view's key sort:
/// kind first, then the text form.
fn value_order(values: &[ValueDto]) -> Vec<(String, String)> {
    values
        .iter()
        .map(|value| (value.kind.clone(), value.text.clone()))
        .collect()
}

pub fn cells_page(
    session: &Session,
    column: Option<u32>,
    row: Option<u32>,
    sort: &str,
    page: usize,
    page_size: usize,
) -> PageDto<CellRowDto> {
    let diff = &session.diff;
    let mut entries: Vec<((usize, usize), (usize, usize))> = cell_map(diff)
        .into_iter()
        .filter(|((new_row, new_col), _)| {
            column.is_none_or(|column| *new_col == column as usize - 1)
                && row.is_none_or(|row| *new_row == row as usize - 1)
        })
        .collect();

    match sort {
        "column" => entries.sort_by_key(|((row, col), _)| (*col, *row)),
        // By the row's key values, then the column, so a row's cells stay
        // together under its identity.
        _ => {
            let mut keys: BTreeMap<usize, Vec<(String, String)>> = BTreeMap::new();
            for &((new_row, _), _) in &entries {
                keys.entry(new_row)
                    .or_insert_with(|| value_order(&key_values(session, Side::New, new_row)));
            }
            entries.sort_by(|((row_a, col_a), _), ((row_b, col_b), _)| {
                keys[row_a].cmp(&keys[row_b]).then(col_a.cmp(col_b))
            });
        }
    }

    let total = entries.len();
    let items = entries
        .into_iter()
        .skip(page * page_size)
        .take(page_size)
        .map(|((new_row, new_col), (old_row, old_col))| CellRowDto {
            key: key_values(session, Side::New, new_row),
            column: diff.schemas.new[new_col].name.clone(),
            old: value_at(session, Side::Old, old_row, old_col),
            new: value_at(session, Side::New, new_row, new_col),
            row: new_row as u32 + 1,
            column_pos: new_col as u32 + 1,
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
    let cells = cell_map(diff);
    let matched = matched_by_new(diff);

    let edited: BTreeSet<usize> = diff
        .columns
        .edited
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
    for source in &sources {
        headers.push(match *source {
            Source::Pair(_, new) => ColumnHeaderDto {
                name: diff.schemas.new[new].name.clone(),
                span: "pair".to_owned(),
                side: None,
            },
            Source::Single(Side::New, new) => ColumnHeaderDto {
                name: diff.schemas.new[new].name.clone(),
                span: "single".to_owned(),
                side: Some("new".to_owned()),
            },
            Source::Single(Side::Old, old) => ColumnHeaderDto {
                name: diff.schemas.old[old].name.clone(),
                span: "single".to_owned(),
                side: Some("old".to_owned()),
            },
        });
    }

    let mut rows: Vec<usize> = if all_rows {
        matched.keys().copied().collect()
    } else {
        cells
            .keys()
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

/// The columns a row-view section shows: changed identity columns, or every
/// identity, as new-side positions and names.
fn section_columns(diff: &Diff, all_columns: bool) -> Vec<(usize, String)> {
    if all_columns {
        identities(diff)
            .into_iter()
            .map(|pair| (pair.new, diff.schemas.new[pair.new].name.clone()))
            .collect()
    } else {
        cell_map(diff)
            .keys()
            .map(|&(_, col)| col)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|col| (col, diff.schemas.new[col].name.clone()))
            .collect()
    }
}

pub fn row_view_section(
    session: &Session,
    kind: &str,
    all_columns: bool,
    page: usize,
    page_size: usize,
) -> RowViewDto {
    let diff = &session.diff;
    let cells = cell_map(diff);
    let matched = matched_by_new(diff);

    match kind {
        "edited" => {
            let columns = section_columns(diff, all_columns);
            // New-side column position to its identity pair, for the old line.
            let by_new: BTreeMap<usize, Pair> = identities(diff)
                .into_iter()
                .map(|pair| (pair.new, pair))
                .collect();
            let changed_rows: Vec<usize> = cells
                .keys()
                .map(|&(row, _)| row)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();

            let mut lines = Vec::new();
            for &new_row in &changed_rows {
                let old_row = matched[&new_row];
                let key = key_values(session, Side::New, new_row);
                let mut old_line = RowLineDto {
                    label: "old".to_owned(),
                    key: key.clone(),
                    values: Vec::new(),
                    changed: Vec::new(),
                };
                let mut new_line = RowLineDto {
                    label: "new".to_owned(),
                    key,
                    values: Vec::new(),
                    changed: Vec::new(),
                };
                for &(col, _) in &columns {
                    let pair = by_new[&col];
                    old_line
                        .values
                        .push(value_at(session, Side::Old, old_row, pair.old));
                    new_line
                        .values
                        .push(value_at(session, Side::New, new_row, pair.new));
                    let changed = cells.contains_key(&(new_row, col));
                    old_line.changed.push(changed);
                    new_line.changed.push(changed);
                }
                lines.push(old_line);
                lines.push(new_line);
            }
            let total = lines.len();
            let items = lines
                .into_iter()
                .skip(page * page_size)
                .take(page_size)
                .collect();
            RowViewDto {
                kind: kind.to_owned(),
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
                kind: kind.to_owned(),
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
                kind: kind.to_owned(),
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
                kind: kind.to_owned(),
                columns,
                rows: None,
                groups: Some(dto::page(groups, total, page, page_size)),
            }
        }
        _ => panic!("unknown row view section {kind:?}"),
    }
}
