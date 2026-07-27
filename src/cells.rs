use arrow_array::RecordBatch;

use crate::compare::ComparisonPlan;
use crate::rows::RowMatches;
use crate::schema::SchemaMatches;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CellChanges {
    pub columns: Vec<ColumnChanges>,
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
    result
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Float64Array, Int32Array, Int64Array, RecordBatch, StringArray};
    use arrow_schema::{Field, Schema};

    use super::{ChangedCell, ColumnChanges, compare_cells};
    use crate::DiffOptions;
    use crate::key::resolve_key;
    use crate::rows::match_rows;
    use crate::schema::reconcile_schema;

    fn table(columns: Vec<(&str, ArrayRef)>) -> RecordBatch {
        let fields = columns
            .iter()
            .map(|(name, values)| Field::new(*name, values.data_type().clone(), true))
            .collect::<Vec<_>>();
        let arrays = columns.into_iter().map(|(_, values)| values).collect();
        RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).unwrap()
    }

    fn changes(old: &RecordBatch, new: &RecordBatch) -> super::CellChanges {
        changes_with(old, new, &["id"])
    }

    /// Compare cells under a specific key, or under a guessed one when `key` is
    /// empty, so key exclusion can be checked for either basis.
    fn changes_with(old: &RecordBatch, new: &RecordBatch, key: &[&str]) -> super::CellChanges {
        let options = DiffOptions {
            key: key.iter().map(|name| (*name).to_owned()).collect(),
        };
        let key = resolve_key(old, new, &options).unwrap();
        let rows = match_rows(&key);
        let schema = reconcile_schema(old, new, &key).unwrap();
        compare_cells(old, new, &schema, &rows)
    }

    #[test]
    fn reports_complete_cells_in_old_coordinate_order() {
        let old = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2]))),
            ("a", Arc::new(Int64Array::from(vec![10, 20]))),
            ("b", Arc::new(Int64Array::from(vec![30, 40]))),
        ]);
        let new = table(vec![
            ("b", Arc::new(Int64Array::from(vec![41, 31]))),
            ("id", Arc::new(Int64Array::from(vec![2, 1]))),
            ("a", Arc::new(Int64Array::from(vec![21, 11]))),
        ]);

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
        let old = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2]))),
            ("value", Arc::new(Int64Array::from(vec![10, 20]))),
        ]);
        let new = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2]))),
            ("value", Arc::new(Int64Array::from(vec![10, 21]))),
        ]);

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
        let old = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2]))),
            ("value", Arc::new(Int64Array::from(vec![10, 20]))),
        ]);
        let new = table(vec![
            ("id", Arc::new(Int64Array::from(vec![2, 3]))),
            ("value", Arc::new(Int64Array::from(vec![20, 99]))),
        ]);

        assert!(changes(&old, &new).changed_cells().is_empty());
    }

    #[test]
    fn added_and_dropped_columns_do_not_manufacture_cells() {
        let old = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1]))),
            ("drop", Arc::new(Int64Array::from(vec![10]))),
        ]);
        let new = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1]))),
            ("add", Arc::new(Int64Array::from(vec![99]))),
        ]);

        assert!(changes(&old, &new).changed_cells().is_empty());
    }

    #[test]
    fn type_changes_are_independent_of_value_changes() {
        let old = table(vec![
            ("id", Arc::new(Int32Array::from(vec![1]))),
            ("same", Arc::new(Int32Array::from(vec![10]))),
            ("changed", Arc::new(Int32Array::from(vec![20]))),
        ]);
        let new = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1]))),
            ("same", Arc::new(Int64Array::from(vec![10]))),
            ("changed", Arc::new(Float64Array::from(vec![21.0]))),
        ]);

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
    fn compatible_nulls_and_nan_do_not_change() {
        let old = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2]))),
            (
                "value",
                Arc::new(StringArray::from(vec![None, Some("NaN")])),
            ),
        ]);
        let new = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2]))),
            (
                "value",
                Arc::new(Float64Array::from(vec![None, Some(f64::NAN)])),
            ),
        ]);

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
