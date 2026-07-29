use arrow_array::RecordBatch;

use crate::compare::ComparisonPlan;
use crate::rows::RowMatches;
use crate::schema::SchemaMatches;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CellChanges {
    pub columns: Vec<ColumnChanges>,
    /// One entry per fanout group, ordered by old row.
    pub fanout: Vec<FanoutChanges>,
}

/// The differences between one old row and each of the new rows sharing its key.
///
/// These cells stay here rather than in `columns`, so they reach neither the
/// one-to-one cell set nor edit summarization. The group is recorded whether or
/// not anything differs, because the fanout itself is the event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FanoutChanges {
    pub old: usize,
    pub new: Vec<usize>,
    pub cells: Vec<ChangedCell>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChangedCell {
    pub old_row: usize,
    pub old_column: usize,
    pub new_row: usize,
    pub new_column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ColumnChanges {
    pub old: usize,
    pub new: usize,
    pub type_changed: bool,
    pub rows: Vec<(usize, usize)>,
}

impl CellChanges {
    pub(crate) fn changed_cells(&self) -> Vec<ChangedCell> {
        let mut cells = self
            .columns
            .iter()
            .flat_map(|column| {
                column.rows.iter().map(|&(old_row, new_row)| ChangedCell {
                    old_row,
                    old_column: column.old,
                    new_row,
                    new_column: column.new,
                })
            })
            .collect::<Vec<_>>();
        cells.sort_by_key(|cell| (cell.old_row, cell.old_column, cell.new_row, cell.new_column));
        cells
    }
}

impl ColumnChanges {
    pub(crate) fn values_changed(&self) -> bool {
        !self.rows.is_empty()
    }
}

pub(crate) fn compare_cells(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &SchemaMatches,
    rows: &RowMatches,
) -> CellChanges {
    let mut result = CellChanges::default();
    let mut fanout_cells = vec![Vec::new(); rows.fanout.len()];
    for identity in &schema.identities {
        let mut changed_rows = Vec::new();
        if !identity.is_key {
            let old_values = old.column(identity.old);
            let new_values = new.column(identity.new);
            let plan = ComparisonPlan::new(old_values.data_type(), new_values.data_type())
                .expect("schema reconciliation accepted the type pair");
            let old_values = plan.canonicalize_old(old_values.as_ref());
            let new_values = plan.canonicalize_new(new_values.as_ref());
            for &(old_row, new_row) in &rows.matched {
                if old_values[old_row] != new_values[new_row] {
                    changed_rows.push((old_row, new_row));
                }
            }
            // The same canonicalized columns serve the one-to-many comparison,
            // so a fanout costs no extra pass over the data.
            for (group, cells) in rows.fanout.iter().zip(&mut fanout_cells) {
                for &new_row in &group.new {
                    if old_values[group.old] != new_values[new_row] {
                        cells.push(ChangedCell {
                            old_row: group.old,
                            old_column: identity.old,
                            new_row,
                            new_column: identity.new,
                        });
                    }
                }
            }
        }
        if identity.type_changed || !changed_rows.is_empty() {
            result.columns.push(ColumnChanges {
                old: identity.old,
                new: identity.new,
                type_changed: identity.type_changed,
                rows: changed_rows,
            });
        }
    }

    result.fanout = rows
        .fanout
        .iter()
        .zip(fanout_cells)
        .map(|(group, mut cells)| {
            // The old row is constant within an event, so grouping by new row
            // reads as each new row's differences from the old one.
            cells.sort_by_key(|cell| (cell.new_row, cell.old_column, cell.new_column));
            FanoutChanges {
                old: group.old,
                new: group.new.clone(),
                cells,
            }
        })
        .collect();
    result
}

#[cfg(test)]
mod tests {
    use arrow_array::RecordBatch;
    use test_support::table;

    use super::{ChangedCell, ColumnChanges, FanoutChanges, compare_cells};
    use crate::DiffOptions;
    use crate::key::testing::resolve_key;
    use crate::rows::match_rows;
    use crate::schema::testing::reconcile_schema;

    fn changes(old: &RecordBatch, new: &RecordBatch) -> super::CellChanges {
        changes_with(old, new, &["id"])
    }

    /// Compare cells under a specific key, or under a guessed one when `key` is
    /// empty, so key exclusion can be checked for either basis.
    fn changes_with(old: &RecordBatch, new: &RecordBatch, key: &[&str]) -> super::CellChanges {
        let options = DiffOptions {
            key: key.iter().map(|name| (*name).to_owned()).collect(),
            hints: Vec::new(),
        };
        let key = resolve_key(old, new, &options).unwrap();
        let rows = match_rows(&key);
        let schema = reconcile_schema(old, new, &key).unwrap();
        compare_cells(old, new, &schema, &rows)
    }

    #[test]
    fn reports_complete_cells_in_old_coordinate_order() {
        let old = table! {
            "id" => [1, 2],
            "a" => [10, 20],
            "b" => [30, 40],
        };
        let new = table! {
            "b" => [41, 31],
            "id" => [2, 1],
            "a" => [21, 11],
        };

        let changes = changes(&old, &new);

        assert_eq!(
            changes.changed_cells(),
            [
                ChangedCell {
                    old_row: 0,
                    old_column: 1,
                    new_row: 1,
                    new_column: 2,
                },
                ChangedCell {
                    old_row: 0,
                    old_column: 2,
                    new_row: 1,
                    new_column: 0,
                },
                ChangedCell {
                    old_row: 1,
                    old_column: 1,
                    new_row: 0,
                    new_column: 2,
                },
                ChangedCell {
                    old_row: 1,
                    old_column: 2,
                    new_row: 0,
                    new_column: 0,
                },
            ]
        );
        assert_eq!(
            changes.columns,
            [
                ColumnChanges {
                    old: 1,
                    new: 2,
                    type_changed: false,
                    rows: vec![(0, 1), (1, 0)],
                },
                ColumnChanges {
                    old: 2,
                    new: 0,
                    type_changed: false,
                    rows: vec![(0, 1), (1, 0)],
                },
            ]
        );
    }

    #[test]
    fn a_guessed_key_column_is_excluded_like_a_declared_one() {
        let old = table! {
            "id" => [1, 2],
            "value" => [10, 20],
        };
        let new = table! {
            "id" => [1, 2],
            "value" => [10, 21],
        };

        // "id" shares both values and "value" only one, so guessing selects
        // "id"; the selected column is excluded whatever the basis.
        assert_eq!(
            changes_with(&old, &new, &[]).changed_cells(),
            [ChangedCell {
                old_row: 1,
                old_column: 1,
                new_row: 1,
                new_column: 1,
            }]
        );
    }

    #[test]
    fn added_and_dropped_rows_do_not_manufacture_cells() {
        let old = table! {
            "id" => [1, 2],
            "value" => [10, 20],
        };
        let new = table! {
            "id" => [2, 3],
            "value" => [20, 99],
        };

        assert!(changes(&old, &new).changed_cells().is_empty());
    }

    #[test]
    fn added_and_dropped_columns_do_not_manufacture_cells() {
        let old = table! {
            "id" => [1],
            "drop" => [10],
        };
        let new = table! {
            "id" => [1],
            "add" => [99],
        };

        assert!(changes(&old, &new).changed_cells().is_empty());
    }

    #[test]
    fn type_changes_are_independent_of_value_changes() {
        let old = table! {
            "id" => i32[1],
            "same" => i32[10],
            "changed" => i32[20],
        };
        let new = table! {
            "id" => [1],
            "same" => [10],
            "changed" => [21.0],
        };

        let changes = changes(&old, &new);

        assert_eq!(
            changes.columns,
            [
                ColumnChanges {
                    old: 0,
                    new: 0,
                    type_changed: true,
                    rows: vec![],
                },
                ColumnChanges {
                    old: 1,
                    new: 1,
                    type_changed: true,
                    rows: vec![],
                },
                ColumnChanges {
                    old: 2,
                    new: 2,
                    type_changed: true,
                    rows: vec![(0, 0)],
                },
            ]
        );
        assert_eq!(changes.changed_cells().len(), 1);
    }

    #[test]
    fn one_column_separates_its_fanout_cells_from_its_matched_cells() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "value" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
            "value" => [0, 0, 0, 0, 7, 0, 0, 5, 0, 0, 0],
        };

        let changes = changes(&old, &new);

        // Separation partitions a column's cells rather than suppressing the
        // column: the matched change is still an ordinary changed cell.
        assert_eq!(
            changes.columns,
            [ColumnChanges {
                old: 1,
                new: 1,
                type_changed: false,
                rows: vec![(6, 7)],
            }]
        );
        assert_eq!(
            changes.changed_cells(),
            [ChangedCell {
                old_row: 6,
                old_column: 1,
                new_row: 7,
                new_column: 1,
            }]
        );
        // The fanout change is reachable only through its event.
        assert_eq!(
            changes.fanout,
            [FanoutChanges {
                old: 3,
                new: vec![3, 4],
                cells: vec![ChangedCell {
                    old_row: 3,
                    old_column: 1,
                    new_row: 4,
                    new_column: 1,
                }],
            }]
        );
    }

    #[test]
    fn a_fanout_event_survives_with_no_changed_cells() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "value" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
            "value" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };

        let changes = changes(&old, &new);

        // The fanout itself is the event, whether or not values differ.
        assert_eq!(
            changes.fanout,
            [FanoutChanges {
                old: 3,
                new: vec![3, 4],
                cells: vec![],
            }]
        );
        assert!(changes.columns.is_empty());
    }

    #[test]
    fn fanout_events_exclude_key_added_and_dropped_columns() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "drop" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
            "add" => [0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0],
        };

        // The key agrees by construction and neither other column is
        // identified, so the event has nothing comparable to report.
        assert!(changes(&old, &new).fanout[0].cells.is_empty());
    }

    #[test]
    fn a_compound_key_fans_out_on_the_whole_tuple() {
        let old = table! {
            "group" => ["a", "a", "a", "a", "a", "b", "b", "b", "b", "b"],
            "id" => [1, 2, 3, 4, 5, 1, 2, 3, 4, 5],
            "value" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        let new = table! {
            "group" => ["a", "a", "a", "a", "a", "b", "b", "b", "b", "b", "b"],
            "id" => ["1", "2", "3", "4", "5", "1", "2", "3", "3", "4", "5"],
            "value" => [0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0],
        };

        let changes = changes_with(&old, &new, &["group", "id"]);

        // ("b", 3) is duplicated while ("a", 3) is not, and the new component
        // is a string, so the tuple and its comparison plans both matter.
        assert_eq!(
            changes.fanout,
            [FanoutChanges {
                old: 7,
                new: vec![7, 8],
                cells: vec![ChangedCell {
                    old_row: 7,
                    old_column: 2,
                    new_row: 8,
                    new_column: 2,
                }],
            }]
        );
    }

    #[test]
    fn compatible_nulls_and_nan_do_not_change() {
        let old = table! {
            "id" => [1, 2],
            "value" => [None, Some("NaN")],
        };
        let new = table! {
            "id" => [1, 2],
            "value" => [None, Some(f64::NAN)],
        };

        let changes = changes(&old, &new);

        assert!(changes.changed_cells().is_empty());
        assert_eq!(
            changes.columns,
            [ColumnChanges {
                old: 1,
                new: 1,
                type_changed: true,
                rows: vec![],
            }]
        );
    }
}
