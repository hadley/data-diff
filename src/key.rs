#![allow(dead_code)] // The resolved keys are consumed by the next row-matching stage.

use std::collections::{HashMap, HashSet};

use arrow_array::RecordBatch;
use xxhash_rust::xxh3::xxh3_128_with_seed;

use crate::compare::{CanonicalValue, ComparisonPlan, stable_hash};
use crate::{DiffError, DiffOptions, Side};

#[derive(Clone, Debug)]
pub(crate) struct ResolvedKey {
    pub columns: Vec<KeyColumn>,
    pub old: Vec<Vec<CanonicalValue>>,
    pub new: Vec<Vec<CanonicalValue>>,
}

#[derive(Clone, Debug)]
pub(crate) struct KeyColumn {
    pub name: String,
    pub old: usize,
    pub new: usize,
}

pub(crate) fn resolve_key(
    old: &RecordBatch,
    new: &RecordBatch,
    options: &DiffOptions,
) -> Result<ResolvedKey, DiffError> {
    let components = validate_components(&options.key)?;
    let mut columns = Vec::with_capacity(components.len());
    let mut old_components = Vec::with_capacity(components.len());
    let mut new_components = Vec::with_capacity(components.len());

    for name in components {
        let old_index = column_index(old, name, Side::Old)?;
        let new_index = column_index(new, name, Side::New)?;
        let old_values = old.column(old_index);
        let new_values = new.column(new_index);
        let plan = ComparisonPlan::new(old_values.data_type(), new_values.data_type()).ok_or_else(
            || DiffError::IncompatibleKeyTypes {
                component: name.to_owned(),
                old_type: format!("{:?}", old_values.data_type()),
                new_type: format!("{:?}", new_values.data_type()),
            },
        )?;
        old_components.push(plan.canonicalize_old(old_values.as_ref()));
        new_components.push(plan.canonicalize_new(new_values.as_ref()));
        columns.push(KeyColumn {
            name: name.to_owned(),
            old: old_index,
            new: new_index,
        });
    }

    let old_keys = transpose(old.num_rows(), &old_components);
    let new_keys = transpose(new.num_rows(), &new_components);
    validate_present(&old_keys, &columns, Side::Old)?;
    validate_present(&new_keys, &columns, Side::New)?;
    validate_unique(&old_keys, Side::Old)?;
    validate_unique(&new_keys, Side::New)?;

    Ok(ResolvedKey {
        columns,
        old: old_keys,
        new: new_keys,
    })
}

fn validate_components(keys: &[String]) -> Result<Vec<&str>, DiffError> {
    if keys.is_empty() {
        return Err(DiffError::MissingKey);
    }
    let mut seen = HashSet::new();
    let mut components = Vec::with_capacity(keys.len());
    for component in keys {
        if component.is_empty() {
            return Err(DiffError::EmptyKeyComponent);
        }
        if component.contains('/') {
            return Err(DiffError::PairedKeyUnsupported {
                component: component.clone(),
            });
        }
        if !seen.insert(component.as_str()) {
            return Err(DiffError::DuplicateKeyComponent {
                component: component.clone(),
            });
        }
        components.push(component.as_str());
    }
    Ok(components)
}

fn column_index(table: &RecordBatch, name: &str, side: Side) -> Result<usize, DiffError> {
    table
        .schema()
        .fields()
        .iter()
        .position(|field| field.name() == name)
        .ok_or_else(|| DiffError::MissingKeyColumn {
            side,
            component: name.to_owned(),
        })
}

fn transpose(rows: usize, columns: &[Vec<CanonicalValue>]) -> Vec<Vec<CanonicalValue>> {
    (0..rows)
        .map(|row| columns.iter().map(|column| column[row].clone()).collect())
        .collect()
}

fn validate_present(
    keys: &[Vec<CanonicalValue>],
    columns: &[KeyColumn],
    side: Side,
) -> Result<(), DiffError> {
    for (row, key) in keys.iter().enumerate() {
        for (component, value) in key.iter().enumerate() {
            if value.invalid_key() {
                return Err(DiffError::InvalidKeyValue {
                    side,
                    component: columns[component].name.clone(),
                    row: row + 1,
                });
            }
        }
    }
    Ok(())
}

fn validate_unique(keys: &[Vec<CanonicalValue>], side: Side) -> Result<(), DiffError> {
    let mut buckets = HashMap::<u128, Vec<usize>>::new();
    for (row, key) in keys.iter().enumerate() {
        let hash = compound_hash(key);
        if let Some(first) = buckets
            .entry(hash)
            .or_default()
            .iter()
            .copied()
            .find(|previous| keys[*previous] == *key)
        {
            return match side {
                Side::Old => Err(DiffError::NonUniqueOldKey {
                    first_row: first + 1,
                    row: row + 1,
                }),
                Side::New => Err(DiffError::UnsupportedFanout {
                    first_row: first + 1,
                    row: row + 1,
                }),
            };
        }
        buckets.get_mut(&hash).unwrap().push(row);
    }
    Ok(())
}

pub(crate) fn compound_hash(key: &[CanonicalValue]) -> u128 {
    let mut bytes = Vec::with_capacity(8 + key.len() * 24);
    bytes.extend_from_slice(&(key.len() as u64).to_le_bytes());
    for component in key {
        let hash = stable_hash(component).to_le_bytes();
        bytes.extend_from_slice(&(hash.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&hash);
    }
    xxh3_128_with_seed(&bytes, 0)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray};
    use arrow_schema::{Field, Schema};

    use super::resolve_key;
    use crate::{DiffError, DiffOptions, Side};

    fn table(columns: Vec<(&str, ArrayRef)>) -> RecordBatch {
        let fields = columns
            .iter()
            .map(|(name, values)| Field::new(*name, values.data_type().clone(), true))
            .collect::<Vec<_>>();
        let arrays = columns.into_iter().map(|(_, values)| values).collect();
        RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).unwrap()
    }

    fn options(key: &[&str]) -> DiffOptions {
        DiffOptions {
            key: key.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn validates_key_syntax() {
        let empty = RecordBatch::new_empty(Arc::new(Schema::empty()));
        assert!(matches!(
            resolve_key(&empty, &empty, &options(&[])),
            Err(DiffError::MissingKey)
        ));
        assert!(matches!(
            resolve_key(&empty, &empty, &options(&[""])),
            Err(DiffError::EmptyKeyComponent)
        ));
        assert!(matches!(
            resolve_key(&empty, &empty, &options(&["old/new"])),
            Err(DiffError::PairedKeyUnsupported { .. })
        ));
        assert!(matches!(
            resolve_key(&empty, &empty, &options(&["id", "id"])),
            Err(DiffError::DuplicateKeyComponent { .. })
        ));
    }

    #[test]
    fn identifies_the_side_of_a_missing_component() {
        let old = table(vec![("id", Arc::new(Int64Array::from(vec![1])))]);
        let new = table(vec![("other", Arc::new(Int64Array::from(vec![1])))]);
        assert_eq!(
            resolve_key(&old, &new, &options(&["id"])).unwrap_err(),
            DiffError::MissingKeyColumn {
                side: Side::New,
                component: "id".into(),
            }
        );
    }

    #[test]
    fn rejects_incompatible_key_types() {
        let old = table(vec![("id", Arc::new(BooleanArray::from(vec![true])))]);
        let new = table(vec![("id", Arc::new(Int64Array::from(vec![1])))]);
        assert!(matches!(
            resolve_key(&old, &new, &options(&["id"])),
            Err(DiffError::IncompatibleKeyTypes { .. })
        ));
    }

    #[test]
    fn rejects_null_and_nan_with_row_context() {
        let old = table(vec![(
            "id",
            Arc::new(Float64Array::from(vec![Some(1.0), None])),
        )]);
        let new = table(vec![("id", Arc::new(Float64Array::from(vec![1.0, 2.0])))]);
        assert_eq!(
            resolve_key(&old, &new, &options(&["id"])).unwrap_err(),
            DiffError::InvalidKeyValue {
                side: Side::Old,
                component: "id".into(),
                row: 2,
            }
        );

        let old = table(vec![("id", Arc::new(Float64Array::from(vec![f64::NAN])))]);
        let new = table(vec![("id", Arc::new(Float64Array::from(vec![1.0])))]);
        assert!(matches!(
            resolve_key(&old, &new, &options(&["id"])),
            Err(DiffError::InvalidKeyValue { .. })
        ));
    }

    #[test]
    fn uniqueness_uses_cross_type_canonicalization() {
        let old = table(vec![("id", Arc::new(StringArray::from(vec!["1", "1.0"])))]);
        let new = table(vec![("id", Arc::new(Int64Array::from(vec![1, 2])))]);
        assert_eq!(
            resolve_key(&old, &new, &options(&["id"])).unwrap_err(),
            DiffError::NonUniqueOldKey {
                first_row: 1,
                row: 2,
            }
        );
    }

    #[test]
    fn distinguishes_new_side_fanout_from_a_broken_old_key() {
        let old = table(vec![("id", Arc::new(Int64Array::from(vec![1, 2])))]);
        let new = table(vec![("id", Arc::new(Int64Array::from(vec![1, 1])))]);
        assert_eq!(
            resolve_key(&old, &new, &options(&["id"])).unwrap_err(),
            DiffError::UnsupportedFanout {
                first_row: 1,
                row: 2,
            }
        );
    }

    #[test]
    fn compound_key_can_be_unique_when_components_are_not() {
        let old = table(vec![
            ("group", Arc::new(StringArray::from(vec!["a", "a"]))),
            ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ]);
        let new = table(vec![
            ("group", Arc::new(StringArray::from(vec!["a", "a"]))),
            ("id", Arc::new(Int64Array::from(vec![1, 2]))),
        ]);

        let key = resolve_key(&old, &new, &options(&["group", "id"])).unwrap();

        assert_eq!(key.columns.len(), 2);
        assert_eq!(key.old, key.new);
    }
}
