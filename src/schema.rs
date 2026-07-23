#![allow(dead_code)] // Consumed by ordering and cell stages.

use std::collections::HashSet;

use arrow_array::RecordBatch;

use crate::DiffError;
use crate::compare::ComparisonPlan;
use crate::key::ResolvedKey;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SchemaMatches {
    pub identities: Vec<ColumnIdentity>,
    pub added: Vec<usize>,
    pub dropped: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ColumnIdentity {
    pub old: usize,
    pub new: usize,
    pub type_changed: bool,
    pub is_key: bool,
}

pub(crate) fn reconcile_schema(
    old: &RecordBatch,
    new: &RecordBatch,
    key: &ResolvedKey,
) -> Result<SchemaMatches, DiffError> {
    let key_columns = key
        .columns
        .iter()
        .map(|column| (column.old, column.new))
        .collect::<HashSet<_>>();
    let mut matched_new = vec![false; new.num_columns()];
    let mut result = SchemaMatches::default();
    let old_schema = old.schema();
    let new_schema = new.schema();

    for (old_index, old_field) in old_schema.fields().iter().enumerate() {
        let new_index = new_schema
            .fields()
            .iter()
            .position(|field| field.name() == old_field.name());
        let Some(new_index) = new_index else {
            result.dropped.push(old_index);
            continue;
        };
        let new_field = new_schema.field(new_index);
        if ComparisonPlan::new(old_field.data_type(), new_field.data_type()).is_none() {
            return Err(DiffError::IncompatibleColumns {
                column: old_field.name().clone(),
                old_type: format!("{:?}", old_field.data_type()),
                new_type: format!("{:?}", new_field.data_type()),
            });
        }
        matched_new[new_index] = true;
        result.identities.push(ColumnIdentity {
            old: old_index,
            new: new_index,
            type_changed: old_field.data_type() != new_field.data_type(),
            is_key: key_columns.contains(&(old_index, new_index)),
        });
    }
    result.added = matched_new
        .iter()
        .enumerate()
        .filter_map(|(index, matched)| (!matched).then_some(index))
        .collect();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, BooleanArray, Float64Array, Int32Array, Int64Array, RecordBatch};
    use arrow_schema::{Field, Schema};

    use super::{ColumnIdentity, SchemaMatches, reconcile_schema};
    use crate::key::resolve_key;
    use crate::{DiffError, DiffOptions};

    fn table(columns: Vec<(&str, ArrayRef)>) -> RecordBatch {
        let fields = columns
            .iter()
            .map(|(name, values)| Field::new(*name, values.data_type().clone(), true))
            .collect::<Vec<_>>();
        let arrays = columns.into_iter().map(|(_, values)| values).collect();
        RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).unwrap()
    }

    fn reconcile(old: &RecordBatch, new: &RecordBatch) -> Result<SchemaMatches, DiffError> {
        let options = DiffOptions {
            key: vec!["id".into()],
        };
        let key = resolve_key(old, new, &options)?;
        reconcile_schema(old, new, &key)
    }

    #[test]
    fn identifies_same_names_and_classifies_unmatched_columns() {
        let old = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1]))),
            ("drop", Arc::new(Int64Array::from(vec![1]))),
            ("value", Arc::new(Float64Array::from(vec![1.0]))),
        ]);
        let new = table(vec![
            ("value", Arc::new(Float64Array::from(vec![1.0]))),
            ("id", Arc::new(Int64Array::from(vec![1]))),
            ("add", Arc::new(Int64Array::from(vec![1]))),
        ]);

        assert_eq!(
            reconcile(&old, &new).unwrap(),
            SchemaMatches {
                identities: vec![
                    ColumnIdentity {
                        old: 0,
                        new: 1,
                        type_changed: false,
                        is_key: true,
                    },
                    ColumnIdentity {
                        old: 2,
                        new: 0,
                        type_changed: false,
                        is_key: false,
                    },
                ],
                added: vec![2],
                dropped: vec![1],
            }
        );
    }

    #[test]
    fn records_key_and_non_key_type_changes() {
        let old = table(vec![
            ("id", Arc::new(Int32Array::from(vec![1]))),
            ("value", Arc::new(Int32Array::from(vec![1]))),
        ]);
        let new = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1]))),
            ("value", Arc::new(Float64Array::from(vec![1.0]))),
        ]);

        let schema = reconcile(&old, &new).unwrap();

        assert!(schema.identities[0].is_key);
        assert!(schema.identities[0].type_changed);
        assert!(schema.identities[1].type_changed);
    }

    #[test]
    fn rejects_incompatible_same_name_columns() {
        let old = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1]))),
            ("flag", Arc::new(BooleanArray::from(vec![true]))),
        ]);
        let new = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1]))),
            ("flag", Arc::new(Int64Array::from(vec![1]))),
        ]);

        assert_eq!(
            reconcile(&old, &new).unwrap_err(),
            DiffError::IncompatibleColumns {
                column: "flag".into(),
                old_type: "Boolean".into(),
                new_type: "Int64".into(),
            }
        );
    }
}
