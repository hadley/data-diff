use std::path::Path;

use data_diff::{diff_tables, DiffOptions};
use data_diff_ui::commands;
use data_diff_ui::session::Session;
use test_support::table;

/// A session over in-memory tables; the paths are display strings here,
/// nothing reading them in the command layer.
fn session(old: arrow_array::RecordBatch, new: arrow_array::RecordBatch, key: &str) -> Session {
    let diff = diff_tables(
        &old,
        &new,
        &DiffOptions {
            key: vec![key.to_owned()],
            ..DiffOptions::default()
        },
    )
    .unwrap();
    Session::new(
        Path::new("old.parquet"),
        Path::new("new.parquet"),
        vec![key.to_owned()],
        Vec::new(),
        old,
        new,
        diff,
    )
}

/// One fixture exercising every panel at once: edits in two columns, a
/// rename, a drop, an add, an added row, a dropped row.
fn fixture() -> Session {
    let old = table! {
        "id" => [1, 2, 3, 4, 6],
        "price" => [9, 14, 20, 7, 1],
        "name" => ["a", "b", "c", "d", "x"],
        "qty" => [1, 2, 3, 4, 5],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5],
        "price" => [9, 16, 21, 8, 2],
        "label" => ["a", "b", "c", "d", "w"],
        "sku" => ["x", "y", "z", "w", "v"],
    };
    session(old, new, "id")
}

#[test]
fn schema_panel_marks_keys_renames_types_and_events() {
    let session = fixture();

    let changed = commands::schema_panel(&session, true);
    insta::assert_json_snapshot!(changed);

    let all = commands::schema_panel(&session, false);
    // The toggle fills in the unchanged identities: id joins the changed rows.
    assert!(all.len() > changed.len());
    assert!(all
        .iter()
        .any(|row| row.old_name.as_deref() == Some("id") && row.key));
}

#[test]
fn cells_page_paginates_in_key_order() {
    let session = fixture();

    let first = commands::cells_page(&session, "key", 0, 2);
    insta::assert_json_snapshot!(first);
    assert_eq!(first.items.len(), 2);
    assert!(first.total > 2);

    let rest = commands::cells_page(&session, "key", 1, 2);
    assert_eq!(first.page, 0);
    assert_eq!(rest.page, 1);
    assert_ne!(first.items, rest.items);

    // Column order groups by column, then row.
    let by_column = commands::cells_page(&session, "column", 0, 50);
    let columns: Vec<&str> = by_column
        .items
        .iter()
        .map(|item| item.column.as_str())
        .collect();
    let mut sorted = columns.clone();
    sorted.sort_unstable();
    assert_eq!(columns, sorted);
}

#[test]
fn column_view_aligns_edits_and_joins_context() {
    let session = fixture();

    let view = commands::column_view(&session, false, false, false, 0, 50);
    insta::assert_json_snapshot!(view);
    // Every edited column is a pair; changed rows only.
    assert!(view.columns.iter().all(|column| column.span == "pair"));
    assert!(view
        .rows
        .items
        .iter()
        .any(|row| row.cells.iter().any(|cell| cell.changed)));

    let everything = commands::column_view(&session, true, true, true, 0, 50);
    // Unchanged identities and the added/dropped columns join as singles —
    // except the key, which is already frozen at the left edge of every row.
    assert!(everything
        .columns
        .iter()
        .any(|column| column.span == "single"));
    assert!(everything.columns.iter().all(|column| column.name != "id"));
    assert!(everything.rows.total > view.rows.total);
}

#[test]
fn row_view_sections_read_their_rows() {
    let session = fixture();

    let edited = commands::row_view_section(&session, "edited", false, 0, 50);
    insta::assert_json_snapshot!(edited);
    let lines = edited.rows.unwrap().items;
    // Stacked old/new lines, two per changed row.
    assert!(lines.len().is_multiple_of(2));
    assert_eq!(lines[0].label, "old");
    assert_eq!(lines[1].label, "new");

    let added = commands::row_view_section(&session, "added", false, 0, 50);
    assert_eq!(added.rows.unwrap().total, 1);
    // The key is frozen at the left edge, not repeated among the values.
    assert!(added.columns.iter().all(|column| column != "id"));
    let dropped = commands::row_view_section(&session, "dropped", false, 0, 50);
    assert_eq!(dropped.rows.unwrap().total, 1);
    assert!(dropped.columns.iter().all(|column| column != "id"));

    let moved = commands::row_view_section(&session, "moved", false, 0, 50);
    assert_eq!(moved.rows.unwrap().total, 0);
}

#[test]
fn fanout_groups_expand_to_aligned_lines() {
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        "qty" => [0, 0, 0, 10, 0, 0, 0, 0, 0, 0],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
        "qty" => [0, 0, 0, 10, 4, 0, 0, 0, 0, 0, 0],
    };
    let session = session(old, new, "id");

    let view = commands::row_view_section(&session, "fanout", false, 0, 50);
    let groups = view.groups.unwrap();
    assert_eq!(groups.total, 1);
    let group = &groups.items[0];
    insta::assert_json_snapshot!(group);
    // The old row on top, each new row below, the changed cell flagged.
    assert_eq!(group.lines.len(), 3);
    assert!(group.lines[2].changed.iter().any(|&changed| changed));
}

#[test]
fn repeated_commands_are_byte_identical() {
    let session = fixture();
    let run = || {
        serde_json::to_string(&commands::session_summary(&session)).unwrap()
            + &serde_json::to_string(&commands::cells_page(&session, "key", 0, 10)).unwrap()
            + &serde_json::to_string(&commands::column_view(&session, true, true, true, 0, 10))
                .unwrap()
            + &serde_json::to_string(&commands::row_view_section(&session, "edited", true, 0, 10))
                .unwrap()
    };
    assert_eq!(run(), run());
}
