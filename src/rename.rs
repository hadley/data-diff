//! Identify a dropped and an added column as one renamed column.

use std::collections::HashMap;

use arrow_array::RecordBatch;

use crate::compare::{CanonicalValue, ComparisonPlan, sequence_hash};
use crate::rows::RowMatches;
use crate::schema::{ColumnIdentity, SchemaMatches};

/// Pair provisional drops with provisional adds that hold the same values.
///
/// Only the one-to-one matched rows are evidence: added, dropped, and
/// fanned-out rows are outside the alignment and take no part. With no matched
/// rows there is nothing to compare, so every candidate keeps its provisional
/// classification.
pub(crate) fn infer_exact(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &mut SchemaMatches,
    rows: &RowMatches,
) {
    infer_exact_with(old, new, schema, rows, sequence_hash);
}

/// The digest is injectable so a test can force every column to collide, which
/// is the only way to reach the verification that decides a rename.
fn infer_exact_with(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &mut SchemaMatches,
    rows: &RowMatches,
    digest: fn(&[CanonicalValue]) -> u128,
) {
    if rows.matched.is_empty() || schema.dropped.is_empty() || schema.added.is_empty() {
        return;
    }
    let mut values = Aligned::new(old, new, rows, digest);
    let mut claimed = vec![false; schema.added.len()];
    let mut accepted = Vec::new();

    // Candidates are walked in column order on both sides, so where several
    // columns are exactly equal they pair off in that order.
    for &old_index in &schema.dropped {
        let old_column = old.column(old_index);
        for (position, &new_index) in schema.added.iter().enumerate() {
            if claimed[position] {
                continue;
            }
            let new_column = new.column(new_index);
            let Some(plan) = ComparisonPlan::new(old_column.data_type(), new_column.data_type())
            else {
                continue;
            };
            if values.agree(plan, old_index, new_index) {
                claimed[position] = true;
                accepted.push((old_index, new_index));
                break;
            }
        }
    }

    for (old_index, new_index) in accepted {
        schema.dropped.retain(|&index| index != old_index);
        schema.added.retain(|&index| index != new_index);
        schema.identities.push(ColumnIdentity {
            old: old_index,
            new: new_index,
            type_changed: old.column(old_index).data_type() != new.column(new_index).data_type(),
            is_key: false,
        });
    }
    // Inferred identities are found in candidate order, while `detect_order`
    // requires the whole list to ascend by old position.
    schema.identities.sort_by_key(|identity| identity.old);
}

/// Candidate columns projected onto the matched rows, digested on demand.
///
/// Canonicalization depends on the pair rather than the column — a string
/// column keeps its bytes against another string column but is parsed into
/// integers against an integer one — so a digest is cached per column *and*
/// plan. A candidate takes part in one or two plans in practice, which keeps
/// this a small multiple of one pass per column rather than one pass per pair.
struct Aligned<'a> {
    old: &'a RecordBatch,
    new: &'a RecordBatch,
    rows: &'a RowMatches,
    digest: fn(&[CanonicalValue]) -> u128,
    cache: HashMap<(Side, usize, ComparisonPlan), (u128, Vec<CanonicalValue>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Side {
    Old,
    New,
}

impl<'a> Aligned<'a> {
    fn new(
        old: &'a RecordBatch,
        new: &'a RecordBatch,
        rows: &'a RowMatches,
        digest: fn(&[CanonicalValue]) -> u128,
    ) -> Self {
        Self {
            old,
            new,
            rows,
            digest,
            cache: HashMap::new(),
        }
    }

    /// Whether the two columns hold equal values in every matched row.
    ///
    /// Digests only decide which pairs are worth comparing; the values
    /// themselves decide the answer, so a collision cannot invent a rename.
    fn agree(&mut self, plan: ComparisonPlan, old_index: usize, new_index: usize) -> bool {
        let (old_digest, _) = self.entry(plan, Side::Old, old_index);
        let (new_digest, _) = self.entry(plan, Side::New, new_index);
        if old_digest != new_digest {
            return false;
        }
        let old_values = &self.cache[&(Side::Old, old_index, plan)].1;
        let new_values = &self.cache[&(Side::New, new_index, plan)].1;
        old_values == new_values
    }

    fn entry(&mut self, plan: ComparisonPlan, side: Side, index: usize) -> (u128, usize) {
        if let Some((digest, values)) = self.cache.get(&(side, index, plan)) {
            return (*digest, values.len());
        }
        let values = match side {
            Side::Old => {
                let column = plan.canonicalize_old(self.old.column(index).as_ref());
                project(&column, self.rows.matched.iter().map(|&(old, _)| old))
            }
            Side::New => {
                let column = plan.canonicalize_new(self.new.column(index).as_ref());
                project(&column, self.rows.matched.iter().map(|&(_, new)| new))
            }
        };
        let digest = (self.digest)(&values);
        let length = values.len();
        self.cache.insert((side, index, plan), (digest, values));
        (digest, length)
    }
}

/// Take a column's values at the given rows, in the order the rows are given.
///
/// `rows.matched` is ordered by old position and holds each pair's new
/// position alongside it, so projecting each side through its own half of the
/// pair puts both columns in one order without materializing aligned tables.
fn project(values: &[CanonicalValue], rows: impl Iterator<Item = usize>) -> Vec<CanonicalValue> {
    rows.map(|row| values[row].clone()).collect()
}

#[cfg(test)]
mod tests {
    use arrow_array::RecordBatch;
    use test_support::table;

    use super::{infer_exact, infer_exact_with};
    use crate::DiffOptions;
    use crate::compare::CanonicalValue;
    use crate::key::resolve_key;
    use crate::rows::match_rows;
    use crate::schema::{SchemaMatches, reconcile_schema};

    fn infer(old: &RecordBatch, new: &RecordBatch) -> SchemaMatches {
        let options = DiffOptions {
            key: vec!["id".into()],
        };
        let key = resolve_key(old, new, &options).unwrap();
        let rows = match_rows(&key);
        let mut schema = reconcile_schema(old, new, &key).unwrap();
        infer_exact(old, new, &mut schema, &rows);
        schema
    }

    /// The `(old, new)` pairs of every identity that is not the key.
    fn renames(schema: &SchemaMatches) -> Vec<(usize, usize)> {
        schema
            .identities
            .iter()
            .filter(|identity| !identity.is_key)
            .map(|identity| (identity.old, identity.new))
            .collect()
    }

    #[test]
    fn identical_values_identify_a_renamed_column() {
        let old = table! {
            "id" => [1, 2, 3],
            "amount" => [10, 20, 30],
        };
        let new = table! {
            "id" => [1, 2, 3],
            "total" => [10, 20, 30],
        };

        let schema = infer(&old, &new);

        assert_eq!(renames(&schema), [(1, 1)]);
        assert!(schema.dropped.is_empty());
        assert!(schema.added.is_empty());
        assert!(!schema.identities[1].type_changed);
    }

    #[test]
    fn a_rename_is_found_across_a_compatible_type_change() {
        let old = table! {
            "id" => [1, 2, 3],
            "amount" => ["10", "20", "30"],
        };
        let new = table! {
            "id" => [1, 2, 3],
            "total" => [10, 20, 30],
        };

        let schema = infer(&old, &new);

        // The values are equal only under the pair's own comparison plan, so a
        // digest taken per column rather than per plan would miss this.
        assert_eq!(renames(&schema), [(1, 1)]);
        assert!(schema.identities[1].type_changed);
    }

    #[test]
    fn candidates_with_incompatible_types_are_left_alone() {
        let old = table! {
            "id" => [1, 2],
            "flag" => [true, false],
        };
        let new = table! {
            "id" => [1, 2],
            "count" => [1, 0],
        };

        let schema = infer(&old, &new);

        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped, [1]);
        assert_eq!(schema.added, [1]);
    }

    #[test]
    fn a_column_matching_nothing_stays_a_drop() {
        let old = table! {
            "id" => [1, 2],
            "gone" => [10, 20],
        };
        let new = table! {
            "id" => [1, 2],
            "fresh" => [11, 21],
        };

        let schema = infer(&old, &new);

        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped, [1]);
        assert_eq!(schema.added, [1]);
    }

    #[test]
    fn without_matched_rows_there_is_no_evidence() {
        let old = table! {
            "id" => [1, 2],
            "amount" => [10, 20],
        };
        let new = table! {
            "id" => [3, 4],
            "total" => [10, 20],
        };

        let schema = infer(&old, &new);

        // The columns are identical, but no row is common to both files, so
        // nothing connects them.
        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped, [1]);
        assert_eq!(schema.added, [1]);
    }

    #[test]
    fn equal_candidates_pair_off_in_column_order() {
        let old = table! {
            "id" => [1, 2],
            "a" => [10, 20],
            "b" => [10, 20],
        };
        let new = table! {
            "id" => [1, 2],
            "x" => [10, 20],
            "y" => [10, 20],
        };

        // Every candidate equals every other, which the design resolves by
        // column order rather than by refusing to choose.
        assert_eq!(renames(&infer(&old, &new)), [(1, 1), (2, 2)]);
    }

    #[test]
    fn matched_rows_are_compared_in_their_own_order() {
        let old = table! {
            "id" => [1, 2, 3],
            "amount" => [10, 20, 30],
        };
        let new = table! {
            "id" => [3, 1, 2],
            "total" => [30, 10, 20],
        };

        // Row 1 of "amount" is row 2 of "total". Comparing the columns by
        // position would find nothing.
        assert_eq!(renames(&infer(&old, &new)), [(1, 1)]);
    }

    #[test]
    fn fanned_out_rows_are_not_evidence() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "amount" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
            "total" => [10, 20, 30, 40, 99, 50, 60, 70, 80, 90, 100],
        };

        // The extra row of the fanned-out key disagrees, but it is outside the
        // alignment, so the matched rows still agree everywhere.
        assert_eq!(renames(&infer(&old, &new)), [(1, 1)]);
    }

    #[test]
    fn a_forced_digest_collision_cannot_invent_a_rename() {
        let old = table! {
            "id" => [1, 2],
            "gone" => [10, 20],
            "amount" => [30, 40],
        };
        let new = table! {
            "id" => [1, 2],
            "fresh" => [11, 21],
            "total" => [30, 40],
        };

        let options = DiffOptions {
            key: vec!["id".into()],
        };
        let key = resolve_key(&old, &new, &options).unwrap();
        let rows = match_rows(&key);
        let mut schema = reconcile_schema(&old, &new, &key).unwrap();
        infer_exact_with(&old, &new, &mut schema, &rows, |_: &[CanonicalValue]| 0);

        // Every column now digests alike, so only the elementwise comparison
        // separates the real rename from the unrelated pair.
        assert_eq!(renames(&schema), [(2, 2)]);
        assert_eq!(schema.dropped, [1]);
        assert_eq!(schema.added, [1]);
    }

    #[test]
    fn columns_of_nulls_are_equal_and_are_paired() {
        let old = table! {
            "id" => [1, 2],
            "gone" => [None::<i64>, None],
        };
        let new = table! {
            "id" => [1, 2],
            "fresh" => [None::<i64>, None],
        };

        // Exact inference has no information-content requirement yet, so two
        // empty columns are evidence of a rename. This pins the specified
        // behavior; the step that adds that requirement changes this test.
        assert_eq!(renames(&infer(&old, &new)), [(1, 1)]);
    }
}
