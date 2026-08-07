use data_diff::{
    Budgets, CellCoordinate, ChangeMass, ColumnEdit, ColumnIdentity, ColumnSchema, Coordinate,
    Diff, DiffError, DiffOptions, EditSummary, FanoutEvent, HintKind, IdentityBasis,
    IncompleteStage, IssueKind, KeyBasis, KeyComponent, KeyDiff, KeyOverlap, KeyRejection,
    KeyRetraction, KeySubject, NormalizedType, OneSidedDiff, RejectionReason, RowBudget, RowEdit,
    Side, diff_added, diff_removed, diff_tables, write_human, write_human_one_sided,
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
        ..DiffOptions::default()
    }
}

fn hinted(key: &[&str], hints: &[&str]) -> DiffOptions {
    DiffOptions {
        key: key.iter().map(|name| (*name).to_owned()).collect(),
        hints: hints.iter().map(|hint| (*hint).to_owned()).collect(),
        ..DiffOptions::default()
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
            retraction: None,
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
fn a_faithful_boolean_reencoding_is_a_type_change_and_nothing_more() {
    let old = table! {
        "id" => [1, 2, 3],
        "paid" => [true, false, true],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "paid" => [1, 0, 1],
    };
    let options = declared("id");

    // Once fatal as incompatible. The 0/1 encoding compares equal value by
    // value, so the retype reads like any other faithful conversion: a type
    // change with no cells behind it.
    let diff = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 1, IdentityBasis::Name),
        ]
    );
    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: true,
            changes: 0,
        }]
    );
    assert!(diff.cells.is_empty());

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn an_unfaithful_boolean_reencoding_reports_its_changed_cells() {
    let old = table! {
        "id" => [1, 2, 3],
        "paid" => [true, false, true],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "paid" => [1, 1, 1],
    };
    let options = declared("id");

    let diff = diff_tables(&old, &new, &options).unwrap();

    // Row 2 genuinely flipped — false is 0, not any nonzero — so the retype
    // carries a measured value change beside it.
    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: true,
            changes: 1,
        }]
    );
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(1, 1, 1, 1)]
    );

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_cross_type_exchange_is_a_swap_rather_than_two_impossible_retypes() {
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "flag" => [
            true, false, true, false, true, false, true, false, true, false, true
        ],
        "count" => [1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 10000, 11000],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "flag" => [1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 10000, 11000],
        "count" => [
            true, false, true, false, true, false, true, false, true, false, true
        ],
    };
    let options = declared("id");

    // Each same-name pair once failed the comparison outright; now each is an
    // ordinary identity whose values disagree everywhere, and the crossings
    // agree perfectly on identical source types, which is the exchange's own
    // bar. The apparent retypes dissolve: each final identity keeps its type.
    let diff = diff_tables(&old, &new, &options).unwrap();

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
    assert!(diff.columns.edited.is_empty());
    assert!(diff.cells.is_empty());
    assert_eq!(diff.order.columns, vec![Coordinate::from_zero_based(2, 1)]);

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_boolean_column_relates_to_its_integer_encoding_by_value() {
    let old = table! {
        "id" => [1, 2],
        "flag" => [true, false],
    };
    let new = table! {
        "id" => [1, 2],
        "count" => [1, 0],
    };
    let options = declared("id");

    let diff = diff_tables(&old, &new, &options).unwrap();

    // A drop and an addition until rename inference measures them: the 0/1
    // encoding is exact agreement, so the identity forms across the types and
    // the retype rides on it.
    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 1, IdentityBasis::Exact),
        ]
    );
    assert!(diff.columns.added.is_empty());
    assert!(diff.columns.dropped.is_empty());
    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: true,
            changes: 0,
        }]
    );

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_boolean_and_a_numeric_pair_may_be_declared_as_the_key() {
    let old = table! {
        "flag" => [true, false],
        "value" => [10, 20],
    };
    let new = table! {
        "count" => [0, 1],
        "value" => [21, 10],
    };
    let options = DiffOptions {
        key: vec!["flag/count".into()],
        ..DiffOptions::default()
    };

    let diff = diff_tables(&old, &new, &options).unwrap();

    // The declared pair validates on the encoding and identifies both rows
    // across the reversal.
    assert_eq!(diff.key.basis, KeyBasis::Declared);
    assert_eq!(diff.key.rejection, None);
    assert_eq!(
        diff.rows.matched,
        vec![
            Coordinate::from_zero_based(0, 1),
            Coordinate::from_zero_based(1, 0),
        ]
    );
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(1, 1, 0, 1)]
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
        ..DiffOptions::default()
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
            retraction: None,
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
            retraction: None,
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
            retraction: None,
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
            retraction: None,
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
    // No column can identify a row: "tag" and "amount" both repeat a value, so
    // even once the rename is found the pair cannot be reconsidered into a
    // key, and rows stay paired by position.
    let old = table! {
        "tag" => ["x", "x", "x"],
        "amount" => [10, 10, 30],
        "note" => ["a", "a", "a"],
    };
    let new = table! {
        "tag" => ["x", "x", "x"],
        "total" => [10, 10, 30],
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
            ..DiffOptions::default()
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
            ..DiffOptions::default()
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
         table_key([:row], basis: fallback)\n\
         col_rename(customer_id -> id, basis: declared)\n\
         row_edit(2, changes: 1)"
    );
}

#[test]
fn a_renamed_key_is_recovered_on_reconsideration() {
    // Guessing pairs candidates by name, so the renamed key is invisible to it
    // and the first pass settles on "amount". Inference then identifies the
    // rename, and reconsidering the key with that identity in hand finds the
    // better candidate: three shared values against two.
    let old = table! {
        "customer_id" => [1, 2, 3],
        "amount" => [10, 20, 30],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "amount" => [10, 20, 35],
    };

    let diff = diff_tables(&old, &new, &DiffOptions::default()).unwrap();

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
            retraction: None,
        }
    );
    // The identity the key rests on keeps saying how it was found.
    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Exact),
            identity(1, 1, IdentityBasis::Name),
        ]
    );
    // Under the first pass's key the changed row was a drop and an add; under
    // the reconsidered key it is what it was all along.
    assert_eq!(diff.rows.matched.len(), 3);
    assert!(diff.rows.added.is_empty());
    assert!(diff.rows.dropped.is_empty());
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(2, 1, 2, 1)]
    );

    let repeated = diff_tables(&old, &new, &DiffOptions::default()).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn an_implausible_guess_is_retracted_for_the_next_candidate() {
    // "a" wins the first guess by column order, but matching by it reverses
    // the rows and makes every other cell disagree: sixteen of twenty-four
    // cell masses changed, past the limit. The guess is retracted, excluded,
    // and the chain lands on "b", whose diff says only that "a" itself
    // changed.
    let old = table! {
        "a" => [1, 2, 3, 4],
        "b" => [10, 20, 30, 40],
        "x" => [5, 6, 7, 8],
    };
    let new = table! {
        "a" => [4, 3, 2, 1],
        "b" => [10, 20, 30, 40],
        "x" => [5, 6, 7, 8],
    };

    let diff = diff_tables(&old, &new, &DiffOptions::default()).unwrap();

    assert_eq!(
        diff.key.retraction,
        Some(KeyRetraction {
            columns: vec![shared("a")],
            mass: ChangeMass {
                changed: 16,
                total: 24,
            },
        })
    );
    assert_eq!(diff.key.basis, KeyBasis::Guessed);
    assert_eq!(diff.key.columns, vec![Coordinate::from_zero_based(1, 1)]);
    assert_eq!(diff.regeneration, None);
    assert_eq!(diff.cells.len(), 4);
    assert_eq!(
        String::from_utf8(render(&diff)).unwrap(),
        "key_retracted([a], reason: excessive_change)\n\
         ----\n\
         table_key([b], basis: guessed, overlap: 1.00)\n\
         col_edit(a, changes: 4)"
    );

    let repeated = diff_tables(&old, &new, &DiffOptions::default()).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn an_implausible_guess_with_no_successor_is_retracted_for_the_fallback() {
    let old = table! {
        "a" => [1, 2, 3, 4],
        "x" => [1, 1, 2, 2],
        "y" => [3, 3, 4, 4],
    };
    let new = table! {
        "a" => [4, 3, 2, 1],
        "x" => [1, 1, 2, 2],
        "y" => [3, 3, 4, 4],
    };

    let diff = diff_tables(&old, &new, &DiffOptions::default()).unwrap();

    // "x" and "y" repeat values, so once "a" is retracted nothing else can
    // identify a row and the chain ends at row position — where the diff is a
    // plausible story again: "a" changed, everything else agrees.
    assert_eq!(
        diff.key.retraction,
        Some(KeyRetraction {
            columns: vec![shared("a")],
            mass: ChangeMass {
                changed: 16,
                total: 24,
            },
        })
    );
    assert_eq!(diff.key.basis, KeyBasis::Fallback);
    assert!(diff.key.columns.is_empty());
    assert_eq!(diff.regeneration, None);
}

#[test]
fn an_implausible_fallback_regenerates_without_a_second_pass() {
    let old = table! {
        "tag" => ["p", "p"],
        "v" => [1, 2],
    };
    let new = table! {
        "tag" => ["q", "q"],
        "v" => [3, 4],
    };

    let diff = diff_tables(&old, &new, &DiffOptions::default()).unwrap();

    // There is nothing below the fallback to retract it to, and inference
    // offered no candidate, so the implausible diff goes straight to
    // regeneration: no retraction, and the model still holds every cell.
    assert_eq!(diff.key.basis, KeyBasis::Fallback);
    assert_eq!(diff.key.retraction, None);
    assert_eq!(
        diff.regeneration,
        Some(ChangeMass {
            changed: 8,
            total: 8,
        })
    );
    assert_eq!(diff.cells.len(), 4);
    assert_eq!(diff.summary.rows.len() + diff.summary.columns.len(), 2);
    assert_eq!(
        String::from_utf8(render(&diff)).unwrap(),
        "table_key([:row], basis: fallback)\ntable_regenerate()"
    );

    let repeated = diff_tables(&old, &new, &DiffOptions::default()).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_declared_key_is_neither_reconsidered_nor_regenerated_over() {
    // Every non-key cell changed, which under a chosen key would be past the
    // limit. The user vouched for this matching, so the edits are real and
    // the row story is kept in full.
    let old = table! {
        "id" => [1, 2, 3],
        "v" => [1, 2, 3],
        "w" => [4, 5, 6],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "v" => [7, 8, 9],
        "w" => [10, 11, 12],
    };

    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert_eq!(diff.key.basis, KeyBasis::Declared);
    assert_eq!(diff.key.retraction, None);
    assert_eq!(diff.regeneration, None);
    assert_eq!(diff.summary.columns.len(), 2);
    assert_eq!(diff.cells.len(), 6);
}

#[test]
fn a_rejection_and_a_retraction_stay_visible_together() {
    let old = table! {
        "a" => [1, 2, 3, 4],
        "x" => [1, 1, 2, 2],
        "y" => [3, 3, 4, 4],
    };
    let new = table! {
        "a" => [4, 3, 2, 1],
        "x" => [1, 1, 2, 2],
        "y" => [3, 3, 4, 4],
    };

    let diff = diff_tables(&old, &new, &declared("absent")).unwrap();

    // The whole chain is on the key: a declaration the data lacks a column
    // for, a guess withdrawn by its own diff, and the fallback that remains.
    assert_eq!(
        diff.key.rejection,
        Some(KeyRejection {
            subject: KeySubject::Component(shared("absent")),
            reason: RejectionReason::MissingColumn { side: Side::Old },
        })
    );
    assert_eq!(
        diff.key.retraction,
        Some(KeyRetraction {
            columns: vec![shared("a")],
            mass: ChangeMass {
                changed: 16,
                total: 24,
            },
        })
    );
    assert_eq!(diff.key.basis, KeyBasis::Fallback);
}

#[test]
fn the_key_is_reconsidered_at_most_once() {
    // Both candidates produce an implausible diff: "a" reverses the rows and
    // "b" pairs neighbours, and either way every cell of the other two
    // columns disagrees. The first guess is retracted; the second stands,
    // because a second pass is never itself reconsidered, and its implausible
    // diff is reported as a regeneration instead.
    let old = table! {
        "a" => [1, 2, 3, 4],
        "b" => [10, 20, 30, 40],
        "x" => [5, 6, 7, 8],
    };
    let new = table! {
        "a" => [4, 3, 2, 1],
        "b" => [20, 10, 40, 30],
        "x" => [9, 10, 11, 12],
    };

    let diff = diff_tables(&old, &new, &DiffOptions::default()).unwrap();

    assert_eq!(
        diff.key.retraction,
        Some(KeyRetraction {
            columns: vec![shared("a")],
            mass: ChangeMass {
                changed: 16,
                total: 24,
            },
        })
    );
    assert_eq!(diff.key.basis, KeyBasis::Guessed);
    assert_eq!(diff.key.columns, vec![Coordinate::from_zero_based(1, 1)]);
    assert_eq!(
        diff.regeneration,
        Some(ChangeMass {
            changed: 16,
            total: 24,
        })
    );

    let repeated = diff_tables(&old, &new, &DiffOptions::default()).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn pass_one_identities_are_rederived_rather_than_carried() {
    // Positionally, "g" and "f" agree everywhere — an exact rename — and the
    // key columns agree in twenty of twenty-two rows, an approximate one. The
    // key is reconsidered onto that pair, and under the keyed matching the
    // two moved rows separate "g" from "f": pass two re-derives the identity
    // as approximate, which it could not say if pass one's exact finding had
    // been carried across.
    let old = table! {
        "cid" => [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
            12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
        ],
        "g" => [
            100, 200, 300, 400, 500, 600, 700, 800, 900, 1000, 1100,
            1200, 1300, 1400, 1500, 1600, 1700, 1800, 1900, 2000, 2100, 2200,
        ],
    };
    let new = table! {
        "id" => [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
            12, 13, 14, 15, 16, 17, 18, 19, 20, 22, 21,
        ],
        "f" => [
            100, 200, 300, 400, 500, 600, 700, 800, 900, 1000, 1100,
            1200, 1300, 1400, 1500, 1600, 1700, 1800, 1900, 2000, 2100, 2200,
        ],
    };

    let diff = diff_tables(&old, &new, &DiffOptions::default()).unwrap();

    assert_eq!(diff.key.basis, KeyBasis::Guessed);
    assert_eq!(diff.key.columns, vec![Coordinate::from_zero_based(0, 0)]);
    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Approximate),
            identity(1, 1, IdentityBasis::Approximate),
        ]
    );
    // All twenty-two rows match under the reconsidered key; the two moved
    // rows carry the only changes.
    assert_eq!(diff.rows.matched.len(), 22);
    assert_eq!(diff.cells.len(), 2);
}

#[test]
fn adopting_a_swapped_key_pair_carries_its_companion() {
    // "a" and "b" exchanged their contents, so neither shares a value with
    // its own name and the first pass falls back — where swap inference sees
    // the exchange. Reconsideration then adopts one half of it as the key,
    // and the companion identity comes along: a swapped identity never
    // appears without its exchange.
    let old = table! {
        "a" => [1, 2, 3],
        "b" => [10, 20, 30],
        "t" => ["x", "x", "x"],
    };
    let new = table! {
        "a" => [10, 20, 30],
        "b" => [1, 2, 3],
        "t" => ["x", "x", "x"],
    };

    let diff = diff_tables(&old, &new, &DiffOptions::default()).unwrap();

    assert_eq!(diff.key.basis, KeyBasis::Guessed);
    assert_eq!(diff.key.columns, vec![Coordinate::from_zero_based(0, 1)]);
    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 1, IdentityBasis::Swapped),
            identity(1, 0, IdentityBasis::Swapped),
            identity(2, 2, IdentityBasis::Name),
        ]
    );
    assert!(diff.rows.added.is_empty());
    assert!(diff.rows.dropped.is_empty());
    assert!(diff.cells.is_empty());

    let repeated = diff_tables(&old, &new, &DiffOptions::default()).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_one_sided_diff_describes_the_file_that_exists() {
    let table = table! {
        "id" => [1, 2, 3],
        "label" => ["a", "b", "c"],
    };

    let added = diff_added(&table).unwrap();
    let removed = diff_removed(&table).unwrap();

    // The schema arrives as validation describes it, the rows as a count; no
    // key, identities, or cells exist to be held.
    assert_eq!(
        added,
        OneSidedDiff {
            side: Side::New,
            columns: vec![
                ColumnSchema {
                    name: "id".into(),
                    source_type: "Int64".into(),
                    normalized_type: NormalizedType::Int64,
                },
                ColumnSchema {
                    name: "label".into(),
                    source_type: "Utf8".into(),
                    normalized_type: NormalizedType::String,
                },
            ],
            rows: 3,
        }
    );
    assert_eq!(removed.side, Side::Old);
    assert_eq!(removed.columns, added.columns);
    assert_eq!(removed.rows, 3);

    let render = |diff: &OneSidedDiff| {
        let mut output = Vec::new();
        write_human_one_sided(&mut output, diff).unwrap();
        output
    };
    let repeated = diff_added(&table).unwrap();
    assert_eq!(added, repeated);
    assert_eq!(render(&added), render(&repeated));
}

#[test]
fn a_one_sided_diff_validates_like_either_side_of_a_two_sided_one() {
    let duplicated = table! {
        "a" => [1],
        "a" => [2],
    };
    assert!(matches!(
        diff_added(&duplicated),
        Err(DiffError::DuplicateColumnNames {
            side: Side::New,
            ..
        })
    ));
    assert!(matches!(
        diff_removed(&duplicated),
        Err(DiffError::DuplicateColumnNames {
            side: Side::Old,
            ..
        })
    ));

    // Binary was the unsupported example here until opaque columns admitted
    // it; a fixed-size list is one of the few types the row encoding still
    // refuses.
    let field = std::sync::Arc::new(arrow_schema::Field::new(
        "item",
        arrow_schema::DataType::Int64,
        true,
    ));
    let nested = arrow_array::FixedSizeListArray::new(
        field,
        2,
        std::sync::Arc::new(arrow_array::Int64Array::from(vec![1, 2])),
        None,
    );
    let unsupported = test_support::table_from_columns(vec![(
        "nested",
        std::sync::Arc::new(nested) as arrow_array::ArrayRef,
    )]);
    assert!(matches!(
        diff_added(&unsupported),
        Err(DiffError::UnsupportedColumn {
            side: Side::New,
            ..
        })
    ));
}

#[test]
fn an_opaque_column_is_edited_like_any_other() {
    let old = table! {
        "id" => [1, 2, 3],
        "when" => date32[100, 200, 300],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "when" => date32[100, 250, 300],
    };
    let options = DiffOptions::default();

    // A date column was fatal before opaque columns; now it is an ordinary
    // identity whose values compare exactly, under a key guessed beside it.
    let diff = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(diff.key.basis, KeyBasis::Guessed);
    assert_eq!(diff.key.columns, vec![Coordinate::from_zero_based(0, 0)]);
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(1, 1, 1, 1)]
    );
    assert_eq!(diff.summary.rows, vec![row_edit(1, 1, 1)]);

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn an_opaque_column_can_be_the_key() {
    let old = table! {
        "when" => date32[100, 200, 300],
        "value" => [1, 1, 2],
    };
    let new = table! {
        "when" => date32[300, 100, 200],
        "value" => [2, 1, 1],
    };

    // Declared: the pair validates on its canonical bytes.
    let declared = diff_tables(&old, &new, &declared("when")).unwrap();
    assert_eq!(declared.key.basis, KeyBasis::Declared);
    assert_eq!(
        declared.rows.matched,
        vec![
            Coordinate::from_zero_based(0, 1),
            Coordinate::from_zero_based(1, 2),
            Coordinate::from_zero_based(2, 0),
        ]
    );
    assert!(declared.cells.is_empty());

    // Guessed: "value" repeats in old, so the date column is the one
    // candidate that can identify rows, and it wins on ordinary evidence.
    let guessed = diff_tables(&old, &new, &DiffOptions::default()).unwrap();
    assert_eq!(guessed.key.basis, KeyBasis::Guessed);
    assert_eq!(guessed.key.columns, vec![Coordinate::from_zero_based(0, 0)]);

    let repeated = diff_tables(&old, &new, &DiffOptions::default()).unwrap();
    assert_eq!(guessed, repeated);
    assert_eq!(render(&guessed), render(&repeated));
}

#[test]
fn a_renamed_opaque_column_is_recovered_on_exact_evidence() {
    let old = table! {
        "id" => [1, 2],
        "stamp" => ts_ms[1000, 2000],
    };
    let new = table! {
        "id" => [1, 2],
        "logged" => ts_ms[1000, 2000],
    };

    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 1, IdentityBasis::Exact),
        ]
    );
    assert!(diff.columns.added.is_empty());
    assert!(diff.columns.dropped.is_empty());

    let repeated = diff_tables(&old, &new, &declared("id")).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn two_same_named_opaque_columns_can_swap() {
    let old = table! {
        "id" => [1, 2, 3],
        "start" => ts_ms[1000, 2000, 3000],
        "end" => ts_ms[9000, 8000, 7000],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "start" => ts_ms[9000, 8000, 7000],
        "end" => ts_ms[1000, 2000, 3000],
    };

    // Identical source types on the crossings, which is the exchange's own
    // bar, met here by construction.
    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 2, IdentityBasis::Swapped),
            identity(2, 1, IdentityBasis::Swapped),
        ]
    );
    assert!(diff.cells.is_empty());

    let repeated = diff_tables(&old, &new, &declared("id")).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn an_incomparable_same_name_pair_is_a_type_change_with_no_value_story() {
    let old = table! {
        "id" => [1, 2],
        "when" => [100, 200],
    };
    let new = table! {
        "id" => [1, 2],
        "when" => date32[100, 200],
    };
    let options = declared("id");

    // The same name still makes an identity; what incomparability removes is
    // the value story. The values are never compared — not claimed changed and
    // not claimed equal — so the type change is the column's whole report.
    let diff = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 1, IdentityBasis::Name),
        ]
    );
    assert!(diff.columns.dropped.is_empty());
    assert!(diff.columns.added.is_empty());
    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: true,
            changes: 0,
        }]
    );
    assert!(diff.cells.is_empty());

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_timezone_is_part_of_the_type() {
    let old = table! {
        "id" => [1, 2],
        "at" => ts_ms[1000, 2000],
    };
    let new = table! {
        "id" => [1, 2],
        "at" => ts_ms_naive[1000, 2000],
    };

    // An instant and a wall-clock reading are different claims, and no rule
    // relates them without assuming the writer's timezone, so awareness is
    // never crossed: the pair is one column whose type changed, with no value
    // story either way — the one refusal promotion deliberately kept.
    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: true,
            changes: 0,
        }]
    );
    assert!(diff.cells.is_empty());
}

#[test]
fn an_exchange_that_swapped_types_is_recovered_through_its_crossings() {
    let old = table! {
        "id" => [1, 2, 3],
        "when" => date32[100, 200, 300],
        "count" => [7, 8, 9],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "when" => [7, 8, 9],
        "count" => date32[100, 200, 300],
    };

    // Neither same-name pair can be measured, which reads as rewritten
    // vacuously; the crossings are identical-typed and agree exactly, so the
    // exchange is recovered and each final identity keeps its own type.
    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 2, IdentityBasis::Swapped),
            identity(2, 1, IdentityBasis::Swapped),
        ]
    );
    assert!(diff.columns.edited.is_empty());
    assert!(diff.cells.is_empty());

    let repeated = diff_tables(&old, &new, &declared("id")).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_declared_key_across_an_incomparable_pair_is_rejected_and_the_diff_continues() {
    let old = table! {
        "id" => [1, 2],
        "when" => date32[100, 200],
    };
    let new = table! {
        "id" => [1, 2],
        "when" => [100, 200],
    };
    let options = declared("when");

    let diff = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(
        diff.key.rejection,
        Some(KeyRejection {
            subject: KeySubject::Component(shared("when")),
            reason: RejectionReason::IncompatibleTypes {
                old_type: "Date32".into(),
                new_type: "Int64".into(),
            },
        })
    );
    // A key needs comparable values, so the declaration is rejected; the
    // identity it asserted needs none, so it survives, its type change its
    // whole story. The comparison continues on the guessed key beside it.
    assert_eq!(diff.key.basis, KeyBasis::Guessed);
    assert_eq!(diff.key.columns, vec![Coordinate::from_zero_based(0, 0)]);
    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Name),
            identity(1, 1, IdentityBasis::Declared),
        ]
    );
    assert!(diff.columns.dropped.is_empty());
    assert!(diff.columns.added.is_empty());

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_rename_hint_across_an_incomparable_pair_is_honoured() {
    let old = table! {
        "id" => [1, 2],
        "when" => date32[100, 200],
    };
    let new = table! {
        "id" => [1, 2],
        "stamp" => [100, 200],
    };
    let options = hinted(&["id"], &["col_rename(when -> stamp)"]);

    // The assertion is about identity, which does not require comparability;
    // the rename prints beside a type-only edit, and no value is ever claimed
    // changed or unchanged.
    let diff = diff_tables(&old, &new, &options).unwrap();

    assert!(diff.issues.is_empty());
    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 1, IdentityBasis::Hinted),
        ]
    );
    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: true,
            changes: 0,
        }]
    );
    assert!(diff.cells.is_empty());
}

#[test]
fn a_one_sided_diff_admits_opaque_columns() {
    let table = table! {
        "id" => [1, 2],
        "at" => ts_ms[1000, 2000],
    };

    let added = diff_added(&table).unwrap();

    assert_eq!(added.columns[1].normalized_type, NormalizedType::Timestamp);
    assert_eq!(
        added.columns[1].source_type,
        "Timestamp(Millisecond, Some(\"UTC\"))"
    );
    assert_eq!(added.rows, 2);
}

#[test]
fn a_cross_unit_timestamp_column_can_be_the_declared_or_guessed_key() {
    let old = table! {
        "at" => ts_ms[1000, 2000, 3000],
        "value" => [1, 1, 2],
    };
    let new = table! {
        "at" => ts_us[3_000_000, 1_000_000, 2_000_000],
        "value" => [2, 1, 1],
    };

    // A unit retype rejected this declaration as `incompatible_types` before
    // promotion; the pair now compares as instants and validates as a key.
    let declared = diff_tables(&old, &new, &declared("at")).unwrap();
    assert_eq!(declared.key.basis, KeyBasis::Declared);
    assert!(declared.key.rejection.is_none());
    assert_eq!(
        declared.rows.matched,
        vec![
            Coordinate::from_zero_based(0, 1),
            Coordinate::from_zero_based(1, 2),
            Coordinate::from_zero_based(2, 0),
        ]
    );
    assert!(declared.cells.is_empty());

    // "value" repeats in old, so the timestamp column is the one candidate,
    // and it wins on ordinary evidence.
    let guessed = diff_tables(&old, &new, &DiffOptions::default()).unwrap();
    assert_eq!(guessed.key.basis, KeyBasis::Guessed);
    assert_eq!(guessed.key.columns, vec![Coordinate::from_zero_based(0, 0)]);

    let repeated = diff_tables(&old, &new, &DiffOptions::default()).unwrap();
    assert_eq!(guessed, repeated);
    assert_eq!(render(&guessed), render(&repeated));
}

#[test]
fn a_cross_unit_retype_compares_values_and_reports_the_type_change() {
    let old = table! {
        "id" => [1, 2, 3],
        "at" => ts_ms[1000, 2000, 3000],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "at" => ts_us[1_000_000, 2_500_000, 3_000_000],
    };
    let options = declared("id");

    // The type change and the value change are independent facts, and
    // promotion makes both visible at once: the equal instants are equal
    // across the unit change, and the edited one is a changed cell.
    let diff = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: true,
            changes: 1,
        }]
    );
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(1, 1, 1, 1)]
    );

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_dropped_timestamp_is_recovered_as_a_rename_across_units() {
    let old = table! {
        "id" => [1, 2],
        "stamp" => ts_ms[1000, 2000],
    };
    let new = table! {
        "id" => [1, 2],
        "logged" => ts_us[1_000_000, 2_000_000],
    };

    // Exact inference hashes the pair under its plan, and the instants are
    // equal across the unit change, so the rename is recovered on the same
    // evidence as any other.
    let diff = diff_tables(&old, &new, &declared("id")).unwrap();

    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 1, IdentityBasis::Exact),
        ]
    );
    assert!(diff.columns.added.is_empty());
    assert!(diff.columns.dropped.is_empty());

    let repeated = diff_tables(&old, &new, &declared("id")).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_decimal_column_meets_the_integers_it_replaced() {
    let old = table! {
        "id" => [1, 2, 3],
        "price" => [500, 600, 700],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "price" => dec[50_000, 60_000, 70_100],
    };
    let options = declared("id");

    // 500 is exactly 500.00, so only the genuinely edited price is a changed
    // cell, beside the visible retype.
    let diff = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: true,
            changes: 1,
        }]
    );
    assert_eq!(
        diff.cells,
        vec![CellCoordinate::from_zero_based(2, 1, 2, 1)]
    );

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn an_iso_date_string_column_retyped_to_dates_is_a_type_only_edit() {
    let old = table! {
        "id" => [1, 2, 3],
        "day" => ["2026-08-01", "2026-08-02", "2026-08-03"],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "day" => date32[20666, 20667, 20668],
    };
    let options = declared("id");

    // Each string parses under the date profile to the day beside it, so the
    // values all compare equal and the retype is the whole report.
    let diff = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: true,
            changes: 0,
        }]
    );
    assert!(diff.cells.is_empty());

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn an_instant_string_column_matches_an_aware_timestamp() {
    let old = table! {
        "id" => [1, 2],
        "at" => ["1970-01-01T00:00:01Z", "1970-01-01T01:00:00+01:00"],
    };
    let new = table! {
        "id" => [1, 2],
        "at" => ts_ms[1000, 0],
    };
    let options = declared("id");

    // Both spellings carry offsets, so both name instants, and each equals
    // the stored epoch offset beside it after normalization to UTC.
    let diff = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(
        diff.columns.edited,
        vec![ColumnEdit {
            column: Coordinate::from_zero_based(1, 1),
            type_changed: true,
            changes: 0,
        }]
    );
    assert!(diff.cells.is_empty());

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn a_declared_key_across_the_awareness_divide_is_still_rejected() {
    let old = table! {
        "id" => [1, 2],
        "at" => ts_ms[1000, 2000],
    };
    let new = table! {
        "id" => [1, 2],
        "at" => ts_ms_naive[1000, 2000],
    };
    let options = declared("at");

    // Promotion widened the matrix, not the machinery around it: a pair with
    // no plan still rejects a declared key and keeps its asserted identity.
    let diff = diff_tables(&old, &new, &options).unwrap();

    assert_eq!(
        diff.key.rejection,
        Some(KeyRejection {
            subject: KeySubject::Component(shared("at")),
            reason: RejectionReason::IncompatibleTypes {
                old_type: "Timestamp(Millisecond, Some(\"UTC\"))".into(),
                new_type: "Timestamp(Millisecond, None)".into(),
            },
        })
    );
    assert_eq!(diff.key.basis, KeyBasis::Guessed);
    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Name),
            identity(1, 1, IdentityBasis::Declared),
        ]
    );

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn tiny_budgets_produce_valid_partial_results_and_report_them() {
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        "gone" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120],
        "p" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        "q" => [-1, -2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12],
        "edited" => [5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        "fresh" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 999],
        "p" => [-1, -2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12],
        "q" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        "edited" => [6, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5],
    };

    // Under the defaults nothing binds: the rename-and-modify pair is one
    // column, the exchange is a swap, and the summary is exactly minimal.
    let options = declared("id");
    let free = diff_tables(&old, &new, &options).unwrap();
    assert!(free.incomplete.is_empty());
    assert!(free.summary.optimal);
    assert!(free.columns.dropped.is_empty());
    assert!(free.columns.added.is_empty());
    assert!(
        free.columns
            .identities
            .contains(&identity(1, 1, IdentityBasis::Approximate))
    );
    assert!(
        free.columns
            .identities
            .contains(&identity(2, 3, IdentityBasis::Swapped))
    );

    // Zeroed budgets exhaust every bounded stage. Each returns its stated
    // valid partial result — the candidates stay a drop and an addition, the
    // exchange stays two same-name identities, the summary still covers every
    // cell — and the diff names all three, in the fixed order.
    let bounded = DiffOptions {
        budgets: Budgets {
            rename_rows: RowBudget::Rows(0),
            swap_rows: RowBudget::Rows(0),
            summary_cells: 0,
            ..Budgets::default()
        },
        ..declared("id")
    };
    let diff = diff_tables(&old, &new, &bounded).unwrap();

    assert_eq!(
        diff.incomplete,
        [
            IncompleteStage::Renames,
            IncompleteStage::Swaps,
            IncompleteStage::Summary,
        ]
    );
    assert_eq!(diff.columns.dropped, [2]);
    assert_eq!(diff.columns.added, [2]);
    assert!(!diff.summary.optimal);
    assert!(diff.summary.rows.is_empty());
    assert_eq!(
        diff.summary.columns,
        vec![
            ColumnEdit {
                column: Coordinate::from_zero_based(2, 2),
                type_changed: false,
                changes: 12,
            },
            ColumnEdit {
                column: Coordinate::from_zero_based(3, 3),
                type_changed: false,
                changes: 12,
            },
            ColumnEdit {
                column: Coordinate::from_zero_based(4, 4),
                type_changed: false,
                changes: 1,
            },
        ]
    );

    // The report leads the problems block, before the separator, and repeats
    // byte for byte: a bounded run is as deterministic as an unbounded one.
    let rendered = String::from_utf8(render(&diff)).unwrap();
    assert!(
        rendered
            .starts_with("incomplete_renames()\nincomplete_swaps()\nincomplete_summary()\n----\n")
    );
    let repeated = diff_tables(&old, &new, &bounded).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn reconsiderations_second_pass_runs_under_fresh_counters() {
    let old = table! {
        "customer_id" => [1, 2, 3],
        "value" => [10, 20, 30],
        "r_old" => ["a", "b", "c"],
    };
    let new = table! {
        "id" => [1, 2, 3],
        "value" => [10, 20, 31],
        "r_new" => ["a", "b", "c"],
    };
    // Pass one guesses "value", and its rename inference spends four
    // examinations of the 3 matched rows on the diagonal: two claiming the
    // key pair, two claiming the other rename. Reconsideration then adopts
    // the inferred key, and the second pass must re-derive (r_old, r_new)
    // over the wider matching, spending two more. Twelve rows cover each
    // pass alone and not both together, so this passes only if every pass
    // runs under its own counters.
    let options = DiffOptions {
        budgets: Budgets {
            rename_rows: RowBudget::Rows(12),
            ..Budgets::default()
        },
        ..DiffOptions::default()
    };

    let diff = diff_tables(&old, &new, &options).unwrap();

    assert!(diff.incomplete.is_empty());
    assert_eq!(diff.key.basis, KeyBasis::Guessed);
    assert_eq!(diff.key.columns, vec![Coordinate::from_zero_based(0, 0)]);
    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Exact),
            identity(1, 1, IdentityBasis::Name),
            identity(2, 2, IdentityBasis::Exact),
        ]
    );

    // A first pass allowed three examinations' worth strands (r_old, r_new)
    // — but the diff reports the pass it kept, and the second pass re-derives
    // the pair well within its own fresh budget, so nothing is incomplete in
    // the end.
    let tighter = DiffOptions {
        budgets: Budgets {
            rename_rows: RowBudget::Rows(9),
            ..Budgets::default()
        },
        ..DiffOptions::default()
    };
    let diff = diff_tables(&old, &new, &tighter).unwrap();

    assert!(diff.incomplete.is_empty());
    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Exact),
            identity(1, 1, IdentityBasis::Name),
            identity(2, 2, IdentityBasis::Exact),
        ]
    );
}

#[test]
fn a_proportional_rename_budget_binds_by_the_tables_own_size() {
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        "a" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120],
        "b" => [-1, -2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        "x" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120],
        "y" => [-1, -2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12],
    };

    // Both columns renamed in place. The diagonal spends one verification and
    // one informativeness measurement per pair, 12 rows each; the table is 36
    // cells, so one row per cell funds the first pair's 24 and dies inside the
    // second — which strands (b, y) while the finished claim stands.
    let bound = DiffOptions {
        budgets: Budgets {
            rename_rows: RowBudget::PerCell(1),
            ..Budgets::default()
        },
        ..declared("id")
    };
    let diff = diff_tables(&old, &new, &bound).unwrap();

    assert_eq!(diff.incomplete, [IncompleteStage::Renames]);
    assert!(
        diff.columns
            .identities
            .contains(&identity(1, 1, IdentityBasis::Exact))
    );
    assert_eq!(diff.columns.dropped, [3]);
    assert_eq!(diff.columns.added, [3]);

    // Two rows per cell fund both pairs, and nothing is incomplete.
    let free = DiffOptions {
        budgets: Budgets {
            rename_rows: RowBudget::PerCell(2),
            ..Budgets::default()
        },
        ..declared("id")
    };
    let diff = diff_tables(&old, &new, &free).unwrap();

    assert!(diff.incomplete.is_empty());
    assert!(
        diff.columns
            .identities
            .contains(&identity(2, 2, IdentityBasis::Exact))
    );
}

#[test]
fn a_proportional_swap_budget_binds_by_the_tables_own_size() {
    let old = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        "a" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120],
        "b" => [-1, -2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12],
        "c" => [11, 21, 31, 41, 51, 61, 71, 81, 91, 101, 111, 121],
        "d" => [-2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12, -13],
        "e" => [12, 22, 32, 42, 52, 62, 72, 82, 92, 102, 112, 122],
        "f" => [-3, -4, -5, -6, -7, -8, -9, -10, -11, -12, -13, -14],
    };
    let new = table! {
        "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        "a" => [-1, -2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12],
        "b" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120],
        "c" => [-2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12, -13],
        "d" => [11, 21, 31, 41, 51, 61, 71, 81, 91, 101, 111, 121],
        "e" => [-3, -4, -5, -6, -7, -8, -9, -10, -11, -12, -13, -14],
        "f" => [12, 22, 32, 42, 52, 62, 72, 82, 92, 102, 112, 122],
    };

    // Three simultaneous exchanges: six rewritten identities, fifteen
    // crossings to enumerate, eighteen first-time measurements of 12 rows
    // each — 216 rows against an 84-cell table. One row per cell exhausts the
    // enumeration, and exhaustion accepts nothing: every identity keeps its
    // name basis, exactly as if no swap had been found.
    let bound = DiffOptions {
        budgets: Budgets {
            swap_rows: RowBudget::PerCell(1),
            ..Budgets::default()
        },
        ..declared("id")
    };
    let diff = diff_tables(&old, &new, &bound).unwrap();

    assert_eq!(diff.incomplete, [IncompleteStage::Swaps]);
    assert!(
        diff.columns
            .identities
            .iter()
            .all(|pair| pair.basis != IdentityBasis::Swapped)
    );

    // Three rows per cell fund the whole enumeration and all three swaps land.
    let free = DiffOptions {
        budgets: Budgets {
            swap_rows: RowBudget::PerCell(3),
            ..Budgets::default()
        },
        ..declared("id")
    };
    let diff = diff_tables(&old, &new, &free).unwrap();

    assert!(diff.incomplete.is_empty());
    assert_eq!(
        diff.columns
            .identities
            .iter()
            .filter(|pair| pair.basis == IdentityBasis::Swapped)
            .count(),
        6
    );
}

#[test]
fn sampled_inference_still_infers_and_reports_nothing() {
    let old = table! {
        "id" => [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
            16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
        ],
        "a" => [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
            16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
        ],
        "b" => [
            -1, -2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12, -13, -14, -15,
            -16, -17, -18, -19, -20, -21, -22, -23, -24, -25, -26, -27, -28, -29, -30,
        ],
    };
    let new = table! {
        "id" => [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
            16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
        ],
        "a" => [
            -1, -2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12, -13, -14, -15,
            -16, -17, -18, -19, -20, -21, -22, -23, -24, -25, -26, -27, -28, -29, -30,
        ],
        "b" => [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
            16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
        ],
    };
    // Thirty matched rows against a sample cap of eight, so swap inference
    // measures a deterministic key-hash sample rather than every row. The
    // exchange still crosses perfectly over whichever rows were chosen, and
    // completing inference over the sample is not budget exhaustion: nothing
    // is reported, exactly as the design specifies.
    let options = DiffOptions {
        budgets: Budgets {
            agreement_rows: 8,
            ..Budgets::default()
        },
        ..declared("id")
    };

    let diff = diff_tables(&old, &new, &options).unwrap();

    assert!(diff.incomplete.is_empty());
    assert_eq!(
        diff.columns.identities,
        vec![
            identity(0, 0, IdentityBasis::Declared),
            identity(1, 2, IdentityBasis::Swapped),
            identity(2, 1, IdentityBasis::Swapped),
        ]
    );
    assert!(diff.cells.is_empty());

    let repeated = diff_tables(&old, &new, &options).unwrap();
    assert_eq!(diff, repeated);
    assert_eq!(render(&diff), render(&repeated));
}

#[test]
fn an_incomplete_line_is_not_a_hint_kind() {
    let old = table! { "id" => [1], "value" => [10] };
    let new = table! { "id" => [1], "value" => [10] };

    // The report lines follow `table_regenerate()`'s precedent: statements
    // about the run, not identities a user could assert, so pasting one back
    // is refused by kind like any other non-hint operation.
    assert_eq!(
        diff_tables(&old, &new, &hinted(&["id"], &["incomplete_renames(value)"])).unwrap_err(),
        DiffError::UnknownHintKind {
            hint: "incomplete_renames(value)".into(),
            kind: "incomplete_renames".into(),
        }
    );
}
