mod common;

use std::sync::Arc;

use arrow_array::{Int32Array, Int64Array, StringArray};
use data_diff::{
    CellCoordinate, ColumnEdit, Coordinate, Diff, DiffError, DiffOptions, EditSummary, KeyBasis,
    KeyDiff, KeyOverlap, diff_tables, write_human,
};

fn declared(key: &str) -> DiffOptions {
    DiffOptions {
        key: vec![key.to_owned()],
    }
}

fn render(diff: &Diff) -> Vec<u8> {
    let mut output = Vec::new();
    write_human(&mut output, diff).unwrap();
    output
}

#[test]
fn combines_schema_row_order_and_cell_changes() {
    let old = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("value", Arc::new(Int64Array::from(vec![10, 20]))),
        ("drop", Arc::new(StringArray::from(vec!["x", "y"]))),
    ]);
    let new = common::batch([
        ("value", Arc::new(Int64Array::from(vec![21, 11, 99]))),
        ("id", Arc::new(Int64Array::from(vec![2, 1, 3]))),
        ("add", Arc::new(StringArray::from(vec!["a", "b", "c"]))),
    ]);

    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert_eq!(
        diff.columns.identities,
        vec![
            Coordinate::from_zero_based(0, 1),
            Coordinate::from_zero_based(1, 0),
        ]
    );
    assert_eq!(diff.columns.added, vec![3]);
    assert_eq!(diff.columns.dropped, vec![3]);
    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 0),
            type_changed: false,
            values_changed: true,
        }]
    );
    assert_eq!(diff.rows.added, vec![3]);
    assert_eq!(
        diff.rows.matched,
        vec![
            Coordinate::from_zero_based(0, 1),
            Coordinate::from_zero_based(1, 0),
        ]
    );
    assert_eq!(diff.order.columns, vec![Coordinate::from_zero_based(1, 0)]);
    assert_eq!(diff.order.rows, vec![Coordinate::from_zero_based(1, 0)]);
    assert_eq!(
        diff.cells,
        vec![
            CellCoordinate::from_zero_based(0, 1, 1, 0),
            CellCoordinate::from_zero_based(1, 1, 0, 0),
        ]
    );
    assert_eq!(
        diff.summary,
        EditSummary {
            optimal: true,
            columns: vec![ColumnEdit {
                column: Coordinate::from_zero_based(1, 0),
                type_changed: false,
                values_changed: true,
            }],
            rows: vec![],
        }
    );
}

#[test]
fn summary_combines_row_and_column_edits_minimally() {
    let old = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2, 3]))),
        ("a", Arc::new(Int64Array::from(vec![0, 0, 0]))),
        ("b", Arc::new(Int64Array::from(vec![0, 0, 0]))),
        ("c", Arc::new(Int64Array::from(vec![0, 0, 0]))),
    ]);
    let new = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2, 3]))),
        ("a", Arc::new(Int64Array::from(vec![1, 0, 0]))),
        ("b", Arc::new(Int64Array::from(vec![1, 0, 0]))),
        ("c", Arc::new(Int64Array::from(vec![0, 1, 1]))),
    ]);

    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert_eq!(
        diff.summary,
        EditSummary {
            optimal: true,
            columns: vec![ColumnEdit {
                column: Coordinate::from_zero_based(3, 3),
                type_changed: false,
                values_changed: true,
            }],
            rows: vec![Coordinate::from_zero_based(0, 0)],
        }
    );
}

#[test]
fn summary_forces_and_coalesces_type_edits() {
    let old = common::batch([
        ("id", Arc::new(Int32Array::from(vec![1, 2]))),
        ("value", Arc::new(Int32Array::from(vec![10, 20]))),
    ]);
    let new = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("value", Arc::new(Int64Array::from(vec![11, 20]))),
    ]);

    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert_eq!(
        diff.summary,
        EditSummary {
            optimal: true,
            columns: vec![
                ColumnEdit {
                    column: Coordinate::from_zero_based(0, 0),
                    type_changed: true,
                    values_changed: false,
                },
                ColumnEdit {
                    column: Coordinate::from_zero_based(1, 1),
                    type_changed: true,
                    values_changed: true,
                },
            ],
            rows: vec![],
        }
    );
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(0, 1, 0, 1)]
    );
}

#[test]
fn selected_row_retains_its_moved_coordinate() {
    let old = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("a", Arc::new(Int64Array::from(vec![10, 20]))),
        ("b", Arc::new(Int64Array::from(vec![30, 40]))),
    ]);
    let new = common::batch([
        ("id", Arc::new(Int64Array::from(vec![2, 1]))),
        ("a", Arc::new(Int64Array::from(vec![20, 11]))),
        ("b", Arc::new(Int64Array::from(vec![40, 31]))),
    ]);

    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert!(diff.summary.columns.is_empty());
    assert_eq!(diff.summary.rows, vec![Coordinate::from_zero_based(0, 1)]);
}

#[test]
fn default_options_guess_a_key_and_align_reordered_rows() {
    let old = common::batch([
        ("customer_id", Arc::new(Int64Array::from(vec![1, 2, 3]))),
        ("value", Arc::new(Int64Array::from(vec![10, 20, 30]))),
    ]);
    let new = common::batch([
        ("value", Arc::new(Int64Array::from(vec![30, 10, 21]))),
        ("customer_id", Arc::new(Int64Array::from(vec![3, 1, 2]))),
    ]);

    let diff = diff_tables(&old, &new, &DiffOptions::default()).unwrap();

    assert_eq!(
        diff.key,
        KeyDiff {
            basis: KeyBasis::Guessed,
            columns: vec![Coordinate::from_zero_based(0, 1)],
            overlap: Some(KeyOverlap {
                shared: 3,
                possible: 3,
            }),
        }
    );
    assert_eq!(
        diff.rows.matched,
        vec![
            Coordinate::from_zero_based(0, 1),
            Coordinate::from_zero_based(1, 2),
            Coordinate::from_zero_based(2, 0),
        ]
    );
    assert!(diff.rows.added.is_empty());
    assert!(diff.rows.dropped.is_empty());
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(1, 1, 2, 0)]
    );
}

#[test]
fn a_guessed_key_stays_out_of_top_level_changed_cells() {
    let old = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("value", Arc::new(Int64Array::from(vec![10, 20]))),
    ]);
    let new = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("value", Arc::new(Int64Array::from(vec![10, 21]))),
    ]);

    let diff = diff_tables(&old, &new, &DiffOptions::default()).unwrap();

    assert_eq!(diff.key.basis, KeyBasis::Guessed);
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(1, 1, 1, 1)]
    );
}

#[test]
fn automatic_resolution_without_an_eligible_key_is_an_error() {
    let empty = common::batch([("id", Arc::new(Int64Array::from(Vec::<i64>::new())))]);
    let rows = common::batch([("id", Arc::new(Int64Array::from(vec![1, 2])))]);
    let disjoint = common::batch([("id", Arc::new(Int64Array::from(vec![3, 4])))]);

    assert_eq!(
        diff_tables(&empty, &rows, &DiffOptions::default()).unwrap_err(),
        DiffError::MissingKey
    );
    assert_eq!(
        diff_tables(&rows, &empty, &DiffOptions::default()).unwrap_err(),
        DiffError::MissingKey
    );
    assert_eq!(
        diff_tables(&rows, &disjoint, &DiffOptions::default()).unwrap_err(),
        DiffError::MissingKey
    );
}

#[test]
fn repeated_guessed_comparisons_are_identical() {
    let old = common::batch([
        ("a", Arc::new(Int64Array::from(vec![1, 2, 3]))),
        ("b", Arc::new(Int64Array::from(vec![7, 8, 9]))),
    ]);
    let new = common::batch([
        ("a", Arc::new(Int64Array::from(vec![2, 3, 4]))),
        ("b", Arc::new(Int64Array::from(vec![9, 8, 7]))),
    ]);
    let options = DiffOptions::default();

    let first = diff_tables(&old, &new, &options).unwrap();
    let second = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(first, second);
    assert_eq!(render(&first), render(&second));
}

#[test]
fn repeated_comparisons_are_identical() {
    let table = common::batch([
        ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ("value", Arc::new(Int64Array::from(vec![10, 20]))),
    ]);
    let options = declared("id");

    let first = diff_tables(&table, &table, &options).unwrap();
    let second = diff_tables(&table, &table, &options).unwrap();

    assert_eq!(first, second);
    assert_eq!(render(&first), render(&second));
}

#[test]
fn unmatched_rows_are_classified_without_cells_or_edits() {
    let empty = common::batch([("id", Arc::new(Int64Array::from(Vec::<i64>::new())))]);
    let rows = common::batch([("id", Arc::new(Int64Array::from(vec![1, 2])))]);
    let disjoint = common::batch([("id", Arc::new(Int64Array::from(vec![3, 4])))]);
    let options = declared("id");

    let added = diff_tables(&empty, &rows, &options).unwrap();
    let dropped = diff_tables(&rows, &empty, &options).unwrap();
    let both_empty = diff_tables(&empty, &empty, &options).unwrap();
    let unrelated = diff_tables(&rows, &disjoint, &options).unwrap();

    assert_eq!(added.rows.added, vec![1, 2]);
    assert_eq!(dropped.rows.dropped, vec![1, 2]);
    assert_eq!(both_empty.schemas.old[0].name, "id");
    assert!(both_empty.rows.matched.is_empty());

    // A pair that shares no key values reconciles like an empty side: every row
    // is atomic, so no comparison happens and nothing is summarized.
    assert_eq!(unrelated.rows.dropped, vec![1, 2]);
    assert_eq!(unrelated.rows.added, vec![1, 2]);
    assert!(unrelated.rows.matched.is_empty());

    for diff in [&added, &dropped, &both_empty, &unrelated] {
        assert!(diff.cells.is_empty());
        assert_eq!(diff.summary, EditSummary::default());
    }
}
