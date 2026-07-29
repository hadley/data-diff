//! Identify a dropped and an added column as one renamed column.

use arrow_array::RecordBatch;

use crate::agreement::Aligned;
use crate::compare::ComparisonPlan;
use crate::rows::RowMatches;
use crate::schema::{ColumnIdentity, SchemaMatches};

/// Pair provisional drops with provisional adds that hold the same values.
///
/// Only the one-to-one matched rows are evidence: added, dropped, and
/// fanned-out rows are outside the alignment and take no part. With no matched
/// rows there is nothing to compare, so every candidate keeps its provisional
/// classification.
pub(crate) fn infer(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &mut SchemaMatches,
    rows: &RowMatches,
) {
    infer_with(old, new, schema, rows, &mut Aligned::new(old, new, rows));
}

/// Exact pairs first, then approximate ones among what is left.
///
/// That order is the design's and is also a real dependency: both stages draw
/// from the same two candidate lists, so a pair that agrees everywhere would
/// otherwise be settled by whichever rule reached it first.
fn infer_with(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &mut SchemaMatches,
    rows: &RowMatches,
    values: &mut Aligned,
) {
    if rows.matched.is_empty() {
        return;
    }
    let exact = exact_pairs(old, new, schema, values);
    apply(old, new, schema, exact);
    let approximate = approximate_pairs(old, new, schema, values);
    apply(old, new, schema, approximate);
    // Inferred identities are found in candidate order, while `detect_order`
    // requires the whole list to ascend by old position.
    schema.identities.sort_by_key(|identity| identity.old);
}

/// Pair candidates that agree in every matched row.
///
/// Candidates are walked in column order on both sides, so where several
/// columns are exactly equal they pair off in that order rather than needing
/// an assignment algorithm to choose between equally good answers.
///
/// Agreeing everywhere is enough on its own, with no requirement that the
/// values distinguish rows. Two columns holding one repeated value do agree
/// without that agreement narrowing anything down, but reporting them as a
/// drop and an addition asserts two changes where one accounts for the
/// evidence, and the more parsimonious reading is the one the design prefers
/// wherever both fit. Chance correction belongs to imperfect matches, which
/// need to know how much of their agreement was luck; complete agreement does
/// not.
///
/// The exception is a pairing that could equally have gone elsewhere. When
/// values cannot distinguish candidates, every constant column matches every
/// other, so column order is not resolving a tie between indistinguishable
/// answers but inventing one relationship out of many. Such a pair is accepted
/// only when it is the only exact match available to both of its ends.
fn exact_pairs(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &SchemaMatches,
    values: &mut Aligned,
) -> Vec<(usize, usize)> {
    // Every exactly agreeing pair, collected before any of them is taken, so
    // that ambiguity is judged against the whole candidate set.
    let matching = schema
        .dropped
        .iter()
        .map(|&old_index| {
            schema
                .added
                .iter()
                .enumerate()
                .filter(|&(_, &new_index)| {
                    plan_for(old, new, old_index, new_index)
                        .is_some_and(|plan| values.agree(plan, old_index, new_index))
                })
                .map(|(position, _)| position)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let mut claims = vec![0_usize; schema.added.len()];
    for position in matching.iter().flatten() {
        claims[*position] += 1;
    }

    let mut claimed = vec![false; schema.added.len()];
    let mut accepted = Vec::new();
    for (index, candidates) in matching.iter().enumerate() {
        let old_index = schema.dropped[index];
        for &position in candidates {
            if claimed[position] {
                continue;
            }
            let new_index = schema.added[position];
            let plan =
                plan_for(old, new, old_index, new_index).expect("a match implies a plan exists");
            let unambiguous = candidates.len() == 1 && claims[position] == 1;
            if !unambiguous && !values.measure(plan, old_index, new_index).informative() {
                continue;
            }
            claimed[position] = true;
            accepted.push((old_index, new_index));
            break;
        }
    }
    accepted
}

/// Pair candidates that agree closely enough, and by more than chance.
///
/// Acceptance is mutual uniqueness rather than first match. Ambiguous *exact*
/// matches are equally good, so choosing the first is choosing arbitrarily
/// between answers that cannot be told apart; ambiguous approximate matches
/// differ in how well they match, and picking the first would be choosing
/// against evidence this step has deliberately not weighed. Overlapping
/// candidates stay drops and additions for the user to resolve.
fn approximate_pairs(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &SchemaMatches,
    values: &mut Aligned,
) -> Vec<(usize, usize)> {
    let qualifying = schema
        .dropped
        .iter()
        .map(|&old_index| {
            schema
                .added
                .iter()
                .enumerate()
                .filter(|&(_, &new_index)| {
                    plan_for(old, new, old_index, new_index)
                        .is_some_and(|plan| values.measure(plan, old_index, new_index).is_close())
                })
                .map(|(position, _)| position)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let mut claims = vec![0_usize; schema.added.len()];
    for position in qualifying.iter().flatten() {
        claims[*position] += 1;
    }

    qualifying
        .iter()
        .enumerate()
        .filter_map(|(index, matches)| {
            let [position] = matches[..] else {
                return None;
            };
            (claims[position] == 1).then(|| (schema.dropped[index], schema.added[position]))
        })
        .collect()
}

/// Turn accepted pairs into identities, removing both of their endpoints.
fn apply(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &mut SchemaMatches,
    accepted: Vec<(usize, usize)>,
) {
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
}

fn plan_for(
    old: &RecordBatch,
    new: &RecordBatch,
    old_index: usize,
    new_index: usize,
) -> Option<ComparisonPlan> {
    ComparisonPlan::new(
        old.column(old_index).data_type(),
        new.column(new_index).data_type(),
    )
}

#[cfg(test)]
mod tests {
    use arrow_array::RecordBatch;
    use test_support::table;

    use super::{infer, infer_with};
    use crate::DiffOptions;
    use crate::agreement::Aligned;
    use crate::compare::CanonicalValue;
    use crate::key::testing::resolve_key;
    use crate::rows::match_rows;
    use crate::schema::SchemaMatches;
    use crate::schema::testing::reconcile_schema;

    fn infer_renames(old: &RecordBatch, new: &RecordBatch) -> SchemaMatches {
        let options = DiffOptions {
            key: vec!["id".into()],
            hints: Vec::new(),
        };
        let key = resolve_key(old, new, &options).unwrap();
        let rows = match_rows(&key);
        let mut schema = reconcile_schema(old, new, &key).unwrap();
        infer(old, new, &mut schema, &rows);
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

        let schema = infer_renames(&old, &new);

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

        let schema = infer_renames(&old, &new);

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

        let schema = infer_renames(&old, &new);

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

        let schema = infer_renames(&old, &new);

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

        let schema = infer_renames(&old, &new);

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
        assert_eq!(renames(&infer_renames(&old, &new)), [(1, 1), (2, 2)]);
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
        assert_eq!(renames(&infer_renames(&old, &new)), [(1, 1)]);
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
        assert_eq!(renames(&infer_renames(&old, &new)), [(1, 1)]);
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
            hints: Vec::new(),
        };
        let key = resolve_key(&old, &new, &options).unwrap();
        let rows = match_rows(&key);
        let mut schema = reconcile_schema(&old, &new, &key).unwrap();
        let mut values = Aligned::with_digest(&old, &new, &rows, |_: &[CanonicalValue]| 0);
        infer_with(&old, &new, &mut schema, &rows, &mut values);

        // Every column now digests alike, so only the elementwise comparison
        // separates the real rename from the unrelated pair.
        assert_eq!(renames(&schema), [(2, 2)]);
        assert_eq!(schema.dropped, [1]);
        assert_eq!(schema.added, [1]);
    }

    #[test]
    fn a_constant_column_with_one_candidate_is_still_a_rename() {
        let old = table! {
            "id" => [1, 2, 3],
            "gone" => [true, true, true],
        };
        let new = table! {
            "id" => [1, 2, 3],
            "fresh" => [true, true, true],
        };

        // These values narrow nothing down, but nothing competes for the
        // pairing either, and one rename accounts for the evidence that two
        // separate operations would spend twice as much to describe.
        assert_eq!(renames(&infer_renames(&old, &new)), [(1, 1)]);
    }

    #[test]
    fn columns_of_nulls_are_no_different_from_any_other_constant() {
        let old = table! {
            "id" => [1, 2],
            "gone" => [None::<i64>, None],
        };
        let new = table! {
            "id" => [1, 2],
            "fresh" => [None::<i64>, None],
        };

        assert_eq!(renames(&infer_renames(&old, &new)), [(1, 1)]);
    }

    #[test]
    fn constant_columns_with_competing_candidates_stay_unresolved() {
        let old = table! {
            "id" => [1, 2],
            "a" => [true, true],
            "b" => [true, true],
        };
        let new = table! {
            "id" => [1, 2],
            "x" => [true, true],
            "y" => [true, true],
        };

        // Every candidate matches every other, so column order would not be
        // resolving a tie between indistinguishable answers, it would be
        // inventing two relationships out of four equally good ones.
        let schema = infer_renames(&old, &new);

        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped, [1, 2]);
        assert_eq!(schema.added, [1, 2]);
    }

    #[test]
    fn one_constant_column_facing_two_candidates_stays_unresolved() {
        let old = table! {
            "id" => [1, 2],
            "gone" => ["ok", "ok"],
        };
        let new = table! {
            "id" => [1, 2],
            "x" => ["ok", "ok"],
            "y" => ["ok", "ok"],
        };

        // Ambiguity is judged from both ends, as it is for approximate pairs.
        let schema = infer_renames(&old, &new);

        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped, [1]);
        assert_eq!(schema.added, [1, 2]);
    }

    #[test]
    fn informative_candidates_still_pair_off_in_column_order_when_tied() {
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

        // The distinction is not "tied" against "unique" but whether the
        // values could have told the candidates apart. Here they vary, and
        // agree anyway, so the tie is between equally complete answers.
        assert_eq!(renames(&infer_renames(&old, &new)), [(1, 1), (2, 2)]);
    }

    #[test]
    fn a_close_but_imperfect_pair_is_still_one_column() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "amount" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "total" => [-1, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        };

        // Ten of eleven rows agree, which is more than nine in ten and far
        // more than chance would give distinct values.
        let schema = infer_renames(&old, &new);

        assert_eq!(renames(&schema), [(1, 1)]);
        assert!(schema.dropped.is_empty());
        assert!(schema.added.is_empty());
    }

    #[test]
    fn one_disagreement_too_many_leaves_a_drop_and_an_addition() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "amount" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "total" => [-1, -2, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        };

        let schema = infer_renames(&old, &new);

        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped, [1]);
        assert_eq!(schema.added, [1]);
    }

    #[test]
    fn nine_in_ten_is_not_more_than_nine_in_ten() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "amount" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "total" => [-1, 20, 30, 40, 50, 60, 70, 80, 90, 100],
        };

        // The same single disagreement is accepted over eleven rows by the
        // test above. The threshold being strict is what gives approximate
        // inference an implicit floor of eleven rows in place of a row
        // minimum: below that, an imperfect pair cannot clear it at all.
        assert!(renames(&infer_renames(&old, &new)).is_empty());
    }

    #[test]
    fn agreement_no_better_than_chance_is_not_evidence() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "gone" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "fresh" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2],
        };

        // Ten of eleven rows agree, which clears the observed threshold, but
        // one value fills nearly the whole column on both sides, so chance
        // explains the agreement and the corrected figure rejects the pair.
        assert!(renames(&infer_renames(&old, &new)).is_empty());
    }

    #[test]
    fn two_old_columns_matching_one_new_column_stay_unresolved() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "a" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
            "b" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "x" => [-1, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        };

        // Both old columns qualify with the one new column, so the new
        // endpoint is claimed twice and nothing is accepted.
        let schema = infer_renames(&old, &new);

        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped, [1, 2]);
        assert_eq!(schema.added, [1]);
    }

    #[test]
    fn one_old_column_matching_two_new_columns_stays_unresolved() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "a" => [-1, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "x" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
            "y" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        };

        // The mirror of the case above, and a separate path: candidates are
        // enumerated by walking old columns and scanning new ones, so this
        // counts the old endpoint's matches rather than the new one's.
        let schema = infer_renames(&old, &new);

        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped, [1]);
        assert_eq!(schema.added, [1, 2]);
    }

    #[test]
    fn an_exact_pair_is_taken_before_a_close_one() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "amount" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "close" => [-1, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
            "total" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        };

        // Both new columns are candidates for "amount", and the exact one wins
        // because exact inference runs first and takes the pair out; leaving
        // both to the approximate stage would make this ambiguous instead.
        let schema = infer_renames(&old, &new);

        assert_eq!(renames(&schema), [(1, 2)]);
        assert_eq!(schema.added, [1]);
    }
}
