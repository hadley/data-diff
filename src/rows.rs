use std::collections::HashMap;

use crate::key::{ResolvedKey, compound_hash};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RowMatches {
    pub added: Vec<usize>,
    pub dropped: Vec<usize>,
    /// One-to-one `(old, new)` row positions, ordered by `old`.
    pub matched: Vec<(usize, usize)>,
}

pub(crate) fn match_rows(key: &ResolvedKey) -> RowMatches {
    let mut new_buckets = HashMap::<u128, Vec<usize>>::new();
    for (row, value) in key.new.iter().enumerate() {
        new_buckets
            .entry(compound_hash(value))
            .or_default()
            .push(row);
    }

    let mut result = RowMatches::default();
    let mut used_new = vec![false; key.new.len()];
    for (old_row, old_key) in key.old.iter().enumerate() {
        let matched = new_buckets
            .get(&compound_hash(old_key))
            .into_iter()
            .flatten()
            .copied()
            .find(|new_row| key.new[*new_row] == *old_key);
        if let Some(new_row) = matched {
            used_new[new_row] = true;
            result.matched.push((old_row, new_row));
        } else {
            result.dropped.push(old_row);
        }
    }
    result.added = used_new
        .iter()
        .enumerate()
        .filter_map(|(row, used)| (!used).then_some(row))
        .collect();
    result
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
    use arrow_schema::{Field, Schema};

    use super::{RowMatches, match_rows};
    use crate::DiffOptions;
    use crate::key::resolve_key;

    fn table(columns: Vec<(&str, ArrayRef)>) -> RecordBatch {
        if columns.is_empty() {
            return RecordBatch::new_empty(Arc::new(Schema::empty()));
        }
        let fields = columns
            .iter()
            .map(|(name, values)| Field::new(*name, values.data_type().clone(), true))
            .collect::<Vec<_>>();
        let arrays = columns.into_iter().map(|(_, values)| values).collect();
        RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).unwrap()
    }

    fn rows(old: RecordBatch, new: RecordBatch, key: &[&str]) -> RowMatches {
        let options = DiffOptions {
            key: key.iter().map(|value| (*value).to_owned()).collect(),
        };
        match_rows(&resolve_key(&old, &new, &options).unwrap())
    }

    #[test]
    fn classifies_add_drop_and_match_in_input_order() {
        let old = table(vec![("id", Arc::new(Int64Array::from(vec![10, 20, 30])))]);
        let new = table(vec![("id", Arc::new(Int64Array::from(vec![30, 10, 40])))]);

        assert_eq!(
            rows(old, new, &["id"]),
            RowMatches {
                added: vec![2],
                dropped: vec![1],
                matched: vec![(0, 1), (2, 0)],
            }
        );
    }

    #[test]
    fn disjoint_keys_make_every_row_atomic() {
        let old = table(vec![("id", Arc::new(Int64Array::from(vec![1, 2])))]);
        let new = table(vec![("id", Arc::new(Int64Array::from(vec![3, 4])))]);

        assert_eq!(
            rows(old, new, &["id"]),
            RowMatches {
                added: vec![0, 1],
                dropped: vec![0, 1],
                matched: vec![],
            }
        );
    }

    #[test]
    fn cross_type_compound_keys_match_canonically() {
        let old = table(vec![
            ("group", Arc::new(StringArray::from(vec!["a", "b"]))),
            ("id", Arc::new(StringArray::from(vec!["1.0", "2e0"]))),
        ]);
        let new = table(vec![
            ("group", Arc::new(StringArray::from(vec!["b", "a"]))),
            ("id", Arc::new(Int64Array::from(vec![2, 1]))),
        ]);

        assert_eq!(rows(old, new, &["group", "id"]).matched, [(0, 1), (1, 0)]);
    }

    #[test]
    fn handles_an_empty_side_without_special_cases() {
        let old = table(vec![("id", Arc::new(Int64Array::from(Vec::<i64>::new())))]);
        let new = table(vec![("id", Arc::new(Int64Array::from(vec![1, 2])))]);

        assert_eq!(
            rows(old, new, &["id"]),
            RowMatches {
                added: vec![0, 1],
                dropped: vec![],
                matched: vec![],
            }
        );
    }
}
