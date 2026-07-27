use std::collections::{HashMap, HashSet};

use arrow_array::RecordBatch;
use xxhash_rust::xxh3::xxh3_128_with_seed;

use crate::compare::{CanonicalValue, ComparisonPlan, stable_hash};
use crate::{DiffError, DiffOptions, KeyBasis, KeyOverlap, Side};

#[derive(Clone, Debug)]
pub(crate) struct ResolvedKey {
    pub basis: KeyBasis,
    pub columns: Vec<KeyColumn>,
    pub old: Vec<Vec<CanonicalValue>>,
    pub new: Vec<Vec<CanonicalValue>>,
    pub overlap: Option<KeyOverlap>,
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
    if options.key.is_empty() {
        return guess_key(old, new);
    }
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
        basis: KeyBasis::Declared,
        columns,
        old: old_keys,
        new: new_keys,
        overlap: None,
    })
}

/// Select the eligible same-name single column with the most shared values.
fn guess_key(old: &RecordBatch, new: &RecordBatch) -> Result<ResolvedKey, DiffError> {
    if old.num_rows() == 0 || new.num_rows() == 0 {
        return Err(DiffError::MissingKey);
    }

    struct Candidate {
        old_index: usize,
        new_index: usize,
        name: String,
        old_values: Vec<CanonicalValue>,
        new_values: Vec<CanonicalValue>,
        shared: usize,
    }

    let new_schema = new.schema();
    let mut best: Option<Candidate> = None;
    for (old_index, old_field) in old.schema().fields().iter().enumerate() {
        let Some(new_index) = new_schema
            .fields()
            .iter()
            .position(|field| field.name() == old_field.name())
        else {
            continue;
        };
        let old_column = old.column(old_index);
        let new_column = new.column(new_index);
        let Some(plan) = ComparisonPlan::new(old_column.data_type(), new_column.data_type()) else {
            continue;
        };
        let old_values = plan.canonicalize_old(old_column.as_ref());
        let new_values = plan.canonicalize_new(new_column.as_ref());
        let Some(shared) = candidate_overlap(&old_values, &new_values, stable_hash) else {
            continue;
        };
        if shared == 0 {
            continue;
        }
        if best.as_ref().is_none_or(|best| shared > best.shared) {
            best = Some(Candidate {
                old_index,
                new_index,
                name: old_field.name().clone(),
                old_values,
                new_values,
                shared,
            });
        }
    }

    let Some(candidate) = best else {
        return Err(DiffError::MissingKey);
    };
    Ok(ResolvedKey {
        basis: KeyBasis::Guessed,
        columns: vec![KeyColumn {
            name: candidate.name,
            old: candidate.old_index,
            new: candidate.new_index,
        }],
        old: single_component_rows(candidate.old_values),
        new: single_component_rows(candidate.new_values),
        overlap: Some(KeyOverlap {
            shared: candidate.shared,
            possible: old.num_rows().min(new.num_rows()),
        }),
    })
}

fn single_component_rows(values: Vec<CanonicalValue>) -> Vec<Vec<CanonicalValue>> {
    values.into_iter().map(|value| vec![value]).collect()
}

/// Count the values a candidate column shares across sides.
///
/// Returns `None` when the column is ineligible: a null or `NaN` on either
/// side, or a duplicated value within either side. Hashes are bucket indexes
/// only; equality is confirmed inside each bucket so a collision can neither
/// manufacture a duplicate nor inflate the overlap.
fn candidate_overlap(
    old: &[CanonicalValue],
    new: &[CanonicalValue],
    hash: impl Fn(&CanonicalValue) -> u128,
) -> Option<usize> {
    if old.iter().chain(new).any(CanonicalValue::invalid_key) {
        return None;
    }
    let old_buckets = unique_buckets(old, &hash)?;
    unique_buckets(new, &hash)?;
    let shared = new
        .iter()
        .filter(|value| {
            old_buckets
                .get(&hash(value))
                .is_some_and(|rows| rows.iter().any(|&row| old[row] == **value))
        })
        .count();
    Some(shared)
}

fn unique_buckets(
    values: &[CanonicalValue],
    hash: impl Fn(&CanonicalValue) -> u128,
) -> Option<HashMap<u128, Vec<usize>>> {
    let mut buckets = HashMap::<u128, Vec<usize>>::new();
    for (row, value) in values.iter().enumerate() {
        let bucket = buckets.entry(hash(value)).or_default();
        if bucket.iter().any(|&previous| values[previous] == *value) {
            return None;
        }
        bucket.push(row);
    }
    Some(buckets)
}

fn validate_components(keys: &[String]) -> Result<Vec<&str>, DiffError> {
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
    use test_support::{rows_without_columns, table};

    use super::{candidate_overlap, resolve_key};
    use crate::compare::{CanonicalValue, stable_hash};
    use crate::{DiffError, DiffOptions, KeyBasis, KeyOverlap, Side};

    fn options(key: &[&str]) -> DiffOptions {
        DiffOptions {
            key: key.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn validates_key_syntax() {
        let empty = table! {};
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
        let old = table! { "id" => [1] };
        let new = table! { "other" => [1] };
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
        let old = table! { "id" => [true] };
        let new = table! { "id" => [1] };
        assert!(matches!(
            resolve_key(&old, &new, &options(&["id"])),
            Err(DiffError::IncompatibleKeyTypes { .. })
        ));
    }

    #[test]
    fn rejects_null_and_nan_with_row_context() {
        let old = table! { "id" => [Some(1.0), None] };
        let new = table! { "id" => [1.0, 2.0] };
        assert_eq!(
            resolve_key(&old, &new, &options(&["id"])).unwrap_err(),
            DiffError::InvalidKeyValue {
                side: Side::Old,
                component: "id".into(),
                row: 2,
            }
        );

        let old = table! { "id" => [f64::NAN] };
        let new = table! { "id" => [1.0] };
        assert!(matches!(
            resolve_key(&old, &new, &options(&["id"])),
            Err(DiffError::InvalidKeyValue { .. })
        ));
    }

    #[test]
    fn uniqueness_uses_cross_type_canonicalization() {
        let old = table! { "id" => ["1", "1.0"] };
        let new = table! { "id" => [1, 2] };
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
        let old = table! { "id" => [1, 2] };
        let new = table! { "id" => [1, 1] };
        assert_eq!(
            resolve_key(&old, &new, &options(&["id"])).unwrap_err(),
            DiffError::UnsupportedFanout {
                first_row: 1,
                row: 2,
            }
        );
    }

    #[test]
    fn guessing_rejects_an_empty_side_before_examining_candidates() {
        let empty = table! { "id" => i64[] };
        let rows = table! { "id" => [1] };
        for (old, new) in [(&empty, &rows), (&rows, &empty), (&empty, &empty)] {
            assert!(matches!(
                resolve_key(old, new, &options(&[])),
                Err(DiffError::MissingKey)
            ));
        }
    }

    #[test]
    fn guesses_the_single_eligible_column() {
        let old = table! {
            "label" => ["x", "x"],
            "id" => [1, 2],
        };
        let new = table! {
            "label" => ["x", "y"],
            "id" => [2, 3],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        assert_eq!(key.basis, KeyBasis::Guessed);
        assert_eq!(key.columns.len(), 1);
        assert_eq!(key.columns[0].name, "id");
        assert_eq!((key.columns[0].old, key.columns[0].new), (1, 1));
        assert_eq!(
            key.overlap,
            Some(KeyOverlap {
                shared: 1,
                possible: 2,
            })
        );
    }

    #[test]
    fn guessing_canonicalizes_across_compatible_types() {
        let old = table! { "id" => ["1", "2"] };
        let new = table! { "id" => [2, 3] };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        assert_eq!(key.basis, KeyBasis::Guessed);
        assert_eq!(
            key.old,
            vec![vec![CanonicalValue::Int(1)], vec![CanonicalValue::Int(2)]]
        );
        assert_eq!(
            key.new,
            vec![vec![CanonicalValue::Int(2)], vec![CanonicalValue::Int(3)]]
        );
        assert_eq!(
            key.overlap,
            Some(KeyOverlap {
                shared: 1,
                possible: 2,
            })
        );
    }

    #[test]
    fn guessing_skips_every_ineligible_candidate() {
        let old = table! {
            "null" => [Some(1), None],
            "nan" => [1.0, f64::NAN],
            "dup_old" => [1, 1],
            "dup_new" => [1, 2],
            "mismatch" => [true, false],
            "disjoint" => [1, 2],
            "missing" => [1, 2],
        };
        let new = table! {
            "null" => [1, 2],
            "nan" => [1.0, 2.0],
            "dup_old" => [1, 2],
            "dup_new" => [1, 1],
            "mismatch" => [1, 2],
            "disjoint" => [3, 4],
        };

        assert!(matches!(
            resolve_key(&old, &new, &options(&[])),
            Err(DiffError::MissingKey)
        ));
    }

    #[test]
    fn guessing_prefers_the_largest_exact_intersection() {
        let old = table! {
            "partial" => [1, 2, 3],
            "full" => [10, 20, 30],
        };
        let new = table! {
            "partial" => [3, 4, 5],
            "full" => [30, 20, 10],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        assert_eq!(key.columns[0].name, "full");
        assert_eq!(
            key.overlap,
            Some(KeyOverlap {
                shared: 3,
                possible: 3,
            })
        );
    }

    #[test]
    fn guessing_breaks_ties_by_old_column_order() {
        let old = table! { "b" => [1, 2], "a" => [1, 2] };
        let new = table! { "a" => [1, 2], "b" => [1, 2] };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        assert_eq!(key.columns[0].name, "b");
        assert_eq!((key.columns[0].old, key.columns[0].new), (0, 1));
    }

    #[test]
    fn overlap_is_normalized_by_the_smaller_side() {
        let old = table! { "id" => [1, 2, 3] };
        let new = table! { "id" => [3, 2] };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        assert_eq!(
            key.overlap,
            Some(KeyOverlap {
                shared: 2,
                possible: 2,
            })
        );
    }

    #[test]
    fn rows_without_columns_leave_nothing_to_guess() {
        let old = rows_without_columns(2);

        assert!(matches!(
            resolve_key(&old, &old, &options(&[])),
            Err(DiffError::MissingKey)
        ));
    }

    #[test]
    fn declared_keys_bypass_the_zero_row_guard() {
        let empty = table! { "id" => i64[] };

        let key = resolve_key(&empty, &empty, &options(&["id"])).unwrap();

        assert_eq!(key.basis, KeyBasis::Declared);
        assert_eq!(key.overlap, None);
        assert!(key.old.is_empty());
    }

    #[test]
    fn a_declared_key_overrides_a_stronger_candidate() {
        let old = table! {
            "weak" => [1, 2, 3],
            "strong" => [10, 20, 30],
        };
        let new = table! {
            "weak" => [1, 4, 5],
            "strong" => [10, 20, 30],
        };

        let key = resolve_key(&old, &new, &options(&["weak"])).unwrap();

        // "strong" shares all three values and "weak" only one, so guessing
        // would choose the other column; a declaration is never compared.
        assert_eq!(key.basis, KeyBasis::Declared);
        assert_eq!(key.columns[0].name, "weak");
        assert_eq!(key.overlap, None);
    }

    #[test]
    fn repeated_guessing_is_deterministic() {
        let old = table! {
            "a" => [1, 2, 3],
            "b" => [7, 8, 9],
        };
        let new = table! {
            "a" => [2, 3, 4],
            "b" => [9, 8, 7],
        };

        let first = resolve_key(&old, &new, &options(&[])).unwrap();
        let second = resolve_key(&old, &new, &options(&[])).unwrap();

        assert_eq!(first.columns[0].name, second.columns[0].name);
        assert_eq!(first.overlap, second.overlap);
        assert_eq!(first.old, second.old);
        assert_eq!(first.new, second.new);
    }

    #[test]
    fn forced_hash_collisions_cannot_fake_duplicates_or_overlap() {
        let constant = |_: &CanonicalValue| 0_u128;
        let old = vec![CanonicalValue::Int(1), CanonicalValue::Int(2)];
        let new = vec![CanonicalValue::Int(2), CanonicalValue::Int(3)];

        assert_eq!(candidate_overlap(&old, &new, constant), Some(1));
        assert_eq!(candidate_overlap(&old, &new, stable_hash), Some(1));
        assert_eq!(
            candidate_overlap(
                &[CanonicalValue::Int(1), CanonicalValue::Int(1)],
                &new,
                constant
            ),
            None
        );
    }

    #[test]
    fn compound_key_can_be_unique_when_components_are_not() {
        let old = table! {
            "group" => ["a", "a"],
            "id" => [1, 2],
        };
        let new = table! {
            "group" => ["a", "a"],
            "id" => [1, 2],
        };

        let key = resolve_key(&old, &new, &options(&["group", "id"])).unwrap();

        assert_eq!(key.columns.len(), 2);
        assert_eq!(key.old, key.new);
    }
}
