use crate::key::{KeyIndex, ResolvedKey};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RowMatches {
    pub added: Vec<usize>,
    pub dropped: Vec<usize>,
    /// One-to-one `(old, new)` row positions, ordered by `old`.
    pub matched: Vec<(usize, usize)>,
    /// One-to-many groups, ordered by `old`.
    pub fanout: Vec<FanoutGroup>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FanoutGroup {
    pub old: usize,
    pub new: Vec<usize>,
}

/// Classify every row by its key.
///
/// Old-side presence is decided before new-side multiplicity, so a key that
/// repeats in `new` without occurring in `old` produces additions rather than a
/// fanout. The key is unique in `old`, so each group of new rows is claimed by
/// at most one old row and the four results partition the rows between them.
pub(crate) fn match_rows(key: &ResolvedKey) -> RowMatches {
    let index = KeyIndex::new(&key.new);
    let mut result = RowMatches::default();
    let mut used_new = vec![false; key.new.len()];
    for (old_row, old_key) in key.old.iter().enumerate() {
        let group = index.rows(old_key).collect::<Vec<_>>();
        for &new_row in &group {
            used_new[new_row] = true;
        }
        match group.len() {
            0 => result.dropped.push(old_row),
            1 => result.matched.push((old_row, group[0])),
            _ => result.fanout.push(FanoutGroup {
                old: old_row,
                new: group,
            }),
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
    use arrow_array::RecordBatch;
    use test_support::table;

    use super::{FanoutGroup, RowMatches, match_rows};
    use crate::DiffOptions;
    use crate::key::testing::resolve_key;

    fn rows(old: RecordBatch, new: RecordBatch, key: &[&str]) -> RowMatches {
        let options = DiffOptions {
            key: key.iter().map(|value| (*value).to_owned()).collect(),
            ..DiffOptions::default()
        };
        match_rows(&resolve_key(&old, &new, &options).unwrap())
    }

    #[test]
    fn classifies_add_drop_and_match_in_input_order() {
        let old = table! { "id" => [10, 20, 30] };
        let new = table! { "id" => [30, 10, 40] };

        assert_eq!(
            rows(old, new, &["id"]),
            RowMatches {
                added: vec![2],
                dropped: vec![1],
                matched: vec![(0, 1), (2, 0)],
                fanout: vec![],
            }
        );
    }

    #[test]
    fn disjoint_keys_make_every_row_atomic() {
        let old = table! { "id" => [1, 2] };
        let new = table! { "id" => [3, 4] };

        assert_eq!(
            rows(old, new, &["id"]),
            RowMatches {
                added: vec![0, 1],
                dropped: vec![0, 1],
                matched: vec![],
                fanout: vec![],
            }
        );
    }

    #[test]
    fn cross_type_compound_keys_match_canonically() {
        let old = table! {
            "group" => ["a", "b"],
            "id" => ["1.0", "2e0"],
        };
        let new = table! {
            "group" => ["b", "a"],
            "id" => [2, 1],
        };

        assert_eq!(rows(old, new, &["group", "id"]).matched, [(0, 1), (1, 0)]);
    }

    #[test]
    fn classifies_fanout_alongside_matches_adds_and_drops() {
        let old = table! { "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] };
        let new = table! { "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10, 99] };

        let rows = rows(old, new, &["id"]);

        assert_eq!(
            rows.fanout,
            [FanoutGroup {
                old: 3,
                new: vec![3, 4],
            }]
        );
        assert_eq!(rows.dropped, [10]);
        assert_eq!(rows.added, [11]);

        assert_eq!(rows.matched.len(), 9);
        assert!(!rows.matched.iter().any(|&(old, _)| old == 3));
        assert!(!rows.matched.iter().any(|&(_, new)| new == 3 || new == 4));
    }

    #[test]
    fn a_duplicate_without_an_old_row_is_two_additions() {
        let old = table! { "id" => [1, 2] };
        let new = table! { "id" => [1, 2, 3, 3] };

        let rows = rows(old, new, &["id"]);

        assert_eq!(rows.added, [2, 3]);
        assert!(rows.fanout.is_empty());
    }

    #[test]
    fn handles_an_empty_side_without_special_cases() {
        let old = table! { "id" => i64[] };
        let new = table! { "id" => [1, 2] };

        assert_eq!(
            rows(old, new, &["id"]),
            RowMatches {
                added: vec![0, 1],
                dropped: vec![],
                matched: vec![],
                fanout: vec![],
            }
        );
    }
}
