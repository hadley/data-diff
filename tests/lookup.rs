use data_diff::{DiffOptions, Lookup, Side, Value, diff_tables};
use test_support::table;

fn declared(key: &str) -> DiffOptions {
    DiffOptions {
        key: vec![key.to_owned()],
        ..DiffOptions::default()
    }
}

/// The integration the API exists for: a diff's changed-cell coordinates fed
/// back through the lookup must name cells whose values actually differ, and
/// the matched cells outside the set must agree. The diff retains no values,
/// so this is the check that coordinates and values tell one story.
#[test]
fn changed_cells_lookup_to_values_that_differ_and_the_rest_agree() {
    let old = table! {
        "id" => [1, 2, 3, 4],
        "price" => [9, 14, 20, 7],
        "name" => ["a", "b", "c", "d"],
    };
    let new = table! {
        "id" => [1, 2, 3, 4],
        "price" => [9, 16, 20, 8],
        "name" => ["a", "b", "c", "d"],
    };
    let diff = diff_tables(&old, &new, &declared("id")).unwrap();
    let lookup = Lookup::new(&old, &new);

    assert_eq!(diff.cells.len(), 2);
    for cell in &diff.cells {
        let (old_at, new_at) = cell.positions();
        let before = lookup.value(Side::Old, old_at[0], old_at[1]).unwrap();
        let after = lookup.value(Side::New, new_at[0], new_at[1]).unwrap();
        assert_ne!(before, after, "changed cell {old_at:?} -> {new_at:?}");
    }

    // Every matched cell outside the changed set agrees: the same-type
    // columns here make typed equality the right strength.
    let changed = diff
        .cells
        .iter()
        .map(|cell| cell.positions().0)
        .collect::<std::collections::BTreeSet<_>>();
    for row in 1..=4_u32 {
        for column in 1..=3_u32 {
            if changed.contains(&[row, column]) {
                continue;
            }
            assert_eq!(
                lookup.value(Side::Old, row, column).unwrap(),
                lookup.value(Side::New, row, column).unwrap(),
                "unchanged cell ({row}, {column})"
            );
        }
    }
}

/// Added and dropped rows are only positions in the model; the row view's
/// expansions read their values through the same lookup.
#[test]
fn added_and_dropped_row_positions_read_their_rows() {
    let old = table! {
        "id" => [1, 2],
        "price" => [9, 14],
    };
    let new = table! {
        "id" => [1, 3],
        "price" => [9, 20],
    };
    let diff = diff_tables(&old, &new, &declared("id")).unwrap();
    let lookup = Lookup::new(&old, &new);

    assert_eq!(diff.rows.dropped, [2]);
    assert_eq!(diff.rows.added, [2]);
    assert_eq!(
        lookup.row(Side::Old, diff.rows.dropped[0] as u32).unwrap(),
        [Value::Int64(2), Value::Int64(14)]
    );
    assert_eq!(
        lookup.row(Side::New, diff.rows.added[0] as u32).unwrap(),
        [Value::Int64(3), Value::Int64(20)]
    );
}

/// A fanout's cells live outside the top-level cell set; their coordinates
/// resolve through the lookup like any other's.
#[test]
fn fanout_cells_resolve_against_both_sides() {
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        "qty" => [0, 0, 0, 10, 0, 0, 0, 0, 0, 0],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
        "qty" => [0, 0, 0, 10, 4, 0, 0, 0, 0, 0, 0],
    };
    let diff = diff_tables(&old, &new, &declared("id")).unwrap();
    let lookup = Lookup::new(&old, &new);

    assert_eq!(diff.rows.fanout.len(), 1);
    let fanout = &diff.rows.fanout[0];
    assert_eq!(fanout.old, 4);
    assert_eq!(fanout.new, [4, 5]);
    assert_eq!(fanout.cells.len(), 1);
    let (old_at, new_at) = fanout.cells[0].positions();
    assert_eq!(
        lookup.value(Side::Old, old_at[0], old_at[1]).unwrap(),
        Value::Int64(10)
    );
    assert_eq!(
        lookup.value(Side::New, new_at[0], new_at[1]).unwrap(),
        Value::Int64(4)
    );
}

/// A type-changed column reads each side in its own source type through the
/// lookup, the honesty the cell view's rendering rests on.
#[test]
fn a_type_changed_column_reads_in_each_sides_source_type() {
    let old = table! {
        "id" => [1, 2],
        "price" => ["9.99", "14.50"],
    };
    let new = table! {
        "id" => [1, 2],
        "price" => [9.99, 16.0],
    };
    let diff = diff_tables(&old, &new, &declared("id")).unwrap();
    let lookup = Lookup::new(&old, &new);

    // "9.99" parses to the double 9.99 exactly, so only row 2 changed.
    assert_eq!(diff.cells.len(), 1);
    let (old_at, new_at) = diff.cells[0].positions();
    assert_eq!(
        lookup.value(Side::Old, old_at[0], old_at[1]).unwrap(),
        Value::String("14.50".into())
    );
    assert_eq!(
        lookup.value(Side::New, new_at[0], new_at[1]).unwrap(),
        Value::Double(16.0)
    );
}

/// The whole path — diff, then a page of lookups — repeated, byte for byte.
#[test]
fn repeated_runs_are_identical() {
    let run = || {
        let old = table! {
            "id" => [1, 2, 3],
            "price" => [9, 14, 20],
        };
        let new = table! {
            "id" => [1, 2, 3],
            "price" => [9, 16, 20],
        };
        let diff = diff_tables(&old, &new, &declared("id")).unwrap();
        let lookup = Lookup::new(&old, &new);
        diff.cells
            .iter()
            .map(|cell| {
                let (old_at, new_at) = cell.positions();
                (
                    lookup.value(Side::Old, old_at[0], old_at[1]).unwrap(),
                    lookup.value(Side::New, new_at[0], new_at[1]).unwrap(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}
