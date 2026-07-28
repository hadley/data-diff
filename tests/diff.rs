use data_diff::{
    CellCoordinate, ColumnEdit, Coordinate, Diff, DiffError, DiffOptions, EditSummary, FanoutEvent,
    HintKind, IssueKind, KeyBasis, KeyDiff, KeyOverlap, Side, diff_tables, write_human,
};
use test_support::table;

fn declared(key: &str) -> DiffOptions {
    DiffOptions {
        key: vec![key.to_owned()],
        hints: Vec::new(),
    }
}

fn hinted(key: &[&str], hints: &[&str]) -> DiffOptions {
    DiffOptions {
        key: key.iter().map(|name| (*name).to_owned()).collect(),
        hints: hints.iter().map(|hint| (*hint).to_owned()).collect(),
    }
}

fn render(diff: &Diff) -> Vec<u8> {
    let mut output = Vec::new();
    write_human(&mut output, diff).unwrap();
    output
}

#[test]
fn combines_schema_row_order_and_cell_changes() {
    let old = table! {
        "id" => [1, 2],
        "value" => [10, 20],
        "drop" => ["x", "y"],
    };
    let new = table! {
        "value" => [21, 11, 99],
        "id" => [2, 1, 3],
        "add" => ["a", "b", "c"],
    };

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
    let old = table! {
        "id" => [1, 2, 3],
        "a" => [0, 0, 0],
        "b" => [0, 0, 0],
        "c" => [0, 0, 0],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "a" => [1, 0, 0],
        "b" => [1, 0, 0],
        "c" => [0, 1, 1],
    };

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
fn selected_row_retains_its_moved_coordinate() {
    let old = table! {
        "id" => [1, 2],
        "a" => [10, 20],
        "b" => [30, 40],
    };
    let new = table! {
        "id" => [2, 1],
        "a" => [20, 11],
        "b" => [40, 31],
    };

    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert!(diff.summary.columns.is_empty());
    assert_eq!(diff.summary.rows, vec![Coordinate::from_zero_based(0, 1)]);
}

#[test]
fn default_options_guess_a_key_and_align_reordered_rows() {
    let old = table! {
        "customer_id" => [1, 2, 3],
        "value" => [10, 20, 30],
    };
    let new = table! {
        "value" => [30, 10, 21],
        "customer_id" => [3, 1, 2],
    };

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
fn automatic_resolution_without_an_eligible_key_is_an_error() {
    let empty = table! { "id" => i64[] };
    let rows = table! { "id" => [1, 2] };
    let disjoint = table! { "id" => [3, 4] };

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
fn a_bounded_fanout_keeps_its_cells_out_of_the_one_to_one_result() {
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        "value" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
        "value" => [0, 0, 0, 0, 7, 0, 0, 5, 0, 0, 0],
    };

    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert_eq!(
        diff.rows.fanout,
        vec![FanoutEvent {
            old: 4,
            new: vec![4, 5],
            cells: vec![CellCoordinate::from_zero_based(3, 1, 4, 1)],
        }]
    );
    // The fanned-out rows are not additions, drops, or matches, and they take
    // no part in ordering.
    assert!(diff.rows.added.is_empty());
    assert!(diff.rows.dropped.is_empty());
    assert_eq!(diff.rows.matched.len(), 9);
    assert!(diff.order.rows.is_empty());

    // The matched change in the same column still travels the ordinary
    // one-to-one path, so every changed cell remains reachable from exactly one
    // place. The summary is asserted whole: a leaked fanout cell would show up
    // here as a second event.
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(6, 1, 7, 1)]
    );
    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: false,
            values_changed: true,
        }]
    );
    assert_eq!(
        diff.summary,
        EditSummary {
            optimal: true,
            columns: vec![],
            rows: vec![Coordinate::from_zero_based(6, 7)],
        }
    );
}

#[test]
fn an_excessive_fanout_rejects_the_declared_key() {
    let old = table! { "id" => [1, 2] };
    let new = table! { "id" => [1, 1, 2] };

    assert_eq!(
        diff_tables(&old, &new, &declared("id")).unwrap_err(),
        DiffError::ExcessiveFanout {
            affected: 1,
            shared: 2,
        }
    );
}

#[test]
fn an_undeclared_rename_is_inferred_from_the_values() {
    let old = table! {
        "id" => [1, 2, 3],
        "amount" => i32[10, 20, 30],
        "note" => ["a", "b", "c"],
    };
    let new = table! {
        "note" => ["a", "B", "c"],
        "id" => [1, 2, 3],
        "total" => [10, 20, 30],
    };
    let options = declared("id");

    let diff = diff_tables(&old, &new, &options).unwrap();

    // Nothing declared the pair: "amount" and "total" are one column because
    // they agree in every matched row.
    assert!(diff.columns.added.is_empty());
    assert!(diff.columns.dropped.is_empty());
    assert_eq!(
        diff.columns.identities,
        vec![
            Coordinate::from_zero_based(0, 1),
            Coordinate::from_zero_based(1, 2),
            Coordinate::from_zero_based(2, 0),
        ]
    );

    // The rename changed type, which is an edit with no cells; the value
    // change belongs to the other column, because an exactly inferred rename
    // agrees everywhere by construction.
    assert_eq!(
        diff.columns.edited,
        vec![
            ColumnEdit {
                column: Coordinate::from_zero_based(1, 2),
                type_changed: true,
                values_changed: false,
            },
            ColumnEdit {
                column: Coordinate::from_zero_based(2, 0),
                type_changed: false,
                values_changed: true,
            },
        ]
    );
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(1, 2, 1, 0)]
    );
    assert_eq!(diff.order.columns, vec![Coordinate::from_zero_based(2, 0)]);

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_rename_is_inferred_despite_an_edit_it_carried() {
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "amount" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        "note" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "total" => [99, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        "note" => [99, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    };
    let options = declared("id");

    let diff = diff_tables(&old, &new, &options).unwrap();

    // Ten of eleven rows agree, which exact inference rejected outright; the
    // disagreement is now read as an edit the rename carried with it.
    assert!(diff.columns.added.is_empty());
    assert!(diff.columns.dropped.is_empty());
    assert_eq!(
        diff.columns.identities,
        vec![
            Coordinate::from_zero_based(0, 0),
            Coordinate::from_zero_based(1, 1),
            Coordinate::from_zero_based(2, 2),
        ]
    );

    // Unlike an exact rename, an approximate one has cells of its own, and
    // they are summarized beside every other change in the row they fall in.
    assert_eq!(
        diff.cells,
        vec![
            CellCoordinate::from_zero_based(0, 1, 0, 1),
            CellCoordinate::from_zero_based(0, 2, 0, 2),
        ]
    );
    assert_eq!(
        diff.summary,
        EditSummary {
            optimal: true,
            columns: vec![],
            rows: vec![Coordinate::from_zero_based(0, 0)],
        }
    );

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_swap_replaces_two_edits_with_two_renames() {
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "price" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        "cost" => [1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 10000, 11000],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "price" => [1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 10000, 11000],
        "cost" => [99, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
    };
    let options = declared("id");

    let diff = diff_tables(&old, &new, &options).unwrap();

    // Both columns changed in almost every row under their own names, and each
    // holds what the other used to, so the identities cross.
    assert_eq!(
        diff.columns.identities,
        vec![
            Coordinate::from_zero_based(0, 0),
            Coordinate::from_zero_based(1, 2),
            Coordinate::from_zero_based(2, 1),
        ]
    );
    assert!(diff.columns.added.is_empty());
    assert!(diff.columns.dropped.is_empty());

    // The move is not asserted separately: exchanging the ends of two
    // identities changes where each column sits, and ordering reads that off
    // the bijection like any other.
    assert_eq!(diff.order.columns, vec![Coordinate::from_zero_based(2, 1)]);

    // One crossing is imperfect, so the swap keeps the cell it could not
    // explain rather than claiming the columns match exactly.
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(0, 1, 0, 2)]
    );

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_renamed_key_identifies_rows_across_both_files() {
    let old = table! {
        "customer_id" => [1, 2, 3],
        "value" => [10, 20, 30],
    };
    let new = table! {
        "id" => [3, 1, 2],
        "value" => [30, 10, 21],
    };
    let options = DiffOptions {
        key: vec!["customer_id/id".into()],
        hints: Vec::new(),
    };

    let diff = diff_tables(&old, &new, &options).unwrap();

    // Without the pair these two columns would be a drop and an addition, and
    // no key could be resolved at all.
    assert_eq!(
        diff.key,
        KeyDiff {
            basis: KeyBasis::Declared,
            columns: vec![Coordinate::from_zero_based(0, 0)],
            overlap: None,
        }
    );
    assert!(diff.columns.added.is_empty());
    assert!(diff.columns.dropped.is_empty());

    // Identity, row matching, ordering, and cells all follow the pair rather
    // than the names.
    assert_eq!(
        diff.rows.matched,
        vec![
            Coordinate::from_zero_based(0, 1),
            Coordinate::from_zero_based(1, 2),
            Coordinate::from_zero_based(2, 0),
        ]
    );
    assert_eq!(diff.order.rows, vec![Coordinate::from_zero_based(2, 0)]);
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(1, 1, 2, 1)]
    );

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_guessed_key_may_fan_out() {
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        "status" => ["x", "x", "x", "x", "x", "x", "x", "x", "x", "x"],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
        "status" => ["x", "x", "x", "x", "y", "x", "x", "x", "x", "x", "x"],
    };

    let diff = diff_tables(&old, &new, &DiffOptions::default()).unwrap();

    // "status" repeats in `old`, so the only candidate is one that fans out.
    assert_eq!(
        diff.key,
        KeyDiff {
            basis: KeyBasis::Guessed,
            columns: vec![Coordinate::from_zero_based(0, 0)],
            overlap: Some(KeyOverlap {
                shared: 10,
                possible: 10,
            }),
        }
    );
    // Nothing downstream asks how the key was chosen, so a guessed fanout
    // produces the same self-contained event a declared one does.
    assert_eq!(
        diff.rows.fanout,
        vec![FanoutEvent {
            old: 4,
            new: vec![4, 5],
            cells: vec![CellCoordinate::from_zero_based(3, 1, 4, 1)],
        }]
    );
    assert!(diff.cells.is_empty());
    assert_eq!(diff.summary, EditSummary::default());

    let repeated = diff_tables(&old, &new, &DiffOptions::default()).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn repeated_fanout_comparisons_are_identical() {
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        "value" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
        "value" => [0, 0, 0, 0, 7, 0, 0, 5, 0, 0, 0],
    };
    let options = declared("id");

    let first = diff_tables(&old, &new, &options).unwrap();
    let second = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(first, second);
    assert_eq!(render(&first), render(&second));
}

#[test]
fn repeated_guessed_comparisons_are_identical() {
    let old = table! {
        "a" => [1, 2, 3],
        "b" => [7, 8, 9],
    };
    let new = table! {
        "a" => [2, 3, 4],
        "b" => [9, 8, 7],
    };
    let options = DiffOptions::default();

    let first = diff_tables(&old, &new, &options).unwrap();
    let second = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(first, second);
    assert_eq!(render(&first), render(&second));
}

#[test]
fn repeated_comparisons_are_identical() {
    let table = table! {
        "id" => [1, 2],
        "value" => [10, 20],
    };
    let options = declared("id");

    let first = diff_tables(&table, &table, &options).unwrap();
    let second = diff_tables(&table, &table, &options).unwrap();

    assert_eq!(first, second);
    assert_eq!(render(&first), render(&second));
}

#[test]
fn unmatched_rows_are_classified_without_cells_or_edits() {
    let empty = table! { "id" => i64[] };
    let rows = table! { "id" => [1, 2] };
    let disjoint = table! { "id" => [3, 4] };
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

#[test]
fn a_hint_identifies_a_column_inference_could_not_have() {
    let old = table! {
        "id" => [1, 2, 3],
        "discount" => [10, 20, 30],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "markdown" => [99, 98, 97],
    };
    let options = hinted(&["id"], &[r#"col_rename("discount" -> "markdown")"#]);

    let diff = diff_tables(&old, &new, &options).unwrap();

    // No value agrees, so no amount of inference would have paired these. The
    // hint is the only thing that knows.
    assert!(diff.columns.added.is_empty());
    assert!(diff.columns.dropped.is_empty());
    assert_eq!(
        diff.columns.identities,
        vec![
            Coordinate::from_zero_based(0, 0),
            Coordinate::from_zero_based(1, 1),
        ]
    );

    // A hint asserts identity, not equality: the values that changed are now
    // visible as an edit, which is what being unmatched was hiding.
    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: false,
            values_changed: true,
        }]
    );
    assert_eq!(diff.cells.len(), 3);
    assert!(diff.issues.is_empty());

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_hint_can_supply_a_renamed_key_column() {
    let old = table! {
        "customer_id" => [1, 2, 3],
        "value" => [10, 20, 30],
    };
    let new = table! {
        "id" => [3, 1, 2],
        "value" => [30, 10, 21],
    };
    let options = hinted(&["id"], &["col_rename(customer_id -> id)"]);

    let diff = diff_tables(&old, &new, &options).unwrap();

    // `--key id` names a column the old file does not have. The hint is applied
    // before the key is resolved, so the component finds its other endpoint
    // through the identity rather than failing.
    assert_eq!(
        diff.key,
        KeyDiff {
            basis: KeyBasis::Declared,
            columns: vec![Coordinate::from_zero_based(0, 0)],
            overlap: None,
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
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(1, 1, 2, 1)]
    );
}

#[test]
fn a_hint_the_key_contradicts_is_reported_and_dropped() {
    let old = table! {
        "id" => [1, 2],
        "gone" => [10, 20],
    };
    let new = table! {
        "code" => [1, 2],
        "fresh" => [10, 20],
    };
    let options = hinted(&["id/code"], &["col_rename(id -> fresh)"]);

    let diff = diff_tables(&old, &new, &options).unwrap();

    // The key pairs old "id" with new "code"; the hint wants old "id"
    // elsewhere. The key is what rows are identified by, so the hint gives way
    // rather than taking the key's endpoint with it.
    assert_eq!(diff.key.columns, vec![Coordinate::from_zero_based(0, 0)]);
    assert_eq!(diff.issues.len(), 1);
    assert_eq!(diff.issues[0].kind, IssueKind::ContradictoryHints);
    assert_eq!(diff.issues[0].hints[0].kind, HintKind::Rename);

    // "gone" and "fresh" agree in every row, so inference pairs them once the
    // hint is out of the way.
    assert_eq!(
        diff.columns.identities,
        vec![
            Coordinate::from_zero_based(0, 0),
            Coordinate::from_zero_based(1, 1),
        ]
    );
}

#[test]
fn a_missing_target_is_reported_without_failing_the_comparison() {
    let old = table! {
        "id" => [1, 2],
        "discount" => [10, 20],
    };
    let new = table! {
        "id" => [1, 2],
        "markdown" => [99, 98],
    };
    let options = hinted(&["id"], &["col_rename(discount -> mrkdown)"]);

    let diff = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(
        diff.issues[0].kind,
        IssueKind::HintMissingTarget {
            side: Side::New,
            column: "mrkdown".into(),
        }
    );
    // Reconciliation carried on without the instruction, reporting the columns
    // as what they look like absent one.
    assert_eq!(diff.columns.dropped, vec![2]);
    assert_eq!(diff.columns.added, vec![2]);
}

#[test]
fn a_hint_can_be_guessed_as_the_key() {
    let old = table! {
        "customer_id" => [1, 2, 3],
        "value" => [10, 20, 30],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "value" => [10, 25, 30],
    };
    let options = hinted(&[], &["col_rename(customer_id -> id)"]);

    let diff = diff_tables(&old, &new, &options).unwrap();

    // Key guessing considers identified columns, and a hint is what identified
    // this one. Without the hint the only guessable key is "value", which reads
    // the changed row as a drop and an addition.
    assert_eq!(
        diff.key,
        KeyDiff {
            basis: KeyBasis::Guessed,
            columns: vec![Coordinate::from_zero_based(0, 0)],
            overlap: Some(KeyOverlap {
                shared: 3,
                possible: 3,
            }),
        }
    );
    assert_eq!(diff.summary.rows, vec![Coordinate::from_zero_based(1, 1)]);
    assert!(diff.rows.added.is_empty());
    assert!(diff.rows.dropped.is_empty());
}

#[test]
fn a_rendered_rename_can_be_fed_back_as_a_hint() {
    let old = table! {
        "id" => [1, 2, 3],
        "amount" => [10, 20, 30],
        "note" => ["a", "b", "c"],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "total" => [10, 20, 30],
        "note" => ["a", "b", "c"],
    };

    // Inference finds this rename on its own, so the output carries a
    // col_rename() line.
    let inferred = diff_tables(&old, &new, &declared("id")).unwrap();
    let rendered = String::from_utf8(render(&inferred)).unwrap();
    let line = rendered
        .lines()
        .find(|line| line.starts_with("col_rename("))
        .expect("the rename is reported");
    assert_eq!(line, r#"col_rename("amount" -> "total")"#);

    // The claim of the format being an input language is that this line, taken
    // exactly as printed, is a hint asserting what it describes.
    let hinted = diff_tables(&old, &new, &hinted(&["id"], &[line])).unwrap();

    assert_eq!(hinted.columns.identities, inferred.columns.identities);
    assert!(hinted.issues.is_empty());
    assert_eq!(render(&hinted), render(&inferred));
}
