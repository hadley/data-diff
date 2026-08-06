//! Identify a dropped and an added column as one renamed column.

use std::collections::{HashMap, HashSet};

use crate::maps::DigestMap;

use arrow_array::RecordBatch;
use arrow_schema::DataType;

use crate::agreement::{Aligned, Meter, RowSample};
use crate::compare::ComparisonPlan;
use crate::rows::RowMatches;
use crate::schema::ColumnMap;
use crate::{IdentityBasis, Side};

/// Pair provisional drops with provisional adds that hold the same values.
///
/// Only the one-to-one matched rows are evidence: added, dropped, and
/// fanned-out rows are outside the alignment and take no part. With no matched
/// rows there is nothing to compare, so every candidate keeps its provisional
/// classification.
///
/// A `col_drop` or `col_add` hint reserves its endpoint in the map, and a
/// reserved endpoint is exactly a provisional drop or addition that must not be
/// paired: the user has said the column has no partner. Excluding it here is
/// both what the instruction means and the performance argument the design
/// makes for these two kinds, candidates being compared pairwise.
///
/// `budget` bounds the value-level pair examinations both stages may spend,
/// and the return value says whether they finished: `false` means the budget
/// exhausted, some candidates were never examined, and the endpoints they
/// would have resolved remain drops and additions.
pub(crate) fn infer(
    old: &RecordBatch,
    new: &RecordBatch,
    map: &mut ColumnMap,
    rows: &RowMatches,
    sample: &RowSample,
    budget: usize,
) -> bool {
    infer_with(
        old,
        new,
        map,
        rows,
        &mut Aligned::new(old, new, rows, sample),
        budget,
    )
}

/// Exact pairs first, then approximate ones among what is left.
///
/// That order is the design's and is also a real dependency: both stages draw
/// from the same two candidate lists, so a pair that agrees everywhere would
/// otherwise be settled by whichever rule reached it first. One meter spans
/// both stages, so the budget is a fact about rename inference rather than
/// about either half of it.
fn infer_with(
    old: &RecordBatch,
    new: &RecordBatch,
    map: &mut ColumnMap,
    rows: &RowMatches,
    values: &mut Aligned,
    budget: usize,
) -> bool {
    if rows.matched.is_empty() {
        return true;
    }
    let mut meter = Meter::new(budget);
    let (exact, exact_complete) = exact_pairs(old, new, map, values, &mut meter);
    apply(map, exact, IdentityBasis::Exact);
    let (approximate, approximate_complete) = approximate_pairs(old, new, map, values, &mut meter);
    apply(map, approximate, IdentityBasis::Approximate);
    exact_complete && approximate_complete
}

/// The unresolved, unreserved endpoints, in column order.
///
/// Reserved endpoints are excluded here rather than filtered at each use, so a
/// hint that says a column has no partner keeps it out of every candidate
/// structure, and the ranks the positional pre-pass aligns are ranks among
/// real candidates.
fn candidates(map: &ColumnMap) -> (Vec<usize>, Vec<usize>) {
    let dropped = map
        .dropped()
        .into_iter()
        .filter(|&index| !map.reserved(Side::Old, index))
        .collect();
    let added = map
        .added()
        .into_iter()
        .filter(|&index| !map.reserved(Side::New, index))
        .collect();
    (dropped, added)
}

/// Pair candidates that agree in every matched row.
///
/// A renamed column usually keeps its position among its peers, so the
/// rank-aligned diagonal is examined first: a same-rank pair that agrees
/// exactly and is informative is claimed immediately, and a table where every
/// column was renamed in place resolves in one examination per column. The
/// pre-pass also settles what used to be settled by column order alone — an
/// informative exact tie now prefers the positionally corresponding candidate
/// before falling back to column order.
///
/// The general pass finds its candidates by a digest join rather than by
/// scanning the dropped × added matrix: added columns are grouped by digest,
/// one map per comparison plan, and each dropped column looks its own digest
/// up. Only genuinely digest-equal pairs are examined at value level, which is
/// what the meter counts.
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
/// other, so neither rank nor column order is resolving a tie between
/// indistinguishable answers but inventing one relationship out of many. Such
/// a pair is accepted only when it is the only exact match available to both
/// of its ends — a judgement that needs the whole verified matrix, so it is
/// never made after the meter exhausts.
fn exact_pairs(
    old: &RecordBatch,
    new: &RecordBatch,
    map: &ColumnMap,
    values: &mut Aligned,
    meter: &mut Meter,
) -> (Vec<(usize, usize)>, bool) {
    let (dropped, added) = candidates(map);
    let mut taken_old = vec![false; dropped.len()];
    let mut taken_new = vec![false; added.len()];
    let mut accepted = Vec::new();

    // The positional pre-pass: only the same-rank pair, claimed on the spot.
    for rank in 0..dropped.len().min(added.len()) {
        let (old_index, new_index) = (dropped[rank], added[rank]);
        let Some(plan) = plan_for(old, new, old_index, new_index) else {
            continue;
        };
        // A differing digest consumes nothing: the pair cannot agree.
        if values.digest(plan, Side::Old, old_index) != values.digest(plan, Side::New, new_index) {
            continue;
        }
        let Some(equal) = values.verify(meter, plan, old_index, new_index) else {
            return (accepted, false);
        };
        if !equal {
            continue;
        }
        let Some(agreement) = values.measure_full(meter, plan, old_index, new_index) else {
            return (accepted, false);
        };
        if !agreement.informative() {
            continue;
        }
        taken_old[rank] = true;
        taken_new[rank] = true;
        accepted.push((old_index, new_index));
    }

    // The digest join over what the pre-pass left.
    let mut join = DigestJoin::new(new, &added);
    let mut matching = Vec::with_capacity(dropped.len());
    let mut complete = true;
    'drops: for (rank, &old_index) in dropped.iter().enumerate() {
        if taken_old[rank] {
            matching.push(Vec::new());
            continue;
        }
        let mut verified = Vec::new();
        for (position, plan) in join.matches(old, values, old_index, &taken_new) {
            let Some(equal) = values.verify(meter, plan, old_index, added[position]) else {
                // Every drop from here on is stranded: its candidates were
                // never examined, so it stays a drop and its adds stay adds.
                complete = false;
                break 'drops;
            };
            if equal {
                verified.push(position);
            }
        }
        matching.push(verified);
    }

    // Ambiguity is judged against the whole verified matrix, which after an
    // exhaustion no longer exists: the claims below would undercount, so the
    // unambiguous shortcut is only trusted when verification finished.
    let mut claims = vec![0_usize; added.len()];
    for position in matching.iter().flatten() {
        claims[*position] += 1;
    }

    let mut claimed = taken_new;
    for (rank, positions) in matching.iter().enumerate() {
        let old_index = dropped[rank];
        for &position in positions {
            if claimed[position] {
                continue;
            }
            let new_index = added[position];
            let plan =
                plan_for(old, new, old_index, new_index).expect("a match implies a plan exists");
            let unambiguous = complete && positions.len() == 1 && claims[position] == 1;
            if !unambiguous {
                let Some(agreement) = values.measure_full(meter, plan, old_index, new_index) else {
                    return (accepted, false);
                };
                if !agreement.informative() {
                    continue;
                }
            }
            claimed[position] = true;
            accepted.push((old_index, new_index));
            break;
        }
    }
    (accepted, complete)
}

/// Added columns grouped by digest, one map per comparison plan.
///
/// The join is built lazily: a plan's bucket fills the first time a drop needs
/// it, one type group at a time, so a plan no drop asks about costs nothing.
/// Projection construction behind the digests is cached per (column, plan) —
/// the amortized linear pass the design accepts — and the buckets are only
/// ever looked up by digest, never iterated, so hash order decides nothing.
struct DigestJoin {
    /// Distinct added data types, each with its `(position, column)` pairs in
    /// add order: the position indexes the added list, the column the table.
    groups: Vec<(DataType, Vec<(usize, usize)>)>,
    buckets: HashMap<ComparisonPlan, DigestMap<Vec<usize>>>,
    folded: HashSet<(ComparisonPlan, usize)>,
}

impl DigestJoin {
    fn new(new: &RecordBatch, added: &[usize]) -> Self {
        let mut groups: Vec<(DataType, Vec<(usize, usize)>)> = Vec::new();
        for (position, &new_index) in added.iter().enumerate() {
            let data_type = new.column(new_index).data_type();
            match groups.iter_mut().find(|(held, _)| held == data_type) {
                Some((_, members)) => members.push((position, new_index)),
                None => groups.push((data_type.clone(), vec![(position, new_index)])),
            }
        }
        Self {
            groups,
            buckets: HashMap::new(),
            folded: HashSet::new(),
        }
    }

    /// The added-list positions whose digest equals the drop's, with the plan
    /// each was digested under, ascending by position.
    ///
    /// Two type groups can share one plan — the plan is a function of the
    /// normalized kinds, not the source types — so a bucket folds in every
    /// group that reaches it, each exactly once.
    fn matches(
        &mut self,
        old: &RecordBatch,
        values: &mut Aligned,
        old_index: usize,
        taken: &[bool],
    ) -> Vec<(usize, ComparisonPlan)> {
        let old_type = old.column(old_index).data_type();
        let mut found = Vec::new();
        for (group, (new_type, members)) in self.groups.iter().enumerate() {
            let Some(plan) = ComparisonPlan::new(old_type, new_type) else {
                continue;
            };
            if self.folded.insert((plan, group)) {
                let bucket = self.buckets.entry(plan).or_default();
                for &(position, new_index) in members {
                    bucket
                        .entry(values.digest(plan, Side::New, new_index))
                        .or_default()
                        .push(position);
                }
            }
            let digest = values.digest(plan, Side::Old, old_index);
            if let Some(positions) = self.buckets[&plan].get(&digest) {
                found.extend(positions.iter().map(|&position| (position, plan)));
            }
        }
        found.sort_unstable_by_key(|&(position, _)| position);
        found.retain(|&(position, _)| !taken[position]);
        found
    }
}

/// Pair candidates that agree closely enough, and by more than chance.
///
/// Acceptance is mutual uniqueness rather than first match. Ambiguous *exact*
/// matches are equally good, so choosing the first is choosing arbitrarily
/// between answers that cannot be told apart; ambiguous approximate matches
/// differ in how well they match, and picking the first would be choosing
/// against evidence this step has deliberately not weighed. Overlapping
/// candidates stay drops and additions for the user to resolve.
///
/// The work proceeds in endpoint groups, drop by drop in column order: first
/// the drop's full row of candidates, then, where exactly one qualifies, the
/// qualifying add's full column for the mutual-uniqueness check. A pair is
/// accepted only after every candidate incident to both of its endpoints has
/// been measured, so when the meter exhausts, the drop it stopped in and every
/// later one are stranded while the groups already completed stay accepted.
fn approximate_pairs(
    old: &RecordBatch,
    new: &RecordBatch,
    map: &ColumnMap,
    values: &mut Aligned,
    meter: &mut Meter,
) -> (Vec<(usize, usize)>, bool) {
    let (dropped, added) = candidates(map);
    let mut accepted = Vec::new();
    for &old_index in &dropped {
        // The drop's full row of candidates.
        let mut qualifying = Vec::new();
        for (position, &new_index) in added.iter().enumerate() {
            let Some(plan) = plan_for(old, new, old_index, new_index) else {
                continue;
            };
            let Some(agreement) = values.measure_sampled(meter, plan, old_index, new_index) else {
                return (accepted, false);
            };
            if agreement.is_close() {
                qualifying.push(position);
            }
        }
        let [position] = qualifying[..] else {
            continue;
        };
        let new_index = added[position];
        // The qualifying add's full column: mutual uniqueness is evidence
        // about every candidate incident to the add, so all of them are
        // measured rather than stopping at the first competitor.
        let mut unique = true;
        for &other_index in &dropped {
            if other_index == old_index {
                continue;
            }
            let Some(plan) = plan_for(old, new, other_index, new_index) else {
                continue;
            };
            let Some(agreement) = values.measure_sampled(meter, plan, other_index, new_index)
            else {
                return (accepted, false);
            };
            if agreement.is_close() {
                unique = false;
            }
        }
        if unique {
            accepted.push((old_index, new_index));
        }
    }
    (accepted, true)
}

/// Claim the accepted pairs, which is the whole of applying them.
///
/// Both endpoints leave the drops and the additions by being paired, those
/// being derived rather than maintained, and claiming inserts in old-position
/// order, so nothing has to put the identities back in the order `detect_order`
/// requires of them.
fn apply(map: &mut ColumnMap, accepted: Vec<(usize, usize)>, basis: IdentityBasis) {
    for (old_index, new_index) in accepted {
        map.claim(old_index, new_index, basis);
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
    use crate::IdentityBasis;
    use crate::agreement::{Aligned, RowSample};
    use crate::compare::CanonicalValue;
    use crate::key::testing::resolve_key;
    use crate::rows::match_rows;
    use crate::schema::ColumnMap;
    use crate::schema::testing::reconcile_schema;

    fn infer_renames(old: &RecordBatch, new: &RecordBatch) -> ColumnMap {
        let (schema, complete) = infer_with_budget(old, new, usize::MAX);
        assert!(complete);
        schema
    }

    fn infer_with_budget(old: &RecordBatch, new: &RecordBatch, budget: usize) -> (ColumnMap, bool) {
        let options = DiffOptions {
            key: vec!["id".into()],
            ..DiffOptions::default()
        };
        let key = resolve_key(old, new, &options).unwrap();
        let rows = match_rows(&key);
        let mut schema = reconcile_schema(old, new, &key);
        let sample = RowSample::full();
        let complete = infer(old, new, &mut schema, &rows, &sample, budget);
        (schema, complete)
    }

    fn renames(schema: &ColumnMap) -> Vec<(usize, usize)> {
        schema
            .pairs()
            .iter()
            .filter(|pair| !pair.is_key)
            .map(|pair| (pair.old, pair.new))
            .collect()
    }

    fn basis(schema: &ColumnMap, old: usize) -> IdentityBasis {
        schema
            .pairs()
            .iter()
            .find(|pair| pair.old == old)
            .expect("the identity exists")
            .basis
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
        assert_eq!(basis(&schema, 1), IdentityBasis::Exact);
        assert!(schema.dropped().is_empty());
        assert!(schema.added().is_empty());
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
        assert_eq!(basis(&schema, 1), IdentityBasis::Exact);
    }

    #[test]
    fn a_dropped_boolean_relates_to_its_integer_encoding() {
        let old = table! {
            "id" => [1, 2],
            "flag" => [true, false],
        };
        let new = table! {
            "id" => [1, 2],
            "count" => [1, 0],
        };

        // These candidates were once incomparable and stayed a drop and an
        // addition. Booleans now compare in the numeric domains, so the 0/1
        // encoding is exact evidence like any other.
        let schema = infer_renames(&old, &new);

        assert_eq!(renames(&schema), [(1, 1)]);
        assert_eq!(basis(&schema, 1), IdentityBasis::Exact);
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
        assert_eq!(schema.dropped(), [1]);
        assert_eq!(schema.added(), [1]);
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
        assert_eq!(schema.dropped(), [1]);
        assert_eq!(schema.added(), [1]);
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
        // position — here the ranks align, so this is also column order.
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
            ..DiffOptions::default()
        };
        let key = resolve_key(&old, &new, &options).unwrap();
        let rows = match_rows(&key);
        let mut schema = reconcile_schema(&old, &new, &key);
        let sample = RowSample::full();
        let mut values = Aligned::with_digest(&old, &new, &rows, &sample, |_: &[CanonicalValue]| 0);
        assert!(infer_with(
            &old,
            &new,
            &mut schema,
            &rows,
            &mut values,
            usize::MAX
        ));

        // Every column now digests alike, so the join offers every pair and
        // only the elementwise verification separates the real rename from
        // the unrelated one.
        assert_eq!(renames(&schema), [(2, 2)]);
        assert_eq!(schema.dropped(), [1]);
        assert_eq!(schema.added(), [1]);
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

        // Every candidate matches every other, so neither rank alignment nor
        // column order would be resolving a tie between indistinguishable
        // answers: it would be inventing two relationships out of four equally
        // good ones. The positional pre-pass claims informative pairs only,
        // so these fall through to the mutual-uniqueness rule and stay put.
        let schema = infer_renames(&old, &new);

        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped(), [1, 2]);
        assert_eq!(schema.added(), [1, 2]);
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
        assert_eq!(schema.dropped(), [1]);
        assert_eq!(schema.added(), [1, 2]);
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
    fn an_informative_tie_prefers_the_positionally_corresponding_candidate() {
        let old = table! {
            "id" => [1, 2],
            "s" => ["only", "old"],
            "b" => [10, 20],
        };
        let new = table! {
            "id" => [1, 2],
            "x" => [10, 20],
            "y" => [10, 20],
            "t" => ["never", "matches"],
        };

        // "b" ties exactly with "x" and "y". Its rank among the drops is 1,
        // and rank 1 among the adds is "y", so the diagonal claims (b, y)
        // where column order alone would have taken "x". The refined
        // tie-break is positional correspondence first, column order after.
        let schema = infer_renames(&old, &new);

        assert_eq!(renames(&schema), [(2, 2)]);
        assert_eq!(schema.dropped(), [1]);
        assert_eq!(schema.added(), [1, 3]);
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
        // The two stages stay distinguishable in the result rather than only in
        // the code: this pair is one column on weaker evidence than the last.
        assert_eq!(basis(&schema, 1), IdentityBasis::Approximate);
        assert!(schema.dropped().is_empty());
        assert!(schema.added().is_empty());
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
        assert_eq!(schema.dropped(), [1]);
        assert_eq!(schema.added(), [1]);
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

        // Both old columns qualify with the one new column, so the mutual
        // uniqueness check fails from the add's end and nothing is accepted.
        let schema = infer_renames(&old, &new);

        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped(), [1, 2]);
        assert_eq!(schema.added(), [1]);
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

        // The mirror of the case above, and a separate path: the drop's own
        // row of candidates holds two qualifiers, so no column check runs.
        let schema = infer_renames(&old, &new);

        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped(), [1]);
        assert_eq!(schema.added(), [1, 2]);
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
        assert_eq!(schema.added(), [1]);
    }

    #[test]
    fn an_exhausted_budget_strands_endpoints_and_keeps_what_finished() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "a" => [-1, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
            "b" => [-1, 21, 31, 41, 51, 61, 71, 81, 91, 101, 111],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "x" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
            "y" => [11, 21, 31, 41, 51, 61, 71, 81, 91, 101, 111],
        };

        // Unbounded, both pairs are approximate renames: (a, x) and (b, y).
        let (schema, complete) = infer_with_budget(&old, &new, usize::MAX);
        assert!(complete);
        assert_eq!(renames(&schema), [(1, 1), (2, 2)]);

        // Two units fund a's row of candidates but not the mutual-uniqueness
        // column, so the group exhausts mid-way: nothing is accepted and both
        // endpoints strand as the drop and addition they were.
        let (schema, complete) = infer_with_budget(&old, &new, 2);
        assert!(!complete);
        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped(), [1, 2]);
        assert_eq!(schema.added(), [1, 2]);

        // Three units complete a's whole endpoint group — its row and the one
        // fresh column measurement — so (a, x) is accepted before the budget
        // dies inside b's group, which strands b and y only.
        let (schema, complete) = infer_with_budget(&old, &new, 3);
        assert!(!complete);
        assert_eq!(renames(&schema), [(1, 1)]);
        assert_eq!(schema.dropped(), [2]);
        assert_eq!(schema.added(), [2]);
    }

    #[test]
    fn an_exhausted_budget_never_accepts_an_uninformative_pair() {
        let old = table! {
            "id" => [1, 2],
            "gone" => [true, true],
            "lost" => [10, 20],
        };
        let new = table! {
            "id" => [1, 2],
            "fresh" => [true, true],
            "found" => [30, 40],
        };

        // Unbounded, (gone, fresh) is the only exact match available to both
        // of its ends, so the constant pair is accepted.
        let (schema, complete) = infer_with_budget(&old, &new, usize::MAX);
        assert!(complete);
        assert_eq!(renames(&schema), [(1, 1)]);

        // One unit funds the diagonal verification of (gone, fresh) and dies
        // on its informativeness measurement. The join never runs, so the
        // whole verified matrix does not exist, and the uninformative pair is
        // not accepted on evidence that was never gathered.
        let (schema, complete) = infer_with_budget(&old, &new, 1);
        assert!(!complete);
        assert!(renames(&schema).is_empty());
        assert_eq!(schema.dropped(), [1, 2]);
        assert_eq!(schema.added(), [1, 2]);
    }

    #[test]
    fn the_bulk_rename_resolves_within_one_examination_pair_per_column() {
        let old = table! {
            "id" => [1, 2, 3],
            "a" => [10, 20, 30],
            "b" => ["u", "v", "w"],
            "c" => [1.5, 2.5, 3.5],
        };
        let new = table! {
            "id" => [1, 2, 3],
            "x" => [10, 20, 30],
            "y" => ["u", "v", "w"],
            "z" => [1.5, 2.5, 3.5],
        };

        // Every column renamed in place: the diagonal claims each pair with
        // one verification and one informativeness measurement, so a budget of
        // exactly two units per column completes the stage.
        let (schema, complete) = infer_with_budget(&old, &new, 6);
        assert!(complete);
        assert_eq!(renames(&schema), [(1, 1), (2, 2), (3, 3)]);
    }
}
