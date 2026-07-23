use arrow_array::RecordBatch;

use crate::compare::ComparisonPlan;
use crate::rows::RowMatches;
use crate::schema::SchemaMatches;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CellChanges {
    pub cells: Vec<ChangedCell>,
    pub columns: Vec<ChangedColumn>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChangedCell {
    pub old_row: usize,
    pub old_column: usize,
    pub new_row: usize,
    pub new_column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChangedColumn {
    pub old: usize,
    pub new: usize,
    pub type_changed: bool,
    pub values_changed: bool,
}

pub(crate) fn compare_cells(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &SchemaMatches,
    rows: &RowMatches,
) -> CellChanges {
    let mut result = CellChanges::default();
    for identity in &schema.identities {
        let mut values_changed = false;
        if !identity.is_key {
            let old_values = old.column(identity.old);
            let new_values = new.column(identity.new);
            let plan = ComparisonPlan::new(old_values.data_type(), new_values.data_type())
                .expect("schema reconciliation accepted the type pair");
            let old_values = plan.canonicalize_old(old_values.as_ref());
            let new_values = plan.canonicalize_new(new_values.as_ref());
            for &(old_row, new_row) in &rows.matched {
                if old_values[old_row] != new_values[new_row] {
                    values_changed = true;
                    result.cells.push(ChangedCell {
                        old_row,
                        old_column: identity.old,
                        new_row,
                        new_column: identity.new,
                    });
                }
            }
        }
        if identity.type_changed || values_changed {
            result.columns.push(ChangedColumn {
                old: identity.old,
                new: identity.new,
                type_changed: identity.type_changed,
                values_changed,
            });
        }
    }
    result
        .cells
        .sort_by_key(|cell| (cell.old_row, cell.old_column, cell.new_row, cell.new_column));
    result
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Float64Array, Int32Array, Int64Array, RecordBatch, StringArray};
    use arrow_schema::{Field, Schema};

    use super::{ChangedCell, ChangedColumn, compare_cells};
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
        let options = DiffOptions {
            key: vec!["id".into()],
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
            changes.cells,
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
        assert_eq!(changes.columns.len(), 2);
        assert!(changes.columns.iter().all(|column| column.values_changed));
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

        assert!(changes(&old, &new).cells.is_empty());
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

        assert!(changes(&old, &new).cells.is_empty());
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
                ChangedColumn {
                    old: 0,
                    new: 0,
                    type_changed: true,
                    values_changed: false,
                },
                ChangedColumn {
                    old: 1,
                    new: 1,
                    type_changed: true,
                    values_changed: false,
                },
                ChangedColumn {
                    old: 2,
                    new: 2,
                    type_changed: true,
                    values_changed: true,
                },
            ]
        );
        assert_eq!(changes.cells.len(), 1);
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

        assert!(changes.cells.is_empty());
        assert_eq!(
            changes.columns,
            [ChangedColumn {
                old: 1,
                new: 1,
                type_changed: true,
                values_changed: false,
            }]
        );
    }
}
