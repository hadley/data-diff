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

    let first = commands::cells_page(&session, "key", false, 0, 2);
    insta::assert_json_snapshot!(first);
    assert_eq!(first.items.len(), 2);
    assert!(first.total > 2);

    let rest = commands::cells_page(&session, "key", false, 1, 2);
    assert_eq!(first.page, 0);
    assert_eq!(rest.page, 1);
    assert_ne!(first.items, rest.items);

    // Column order groups by column, then row.
    let by_column = commands::cells_page(&session, "column", false, 0, 50);
    let columns: Vec<&str> = by_column
        .items
        .iter()
        .map(|item| item.column.as_str())
        .collect();
    let mut sorted = columns.clone();
    sorted.sort_unstable();
    assert_eq!(columns, sorted);
}

/// The fixture has one added row (id 5) and one dropped row (id 6). With
/// the flag off the cell view is exactly the changed cells; with it on,
/// each such row contributes one line per non-key column on its own side,
/// the other side absent.
#[test]
fn cells_page_joins_added_and_dropped_rows_on_request() {
    let session = fixture();

    let changed_only = commands::cells_page(&session, "key", false, 0, 50);
    let joined = commands::cells_page(&session, "key", true, 0, 50);

    // Three non-key columns a side: price/label/sku for the added row,
    // price/name/qty for the dropped one.
    assert_eq!(joined.total, changed_only.total + 6);

    let added: Vec<_> = joined
        .items
        .iter()
        .filter(|item| item.old.is_none())
        .collect();
    assert_eq!(added.len(), 3);
    assert!(added
        .iter()
        .all(|item| item.key[0].text == "5" && item.new.is_some() && item.delta.is_none()));

    let dropped: Vec<_> = joined
        .items
        .iter()
        .filter(|item| item.new.is_none())
        .collect();
    assert_eq!(dropped.len(), 3);
    assert!(dropped
        .iter()
        .all(|item| item.key[0].text == "6" && item.old.is_some() && item.delta.is_none()));

    // The changed lines keep both sides and interleave in key order: the
    // added row's lines follow id 4's edits, the dropped row's close out.
    let keys: Vec<&str> = joined
        .items
        .iter()
        .map(|item| item.key[0].text.as_str())
        .collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted);

    // Column sort groups the one-sided lines under their column names too.
    let by_column = commands::cells_page(&session, "column", true, 0, 50);
    let columns: Vec<&str> = by_column
        .items
        .iter()
        .map(|item| item.column.as_str())
        .collect();
    let mut sorted = columns.clone();
    sorted.sort_unstable();
    assert_eq!(columns, sorted);

    // Repeated runs are byte-identical.
    assert_eq!(
        serde_json::to_string(&joined).unwrap(),
        serde_json::to_string(&commands::cells_page(&session, "key", true, 0, 50)).unwrap()
    );
}

#[test]
fn cells_sort_keys_numerically_not_textually() {
    // Twenty rows each with one edit; the key sort must read the keys as
    // numbers — 1, 2, …, 20 — not as text, which would give 1, 10, 11, ….
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20],
        "v" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20],
        "v" => [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    };
    let session = session(old, new, "id");

    let page = commands::cells_page(&session, "key", false, 0, 50);
    let keys: Vec<&str> = page
        .items
        .iter()
        .map(|item| item.key[0].text.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
            "17", "18", "19", "20"
        ]
    );
}

#[test]
fn column_view_aligns_edits_and_joins_context() {
    let session = fixture();

    let view = commands::column_view(&session, false, false, false, None, 0, 50);
    insta::assert_json_snapshot!(view);
    // Every edited column is a pair; changed rows only.
    assert!(view.columns.iter().all(|column| column.span == "pair"));
    assert!(view
        .rows
        .items
        .iter()
        .any(|row| row.cells.iter().any(|cell| cell.changed)));

    let everything = commands::column_view(&session, true, true, true, None, 0, 50);
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
fn edited_columns_group_by_changed_row_set() {
    // "a" and "b" change in exactly the same rows, "c" in a row of its own,
    // and hints force the column description the optimizer would not choose
    // for so small a rectangle.
    let old = table! {
        "id" => [1, 2, 3, 4, 5],
        "a" => [10, 20, 30, 40, 50],
        "b" => [60, 70, 80, 90, 100],
        "c" => [110, 120, 130, 140, 150],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5],
        "a" => [11, 22, 30, 40, 50],
        "b" => [61, 72, 80, 90, 100],
        "c" => [110, 120, 131, 140, 150],
    };
    let hints = vec![
        "col_edit(a)".to_owned(),
        "col_edit(b)".to_owned(),
        "col_edit(c)".to_owned(),
    ];
    let diff = diff_tables(
        &old,
        &new,
        &DiffOptions {
            key: vec!["id".to_owned()],
            hints: hints.clone(),
            ..DiffOptions::default()
        },
    )
    .unwrap();
    let session = Session::new(
        Path::new("old.parquet"),
        Path::new("new.parquet"),
        vec!["id".to_owned()],
        hints,
        old,
        new,
        diff,
    );

    // One multi-column sub-entry, titled by the members and counted in the
    // shared rows; "c" is a singleton and stays with the parent entry.
    let groups = commands::edited_column_groups(&session).groups;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].columns, ["a", "b"]);
    assert_eq!(groups[0].rows, 2);

    // The parent entry shows every edited column; the sub-entry narrows to
    // the group's columns and rows.
    let all = commands::column_view(&session, false, false, false, None, 0, 50);
    assert_eq!(
        all.columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    assert_eq!(all.rows.total, 3);
    let one = commands::column_view(&session, false, false, false, Some(0), 0, 50);
    assert_eq!(
        one.columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(one.rows.total, 2);

    // An unknown group index is empty rather than an error.
    let none = commands::column_view(&session, false, false, false, Some(9), 0, 50);
    assert!(none.columns.is_empty());
    assert_eq!(none.rows.total, 0);
}

#[test]
fn row_view_sections_read_their_rows() {
    let session = fixture();

    let edited = commands::row_view_section(&session, "edited", false, None, None, 0, 50);
    insta::assert_json_snapshot!(edited);
    // The fixture's changed cells are all in `price`, which the summary
    // covers as a column edit — so the row view's edited section is empty,
    // each event shown in exactly one place.
    assert_eq!(edited.rows.unwrap().total, 0);

    let added = commands::row_view_section(&session, "added", false, None, None, 0, 50);
    assert_eq!(added.rows.unwrap().total, 1);
    // The key is frozen at the left edge, not repeated among the values.
    assert!(added.columns.iter().all(|column| column != "id"));
    let dropped = commands::row_view_section(&session, "dropped", false, None, None, 0, 50);
    assert_eq!(dropped.rows.unwrap().total, 1);
    assert!(dropped.columns.iter().all(|column| column != "id"));

    let moved = commands::row_view_section(&session, "moved", false, None, None, 0, 50);
    assert_eq!(moved.rows.unwrap().total, 0);
}

#[test]
fn edited_rows_group_by_changed_column_set() {
    // Twenty rows, so that naming a changed row is cheap and the cover
    // describes the rectangle by its rows rather than its columns.
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20],
        "a" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200],
        "b" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200],
        "c" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20],
        "a" => [11, 21, 31, 40, 50, 61, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200],
        "b" => [11, 21, 31, 40, 50, 61, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200],
        "c" => [10, 20, 30, 40, 51, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200],
    };
    let session = session(old, new, "id");

    // A rectangle over "a" and "b", a singleton changed in "c" interrupting
    // it, then one more rectangle row: two sidebar sub-entries, one per
    // distinct column set.
    let groups = commands::edited_groups(&session).groups;
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].columns, ["a", "b"]);
    assert_eq!(groups[0].rows, 4);
    assert_eq!(groups[1].columns, ["c"]);
    assert_eq!(groups[1].rows, 1);

    // The parent entry: every edited row as old/new line pairs over the
    // section's changed columns, paginated by line.
    let all = commands::row_view_section(&session, "edited", false, None, None, 0, 50);
    assert_eq!(all.columns, ["a", "b", "c"]);
    let lines = all.rows.unwrap();
    assert_eq!(lines.total, 10);
    assert_eq!(lines.items.len(), 10);
    assert_eq!(lines.items[0].label, "old");
    assert_eq!(lines.items[1].label, "new");
    let page = commands::row_view_section(&session, "edited", false, None, None, 1, 4);
    let page = page.rows.unwrap();
    assert_eq!(page.total, 10);
    assert_eq!(page.items.len(), 4);

    // A sub-entry: one group's rows, the column set narrowing to the
    // group's changed columns.
    let one = commands::row_view_section(&session, "edited", false, Some(1), None, 0, 50);
    assert_eq!(one.columns, ["c"]);
    let lines = one.rows.unwrap();
    assert_eq!(lines.total, 2);
    assert_eq!(lines.items[0].key[0].text, "5");

    // The all-columns toggle fills in every identity instead.
    let wide = commands::row_view_section(&session, "edited", true, Some(1), None, 0, 50);
    assert_eq!(wide.columns, ["id", "a", "b", "c"]);

    // The side toggle: one line per edited row, on the requested side only.
    let old_only = commands::row_view_section(
        &session,
        "edited",
        false,
        None,
        Some(data_diff::Side::Old),
        0,
        50,
    );
    let lines = old_only.rows.unwrap();
    assert_eq!(lines.total, 5);
    assert!(lines.items.iter().all(|line| line.label == "old"));
    assert_eq!(lines.items[0].values[0].text, "10");
    // The hidden side rides along, so a changed cell's tooltip can say
    // old → new without a second fetch.
    assert_eq!(lines.items[0].alt.as_ref().unwrap()[0].text, "11");
    let new_only = commands::row_view_section(
        &session,
        "edited",
        false,
        None,
        Some(data_diff::Side::New),
        0,
        50,
    );
    let lines = new_only.rows.unwrap();
    assert_eq!(lines.total, 5);
    assert!(lines.items.iter().all(|line| line.label == "new"));
    assert_eq!(lines.items[0].values[0].text, "11");
    assert_eq!(lines.items[0].alt.as_ref().unwrap()[0].text, "10");
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

    let view = commands::row_view_section(&session, "fanout", false, None, None, 0, 50);
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
            + &serde_json::to_string(&commands::cells_page(&session, "key", false, 0, 10)).unwrap()
            + &serde_json::to_string(&commands::column_view(
                &session, true, true, true, None, 0, 10,
            ))
            .unwrap()
            + &serde_json::to_string(&commands::row_view_section(
                &session, "edited", true, None, None, 0, 10,
            ))
            .unwrap()
            + &serde_json::to_string(&commands::edited_groups(&session)).unwrap()
            + &serde_json::to_string(&commands::edited_column_groups(&session)).unwrap()
    };
    assert_eq!(run(), run());
}
