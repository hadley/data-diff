use data_diff::{
    CellCoordinate, ColumnEdit, ColumnIdentity, Coordinate, Diff, DiffOptions, EditSummary,
    FanoutEvent, HintKind, IdentityBasis, IssueKind, KeyBasis, KeyComponent, KeyDiff, KeyOverlap,
    KeyRejection, KeySubject, RejectionReason, RowEdit, Side, diff_tables, write_human,
};
use test_support::table;

/// One identity, spelled as its two zero-based positions and its basis.
fn identity(old: usize, new: usize, basis: IdentityBasis) -> ColumnIdentity {
    ColumnIdentity {
        column: Coordinate::from_zero_based(old, new),
        basis,
    }
}

/// One row edit, spelled as its two zero-based positions and its cell count.
fn row_edit(old: usize, new: usize, changes: usize) -> RowEdit {
    RowEdit {
        row: Coordinate::from_zero_based(old, new),
        changes,
    }
}

/// One key component naming a column that differs between the files.
fn paired(old: &str, new: &str) -> KeyComponent {
    KeyComponent {
        old: old.to_owned(),
        new: new.to_owned(),
    }
}

/// One key component naming the same column on both sides.
fn shared(name: &str) -> KeyComponent {
    KeyComponent {
        old: name.to_owned(),
        new: name.to_owned(),
    }
}

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
            identity(0, 1, IdentityBasis::Declared),
            identity(1, 0, IdentityBasis::Name),
        ]
    );
    assert_eq!(diff.columns.added, vec![3]);
    assert_eq!(diff.columns.dropped, vec![3]);
    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 0),
            type_changed: false,
            changes: 2,
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
                changes: 2,
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
                changes: 2,
            }],
            rows: vec![row_edit(0, 0, 2)],
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
    assert_eq!(diff.summary.rows, vec![row_edit(0, 1, 2)]);
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
            rejection: None,
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
fn automatic_resolution_without_an_eligible_key_falls_back() {
    let empty = table! { "id" => i64[] };
    let rows = table! { "id" => [1, 2] };
    let disjoint = table! { "id" => [3, 4] };

    // Nothing can identify a row, so each pair falls back to row position
    // rather than failing.
    for (old, new) in [(&empty, &rows), (&rows, &empty), (&rows, &disjoint)] {
        let diff = diff_tables(old, new, &DiffOptions::default()).unwrap();
        assert_eq!(diff.key.basis, KeyBasis::Fallback);
        assert!(diff.key.columns.is_empty());
        assert_eq!(diff.key.rejection, None);
    }
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
    // A fanout counts its own differing comparisons, and they stay its own: the
    // column edit beside it counts only the one-to-one cell.
    let rendered = String::from_utf8(render(&diff)).unwrap();
    assert!(
        rendered.contains("row_fanout(4 -> [4, 5], changes: 1)"),
        "{rendered}"
    );

    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: false,
            changes: 1,
        }]
    );
    assert_eq!(
        diff.summary,
        EditSummary {
            optimal: true,
            columns: vec![],
            rows: vec![row_edit(6, 7, 1)],
        }
    );
}

#[test]
fn an_excessive_fanout_rejects_the_declared_key() {
    let old = table! { "id" => [1, 2] };
    let new = table! { "id" => [1, 1, 2] };

    // The key is refused rather than fatal, and the comparison continues on
    // whatever can identify rows instead.
    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert_eq!(
        diff.key.rejection,
        Some(KeyRejection {
            subject: KeySubject::Key(vec![shared("id")]),
            reason: RejectionReason::ExcessiveFanout {
                affected: 1,
                shared: 2,
            },
        })
    );
    assert_ne!(diff.key.basis, KeyBasis::Declared);
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
            identity(0, 1, IdentityBasis::Declared),
            // Agreeing in every matched row is the strongest evidence there is,
            // and still evidence rather than an instruction, which is what the
            // basis is there to say.
            identity(1, 2, IdentityBasis::Exact),
            identity(2, 0, IdentityBasis::Name),
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
                changes: 0,
            },
            ColumnEdit {
                column: Coordinate::from_zero_based(2, 0),
                type_changed: false,
                changes: 1,
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
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 1, IdentityBasis::Approximate),
            identity(2, 2, IdentityBasis::Name),
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
            rows: vec![row_edit(0, 0, 2)],
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
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 2, IdentityBasis::Swapped),
            identity(2, 1, IdentityBasis::Swapped),
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
            rejection: None,
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
            rejection: None,
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
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 1, IdentityBasis::Hinted),
        ]
    );

    // A hint asserts identity, not equality: the values that changed are now
    // visible as an edit, which is what being unmatched was hiding.
    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: false,
            changes: 3,
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
            rejection: None,
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
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 1, IdentityBasis::Exact),
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
            rejection: None,
        }
    );
    assert_eq!(diff.summary.rows, vec![row_edit(1, 1, 1)]);
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
    assert_eq!(line, "col_rename(amount -> total, basis: exact)");

    // The claim of the format being an input language is that this line, taken
    // exactly as printed, is a hint asserting what it describes. The basis is
    // detail the line carries about the operation rather than part of what it
    // asserts, so the parser reads past it.
    let hinted = diff_tables(&old, &new, &hinted(&["id"], &[line])).unwrap();

    assert!(hinted.issues.is_empty());

    // What survives the round trip is the identity, and what does not is the
    // basis — which is the point rather than a shortfall. Supplying the line
    // does not make the values agree exactly; it makes the identity an
    // instruction, and the output says so.
    let columns = |diff: &Diff| {
        diff.columns
            .identities
            .iter()
            .map(|identity| identity.column.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(columns(&hinted), columns(&inferred));
    assert_eq!(
        hinted.columns.identities[1].basis,
        IdentityBasis::Hinted,
        "the basis of an asserted identity is the assertion"
    );
    assert_eq!(inferred.columns.identities[1].basis, IdentityBasis::Exact);

    // Nothing else about the comparison moves with it.
    assert_eq!(hinted.cells, inferred.cells);
    assert_eq!(hinted.summary, inferred.summary);
    assert_eq!(
        String::from_utf8(render(&hinted)).unwrap(),
        rendered.replace("basis: exact", "basis: hinted")
    );
}

#[test]
fn a_drop_and_an_add_choose_replacement_over_an_inferred_rename() {
    let old = table! {
        "id" => [1, 2, 3],
        "region" => ["north", "south", "east"],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "zone" => ["north", "south", "east"],
    };

    // The values agree everywhere, so inference identifies the two columns.
    let inferred = diff_tables(&old, &new, &declared("id")).unwrap();
    assert_eq!(inferred.columns.identities.len(), 2);
    assert!(inferred.columns.dropped.is_empty());

    // Only the user knows they are unrelated. Reserving both endpoints keeps
    // them out of inference, and neither hint needs the other to say so: each
    // alone would leave its opposite number with nothing to pair with.
    let replaced = diff_tables(
        &old,
        &new,
        &hinted(&["id"], &["col_drop(region)", "col_add(zone)"]),
    )
    .unwrap();

    assert!(replaced.issues.is_empty());
    assert_eq!(replaced.columns.dropped, [2]);
    assert_eq!(replaced.columns.added, [2]);
    // An unmatched column has no cells, so nothing about the values survives.
    assert!(replaced.cells.is_empty());
    assert!(replaced.summary.columns.is_empty());
}

#[test]
fn an_edit_hint_withdraws_a_swap() {
    let old = table! {
        "id" => [1, 2],
        "price" => [10, 20],
        "cost" => [30, 40],
    };
    let new = table! {
        "id" => [1, 2],
        "price" => [30, 40],
        "cost" => [10, 20],
    };

    // Each column holds what the other used to, which reads as an exchange.
    let inferred = diff_tables(&old, &new, &declared("id")).unwrap();
    assert_eq!(
        inferred.columns.identities,
        [
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 2, IdentityBasis::Swapped),
            identity(2, 1, IdentityBasis::Swapped),
        ]
    );

    // Naming one of the two columns is enough to withdraw it: an exchange takes
    // two, and this one has lost an end. Both columns then keep their own names
    // and report what actually happened to their values.
    let edited = diff_tables(&old, &new, &hinted(&["id"], &["col_edit(price)"])).unwrap();

    assert!(edited.issues.is_empty());
    assert_eq!(
        edited.columns.identities,
        [
            identity(0, 0, IdentityBasis::Declared),
            // The exchange withdrawn, both columns keep the identity their names
            // gave them, and the basis says as much.
            identity(1, 1, IdentityBasis::Name),
            identity(2, 2, IdentityBasis::Name),
        ]
    );
    assert_eq!(
        edited.summary.columns,
        [
            ColumnEdit {
                column: Coordinate::from_zero_based(1, 1),
                type_changed: false,
                changes: 2,
            },
            ColumnEdit {
                column: Coordinate::from_zero_based(2, 2),
                type_changed: false,
                changes: 2,
            },
        ]
    );
}

#[test]
fn an_edit_hint_summarizes_by_column_where_rows_would_have_won() {
    let old = table! {
        "id" => [1, 2],
        "a" => [10, 20],
        "b" => [30, 40],
    };
    let new = table! {
        "id" => [1, 2],
        "a" => [11, 21],
        "b" => [31, 40],
    };

    // Column "a" changes in both rows and "b" in the first, so covering the
    // two rows is the smaller description.
    let inferred = diff_tables(&old, &new, &declared("id")).unwrap();
    assert_eq!(
        inferred.summary.rows,
        [row_edit(0, 0, 2), row_edit(1, 1, 1)]
    );
    assert!(inferred.summary.columns.is_empty());

    // The hint says the change was to a column. Its cells leave the graph with
    // it, and what is left — one cell in "b" — is covered by the row it sits in.
    let edited = diff_tables(&old, &new, &hinted(&["id"], &["col_edit(a)"])).unwrap();

    assert!(edited.issues.is_empty());
    assert_eq!(
        edited.summary.columns,
        [ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: false,
            changes: 2,
        }]
    );
    // The surviving row edit counts the cell in the hinted column too: a hint
    // moves which events are reported, not what is true of row 1.
    assert_eq!(edited.summary.rows, [row_edit(0, 0, 2)]);
    // The hint changed how the same cells are described, not which cells there
    // are: the complete diff is untouched.
    assert_eq!(edited.cells, inferred.cells);
}

#[test]
fn an_edit_hint_on_an_unchanged_column_is_reported() {
    let table = table! {
        "id" => [1, 2],
        "value" => [10, 20],
    };

    let diff = diff_tables(&table, &table, &hinted(&["id"], &["col_edit(value)"])).unwrap();

    // A hint never manufactures a change. The identity is real and nothing
    // about it changed, so the instruction is dropped and the diff still says
    // the files are the same.
    assert_eq!(diff.issues.len(), 1);
    assert_eq!(diff.issues[0].kind, IssueKind::HintNoChange);
    assert_eq!(diff.issues[0].hints[0].kind, HintKind::Edit);
    assert!(diff.summary.columns.is_empty());
    assert!(diff.summary.rows.is_empty());
    assert!(diff.cells.is_empty());
}

#[test]
fn an_edit_hint_on_a_column_with_no_identity_is_reported() {
    let old = table! {
        "id" => [1, 2],
        "discount" => [10, 20],
    };
    let new = table! {
        "id" => [1, 2],
        "markdown" => [99, 98],
    };

    // Nothing connects these two columns, so "discount" is dropped and there is
    // no identity for the edit to attach to.
    let diff = diff_tables(&old, &new, &hinted(&["id"], &["col_edit(discount)"])).unwrap();

    assert_eq!(diff.issues.len(), 1);
    assert_eq!(diff.issues[0].kind, IssueKind::HintUnresolvedIdentity);
    assert_eq!(diff.columns.dropped, [2]);
    assert_eq!(diff.columns.added, [2]);
}

#[test]
fn a_rendered_edit_can_be_fed_back_as_a_hint() {
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "amount" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "total" => [10, 20, 30, 40, 50, 60, 71, 80, 90, 100, 110],
    };

    // "amount" became "total" and one of its eleven values changed with it,
    // which is close enough for inference to identify them. One changed cell
    // can be described by its row or by its column, and the summary picks the
    // row.
    let inferred = diff_tables(&old, &new, &declared("id")).unwrap();
    let rendered = String::from_utf8(render(&inferred)).unwrap();
    assert!(rendered.contains("col_rename(amount -> total, basis: approximate)"));
    assert!(rendered.contains("row_edit(7, changes: 1)"));

    // A col_edit() line names its column as the new file does, so feeding one
    // back means naming an identity by an end the old file does not have. It
    // still attaches, which is what makes the printed line an instruction
    // rather than merely a description.
    let edited = diff_tables(&old, &new, &hinted(&["id"], &["col_edit(total)"])).unwrap();

    assert!(edited.issues.is_empty());
    assert_eq!(
        edited.summary.columns,
        [ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: false,
            changes: 1,
        }]
    );
    assert!(edited.summary.rows.is_empty());

    // And the line the hint produces is itself a hint. `col_edit(total, changes: 1)`
    // carries a count the bare spelling does not, which the parser reads past
    // the way it reads past a rename's basis: the claim is the first argument,
    // and the rest is detail the format prints about the operation.
    let printed = String::from_utf8(render(&edited)).unwrap();
    let line = printed
        .lines()
        .find(|line| line.starts_with("col_edit("))
        .expect("the edit is reported");
    assert_eq!(line, "col_edit(total, changes: 1)");

    let again = diff_tables(&old, &new, &hinted(&["id"], &[line])).unwrap();
    assert!(again.issues.is_empty());
    assert_eq!(render(&again), render(&edited));
}

#[test]
fn issues_report_in_the_order_the_hints_were_supplied() {
    let old = table! {
        "id" => [1, 2],
        "value" => [10, 20],
    };
    let new = table! {
        "id" => [1, 2],
        "value" => [10, 20],
    };

    // These two fail on opposite sides of the comparison: a missing target is
    // settled before the key is resolved, and an unchanged identity cannot be
    // judged until the cells are compared. Neither ordering may leak that.
    let first = ["col_rename(value -> absent)", "col_edit(value)"];
    let second = ["col_edit(value)", "col_rename(value -> absent)"];

    let kinds = |hints: &[&str]| {
        diff_tables(&old, &new, &hinted(&["id"], hints))
            .unwrap()
            .issues
            .iter()
            .map(|issue| issue.kind.clone())
            .collect::<Vec<_>>()
    };

    assert_eq!(
        kinds(&first),
        [
            IssueKind::HintMissingTarget {
                side: Side::New,
                column: "absent".into(),
            },
            IssueKind::HintNoChange,
        ]
    );
    assert_eq!(
        kinds(&second),
        [
            IssueKind::HintNoChange,
            IssueKind::HintMissingTarget {
                side: Side::New,
                column: "absent".into(),
            },
        ]
    );
}

#[test]
fn a_reservation_frees_the_other_endpoint_for_another_candidate() {
    let old = table! {
        "id" => [1, 2, 3],
        "region" => ["north", "south", "east"],
        "spare" => ["north", "south", "east"],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "zone" => ["north", "south", "east"],
    };

    // Two old columns match the new one, and column order settles it.
    let inferred = diff_tables(&old, &new, &declared("id")).unwrap();
    assert_eq!(inferred.columns.identities.len(), 2);
    assert_eq!(inferred.columns.dropped, [3]);

    // Reserving "region" does not stop "zone" being identified: it says one
    // column has no partner, not that the other has none either. Where there is
    // only one candidate the two amount to the same thing, which is why a
    // replacement is spelled with both halves rather than left to that.
    let dropped = diff_tables(&old, &new, &hinted(&["id"], &["col_drop(region)"])).unwrap();

    assert!(dropped.issues.is_empty());
    assert_eq!(dropped.columns.dropped, [2]);
    assert!(dropped.columns.added.is_empty());

    // Saying so of both leaves nothing for it to pair with.
    let replaced = diff_tables(
        &old,
        &new,
        &hinted(&["id"], &["col_drop(region)", "col_drop(spare)"]),
    )
    .unwrap();

    assert_eq!(replaced.columns.dropped, [2, 3]);
    assert_eq!(replaced.columns.added, [2]);
}

#[test]
fn every_stage_still_runs_under_a_fallback_key() {
    // No column can identify a row: "tag" repeats and "amount" is rewritten
    // wholesale under a new name. Rows are paired by position instead.
    let old = table! {
        "tag" => ["x", "x", "x"],
        "amount" => [10, 20, 30],
        "note" => ["a", "a", "a"],
    };
    let new = table! {
        "tag" => ["x", "x", "x"],
        "total" => [10, 20, 30],
        "note" => ["a", "a", "z"],
    };

    let diff = diff_tables(&old, &new, &DiffOptions::default()).unwrap();

    assert_eq!(diff.key.basis, KeyBasis::Fallback);
    assert!(diff.key.columns.is_empty());
    // Rename inference reads agreement across the positionally matched rows and
    // is not disabled: no stage below key resolution branches on the basis.
    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Name),
            identity(1, 1, IdentityBasis::Exact),
            identity(2, 2, IdentityBasis::Name),
        ]
    );
    // Positional matches ascend on both sides, so there is never a row to move.
    assert!(diff.order.rows.is_empty());
    assert_eq!(diff.rows.matched.len(), 3);
    assert!(diff.rows.added.is_empty());
    assert!(diff.rows.dropped.is_empty());
    assert_eq!(diff.summary.rows, vec![row_edit(2, 2, 1)]);
}

#[test]
fn a_declared_positional_key_and_the_fallback_reach_one_key() {
    let old = table! { "tag" => ["x", "x"], "value" => [1, 1] };
    let new = table! { "tag" => ["x", "x"], "value" => [1, 2] };

    let fallen_back = diff_tables(&old, &new, &DiffOptions::default()).unwrap();
    let declared = diff_tables(
        &old,
        &new,
        &DiffOptions {
            key: vec![data_diff::POSITIONAL_COMPONENT.to_owned()],
            hints: Vec::new(),
        },
    )
    .unwrap();

    // Only how the key was arrived at differs; everything read off it agrees.
    assert_eq!(declared.key.basis, KeyBasis::Declared);
    assert_eq!(fallen_back.key.basis, KeyBasis::Fallback);
    assert_eq!(declared.rows, fallen_back.rows);
    assert_eq!(declared.cells, fallen_back.cells);
    assert_eq!(declared.summary, fallen_back.summary);
    assert_eq!(declared.columns, fallen_back.columns);
}

#[test]
fn a_rejected_pair_keeps_the_identity_it_asserted() {
    let old = table! { "customer_id" => [1, 1], "value" => [10, 10] };
    let new = table! { "id" => [1, 1], "value" => [10, 25] };

    // The pair asserts two things: that these columns are one, and that the
    // column identifies rows. Only the second fails.
    let diff = diff_tables(
        &old,
        &new,
        &DiffOptions {
            key: vec!["customer_id/id".to_owned()],
            hints: Vec::new(),
        },
    )
    .unwrap();

    assert_eq!(
        diff.key.rejection,
        Some(KeyRejection {
            subject: KeySubject::Key(vec![paired("customer_id", "id")]),
            reason: RejectionReason::NonUniqueOld {
                first_row: 1,
                row: 2,
            },
        })
    );
    assert!(
        diff.columns
            .identities
            .contains(&identity(0, 0, IdentityBasis::Declared))
    );
    assert_eq!(
        String::from_utf8(render(&diff)).unwrap(),
        "key_invalid([customer_id -> id], reason: non_unique_old)\n\
         ----\n\
         col_key([#row], basis: fallback)\n\
         col_rename(customer_id -> id, basis: declared)\n\
         row_edit(2, changes: 1)"
    );
}
