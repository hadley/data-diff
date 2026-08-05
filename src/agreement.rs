//! How far two columns agree over the rows their tables have in common.
//!
//! Rename and swap inference ask the same question of a candidate pair, so the
//! measurement lives here rather than in either of them. So do the two devices
//! that keep the asking bounded: the [`Meter`] that counts first-time pair
//! examinations against a budget, and the [`RowSample`] that caps how many
//! matched rows an approximate measurement reads.

use std::collections::HashMap;

use arrow_array::RecordBatch;

use crate::Side;
use crate::compare::{CanonicalValue, ComparisonPlan, sequence_hash};
use crate::key::ResolvedKey;
use crate::rows::RowMatches;

/// The share of aligned rows a pair must agree in to be one column.
///
/// Being strict, this also fixes the smallest table an imperfect pair can be
/// identified in: nine of ten rows is not *more* than nine in ten, so eleven
/// aligned rows are needed before a single disagreement can be tolerated. A
/// pair agreeing in every row clears it at any size. There is no separate row
/// minimum, and `design.md` records why one was declined.
pub(crate) const MIN_AGREEMENT_PERCENT: u64 = 90;

/// The chance-corrected agreement a pair must reach as well.
pub(crate) const MIN_KAPPA_PERCENT: u64 = 80;

/// The share of aligned rows a column must fall short of to read as rewritten.
pub(crate) const MAX_EDITED_AGREEMENT_PERCENT: u64 = 50;

/// What two aligned columns agree about, as counts rather than proportions.
///
/// Counts are what makes the design's proportions reproducible. Expected
/// agreement is a sum over a frequency map, and floating-point addition is not
/// associative, so accumulating it in hash order would let its last bit follow
/// the iteration order of a `HashMap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Agreement {
    /// Rows compared, the $n$ the proportions are taken over.
    rows: u64,
    /// Rows whose two values are equal, so that $p_o$ is `agreeing / rows`.
    agreeing: u64,
    /// $\sum_v c_{old}(v) \, c_{new}(v)$, so that $p_e$ is this over $n^2$.
    ///
    /// Wider than the row counts because it grows as $n^2$ and is compared
    /// against $n^2$, which would exhaust `u64` at four billion rows.
    expected: u128,
}

impl Agreement {
    /// Whether the pair agrees closely enough, and by enough more than chance,
    /// to be read as one column.
    ///
    /// This is the bar for an *imperfect* match. Exact inference does not apply
    /// it, complete agreement having no element of chance to correct for.
    pub(crate) fn is_close(&self) -> bool {
        self.informative()
            && self.observed_above(MIN_AGREEMENT_PERCENT)
            && self.kappa_above(MIN_KAPPA_PERCENT)
    }

    /// Whether the pair agrees in so few rows that it reads as rewritten.
    ///
    /// Not the negation of [`Agreement::is_close`]: the gap between them is
    /// deliberate, a pair agreeing in most rows but not nearly all of them
    /// being evidence of nothing in particular. With nothing compared, neither
    /// side rises above zero and neither predicate holds.
    pub(crate) fn is_distant(&self) -> bool {
        self.agreeing * 100 < self.rows * MAX_EDITED_AGREEMENT_PERCENT
    }

    /// Whether the values distinguish rows at all, which is $p_e < 1$.
    ///
    /// Expected agreement reaches one exactly when both columns hold a single
    /// value in every aligned row — two all-null columns being the case worth
    /// naming. Such values narrow nothing down, so they cannot tell one
    /// candidate from another, and leave $\kappa$ undefined. Exact inference,
    /// needing no $\kappa$, consults this only to decide whether a pairing was
    /// arbitrary.
    pub(crate) fn informative(&self) -> bool {
        self.expected < self.pairs()
    }

    /// $p_o > \text{percent} / 100$, cleared of its denominator.
    fn observed_above(&self, percent: u64) -> bool {
        self.agreeing * 100 > self.rows * percent
    }

    /// Whether $\kappa$, the agreement left once chance is discounted, exceeds
    /// `percent`.
    ///
    /// $\kappa = (p_o - p_e) / (1 - p_e)$ measures observed agreement against
    /// the agreement two unrelated columns would have stumbled into anyway,
    /// given how often each of their values occurs. It reads 1 when every row
    /// agrees and 0 when the rows agree no more often than those frequencies
    /// predict, so a pair of nearly constant columns cannot look identified
    /// merely by coinciding almost everywhere. Undefined at $p_e = 1$, where
    /// [`Self::informative`] must have ruled the pair out already.
    ///
    /// Clearing the denominators gives $100 (mn - S) > k (n^2 - S)$; moving
    /// both differences across leaves every term non-negative, keeping the
    /// comparison in unsigned arithmetic.
    fn kappa_above(&self, percent: u64) -> bool {
        let observed = u128::from(self.agreeing) * u128::from(self.rows);
        let percent = u128::from(percent);
        100 * observed + percent * self.expected > percent * self.pairs() + 100 * self.expected
    }

    /// $n^2$, the number of ordered row pairs the expected count ranges over.
    fn pairs(&self) -> u128 {
        u128::from(self.rows) * u128::from(self.rows)
    }
}

/// A counted budget of first-time pair examinations.
///
/// Budgets are counts of deterministic work units. A memoized answer costs
/// nothing; only a first-time examination charges, so a unit is spent exactly
/// once per distinct question.
pub(crate) struct Meter {
    remaining: usize,
}

impl Meter {
    pub(crate) fn new(units: usize) -> Self {
        Self { remaining: units }
    }

    /// A meter for work the design deliberately leaves unbudgeted, such as
    /// swap inference's linear rewritten filter.
    pub(crate) fn unlimited() -> Self {
        Self::new(usize::MAX)
    }

    fn charge(&mut self) -> bool {
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        true
    }
}

/// The matched rows approximate measurement reads.
///
/// Above the `agreement_rows` budget, the sample is the matched pairs with the
/// smallest hashes of their canonicalized key values, ties broken by position
/// in the matching — which is old-row order, so the tie-break is by old
/// position. Hashing the key rather than the position keeps the selection a
/// pure function of the input, and means the same logical rows are sampled
/// across reconsideration's two passes when the key survives. At or below the
/// budget the sample is every matched row and nothing changes.
pub(crate) struct RowSample(Option<Vec<usize>>);

impl RowSample {
    /// Every matched row, for tests that want no cap in play.
    ///
    /// Production callers reach the same state through [`Self::select`] when
    /// the matched rows fit the budget, which is why only tests name this.
    #[cfg(test)]
    pub(crate) fn full() -> Self {
        Self(None)
    }

    pub(crate) fn select(key: &ResolvedKey, rows: &RowMatches, cap: usize) -> Self {
        Self::with_hash(key, rows, cap, sequence_hash)
    }

    /// The hash is injectable so a test can force every key to collide, which
    /// is the only way to reach the tie-break.
    pub(crate) fn with_hash(
        key: &ResolvedKey,
        rows: &RowMatches,
        cap: usize,
        hash: fn(&[CanonicalValue]) -> u128,
    ) -> Self {
        if rows.matched.len() <= cap {
            return Self(None);
        }
        let mut ranked = rows
            .matched
            .iter()
            .enumerate()
            .map(|(position, &(old_row, _))| (hash(&key.old[old_row]), position))
            .collect::<Vec<_>>();
        ranked.sort_unstable();
        let mut positions = ranked[..cap]
            .iter()
            .map(|&(_, position)| position)
            .collect::<Vec<_>>();
        // Measurement walks the sample in matching order, not in hash order,
        // so the values it reads stay ordered however the hashes fell.
        positions.sort_unstable();
        Self(Some(positions))
    }

    fn is_full(&self) -> bool {
        self.0.is_none()
    }
}

/// Which rows a memoized measurement was taken over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Over {
    Full,
    Sampled,
}

/// Candidate columns projected onto the matched rows, measured on demand.
///
/// Canonicalization depends on the pair rather than on the column — a string
/// column keeps its bytes against another string column but is parsed into
/// integers against an integer one — so a projection is cached per column
/// *and* plan. A candidate takes part in few plans in practice, which keeps
/// this a small multiple of one pass per column rather than one pass per pair.
///
/// Every answer about a pair is memoized, and only a first-time computation
/// charges the caller's [`Meter`]: a unit of budget is one distinct pair
/// examination, however many times the stage asks the question.
pub(crate) struct Aligned<'a> {
    old: &'a RecordBatch,
    new: &'a RecordBatch,
    rows: &'a RowMatches,
    sample: &'a RowSample,
    digest: fn(&[CanonicalValue]) -> u128,
    cache: HashMap<(Side, usize, ComparisonPlan), Projection>,
    verified: HashMap<(usize, usize, ComparisonPlan), bool>,
    measured: HashMap<(Over, usize, usize, ComparisonPlan), Agreement>,
}

/// One column's values over the matched rows, with what can be read off them.
struct Projection {
    digest: u128,
    values: Vec<CanonicalValue>,
    counts: HashMap<CanonicalValue, u64>,
}

impl<'a> Aligned<'a> {
    pub(crate) fn new(
        old: &'a RecordBatch,
        new: &'a RecordBatch,
        rows: &'a RowMatches,
        sample: &'a RowSample,
    ) -> Self {
        Self::with_digest(old, new, rows, sample, sequence_hash)
    }

    /// The digest is injectable so a test can force every column to collide,
    /// which is the only way to reach the verification behind it.
    pub(crate) fn with_digest(
        old: &'a RecordBatch,
        new: &'a RecordBatch,
        rows: &'a RowMatches,
        sample: &'a RowSample,
        digest: fn(&[CanonicalValue]) -> u128,
    ) -> Self {
        Self {
            old,
            new,
            rows,
            sample,
            digest,
            cache: HashMap::new(),
            verified: HashMap::new(),
            measured: HashMap::new(),
        }
    }

    /// The digest of one column's full projection under a plan.
    ///
    /// Building the projection behind it is the unbudgeted linear pass the
    /// design accepts for exact inference; digests only decide which pairs are
    /// worth comparing, so a collision cannot invent a rename.
    pub(crate) fn digest(&mut self, plan: ComparisonPlan, side: Side, index: usize) -> u128 {
        self.fill(plan, side, index);
        self.cache[&(side, index, plan)].digest
    }

    /// Whether the two columns hold equal values in every matched row.
    ///
    /// Memoized; a first-time comparison charges the meter, and `None` means
    /// the meter could not fund it, so the comparison never ran.
    pub(crate) fn verify(
        &mut self,
        meter: &mut Meter,
        plan: ComparisonPlan,
        old: usize,
        new: usize,
    ) -> Option<bool> {
        if let Some(&equal) = self.verified.get(&(old, new, plan)) {
            return Some(equal);
        }
        if !meter.charge() {
            return None;
        }
        self.fill(plan, Side::Old, old);
        self.fill(plan, Side::New, new);
        let old_column = &self.cache[&(Side::Old, old, plan)];
        let new_column = &self.cache[&(Side::New, new, plan)];
        let equal =
            old_column.digest == new_column.digest && old_column.values == new_column.values;
        self.verified.insert((old, new, plan), equal);
        Some(equal)
    }

    /// Count what the two columns agree about over every matched row.
    ///
    /// The exact stage's question: informativeness there is about what the
    /// full columns could distinguish, not what a sample happened to hold.
    pub(crate) fn measure_full(
        &mut self,
        meter: &mut Meter,
        plan: ComparisonPlan,
        old: usize,
        new: usize,
    ) -> Option<Agreement> {
        self.measure(meter, Over::Full, plan, old, new)
    }

    /// Count what the two columns agree about over the sampled rows.
    ///
    /// The approximate and swap stages' question. With a full sample this is
    /// the same measurement as [`Self::measure_full`] and shares its memo, so
    /// one distinct examination is never charged twice.
    pub(crate) fn measure_sampled(
        &mut self,
        meter: &mut Meter,
        plan: ComparisonPlan,
        old: usize,
        new: usize,
    ) -> Option<Agreement> {
        let over = if self.sample.is_full() {
            Over::Full
        } else {
            Over::Sampled
        };
        self.measure(meter, over, plan, old, new)
    }

    fn measure(
        &mut self,
        meter: &mut Meter,
        over: Over,
        plan: ComparisonPlan,
        old: usize,
        new: usize,
    ) -> Option<Agreement> {
        if let Some(&agreement) = self.measured.get(&(over, old, new, plan)) {
            return Some(agreement);
        }
        if !meter.charge() {
            return None;
        }
        self.fill(plan, Side::Old, old);
        self.fill(plan, Side::New, new);
        let old_column = &self.cache[&(Side::Old, old, plan)];
        let new_column = &self.cache[&(Side::New, new, plan)];
        let agreement = match over {
            Over::Full => Agreement {
                rows: old_column.values.len() as u64,
                agreeing: old_column
                    .values
                    .iter()
                    .zip(&new_column.values)
                    .filter(|(old, new)| old == new)
                    .count() as u64,
                expected: expected(&old_column.counts, &new_column.counts),
            },
            Over::Sampled => {
                let positions = self
                    .sample
                    .0
                    .as_deref()
                    .expect("a full sample measures as Over::Full");
                let mut old_counts: HashMap<&CanonicalValue, u64> = HashMap::new();
                let mut new_counts: HashMap<&CanonicalValue, u64> = HashMap::new();
                let mut agreeing = 0_u64;
                for &position in positions {
                    let old_value = &old_column.values[position];
                    let new_value = &new_column.values[position];
                    if old_value == new_value {
                        agreeing += 1;
                    }
                    *old_counts.entry(old_value).or_default() += 1;
                    *new_counts.entry(new_value).or_default() += 1;
                }
                Agreement {
                    rows: positions.len() as u64,
                    agreeing,
                    expected: expected(&old_counts, &new_counts),
                }
            }
        };
        self.measured.insert((over, old, new, plan), agreement);
        Some(agreement)
    }

    fn fill(&mut self, plan: ComparisonPlan, side: Side, index: usize) {
        if self.cache.contains_key(&(side, index, plan)) {
            return;
        }
        // `rows.matched` is ordered by old position and holds each pair's new
        // position alongside it, so projecting each side through its own half
        // of the pair puts both columns in one order without materializing
        // aligned tables.
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
        let mut counts: HashMap<CanonicalValue, u64> = HashMap::new();
        for value in &values {
            *counts.entry(value.clone()).or_default() += 1;
        }
        self.cache.insert(
            (side, index, plan),
            Projection {
                digest: (self.digest)(&values),
                values,
                counts,
            },
        );
    }
}

/// $\sum_v c_{old}(v) \, c_{new}(v)$ over the values the two sides share.
///
/// Each term is a product of two row counts, so it is widened before it is
/// formed rather than after. Integer addition is associative, so summing in
/// hash order stays deterministic.
fn expected<V: std::hash::Hash + Eq>(
    old_counts: &HashMap<V, u64>,
    new_counts: &HashMap<V, u64>,
) -> u128 {
    old_counts
        .iter()
        .filter_map(|(value, count)| {
            new_counts
                .get(value)
                .map(|shared| u128::from(*count) * u128::from(*shared))
        })
        .sum()
}

fn project(values: &[CanonicalValue], rows: impl Iterator<Item = usize>) -> Vec<CanonicalValue> {
    rows.map(|row| values[row].clone()).collect()
}

#[cfg(test)]
mod tests {
    use arrow_array::RecordBatch;
    use test_support::table;

    use super::{Agreement, Aligned, MIN_AGREEMENT_PERCENT, MIN_KAPPA_PERCENT, Meter, RowSample};
    use crate::DiffOptions;
    use crate::compare::{CanonicalValue, ComparisonPlan};
    use crate::key::testing::resolve_key;
    use crate::rows::{RowMatches, match_rows};

    fn matched(old: &RecordBatch, new: &RecordBatch) -> RowMatches {
        let options = DiffOptions {
            key: vec!["id".into()],
            ..DiffOptions::default()
        };
        let key = resolve_key(old, new, &options).unwrap();
        match_rows(&key)
    }

    /// Measure the second column of each table against the other, in full.
    fn measure(old: &RecordBatch, new: &RecordBatch) -> Agreement {
        let rows = matched(old, new);
        let sample = RowSample::full();
        let plan =
            ComparisonPlan::new(old.column(1).data_type(), new.column(1).data_type()).unwrap();
        Aligned::new(old, new, &rows, &sample)
            .measure_full(&mut Meter::unlimited(), plan, 1, 1)
            .expect("an unlimited meter funds every measurement")
    }

    #[test]
    fn counts_agreeing_rows_and_expected_agreement() {
        let old = table! {
            "id" => [1, 2, 3, 4],
            "value" => [10, 10, 20, 30],
        };
        let new = table! {
            "id" => [1, 2, 3, 4],
            "value" => [10, 10, 20, 99],
        };

        // 10 appears twice on each side and 20 once, so the expected numerator
        // is 2*2 + 1*1 = 5; 30 and 99 share nothing and contribute nothing.
        assert_eq!(
            measure(&old, &new),
            Agreement {
                rows: 4,
                agreeing: 3,
                expected: 5,
            }
        );
    }

    #[test]
    fn nulls_are_a_category_rather_than_an_absence() {
        let old = table! {
            "id" => [1, 2, 3, 4],
            "value" => [None, None, Some(10), Some(20)],
        };
        let new = table! {
            "id" => [1, 2, 3, 4],
            "value" => [None, Some(10), None, Some(20)],
        };

        // Null agrees with null and disagrees with a value, and it is counted
        // like any other category: nulls contribute 2*2 to the expected
        // numerator, and the 10s and 20s one each.
        assert_eq!(
            measure(&old, &new),
            Agreement {
                rows: 4,
                agreeing: 2,
                expected: 6,
            }
        );
    }

    #[test]
    fn rows_are_taken_from_the_alignment_rather_than_by_position() {
        let old = table! {
            "id" => [1, 2, 3],
            "value" => [10, 20, 30],
        };
        let new = table! {
            "id" => [3, 1, 2],
            "value" => [30, 10, 20],
        };

        // Every row agrees, which comparing the columns by position would miss
        // entirely: row 1 of the old column is row 2 of the new one.
        assert_eq!(
            measure(&old, &new),
            Agreement {
                rows: 3,
                agreeing: 3,
                expected: 3,
            }
        );
    }

    #[test]
    fn unmatched_rows_are_not_measured() {
        let old = table! {
            "id" => [1, 2],
            "value" => [10, 20],
        };
        let new = table! {
            "id" => [3, 4],
            "value" => [10, 20],
        };

        let agreement = measure(&old, &new);

        // Identical columns, but no row is common to both files, so there is
        // nothing to measure and neither predicate can be satisfied.
        assert_eq!(agreement.rows, 0);
        assert!(!agreement.is_close());
        assert!(!agreement.is_distant());
    }

    #[test]
    fn observed_agreement_excludes_its_own_boundary() {
        let boundary = Agreement {
            rows: 10,
            agreeing: 9,
            expected: 0,
        };
        assert!(!boundary.observed_above(MIN_AGREEMENT_PERCENT));

        // Nine in ten is not more than nine in ten, so an imperfect pair needs
        // eleven rows before it can clear the threshold at all.
        let above = Agreement {
            rows: 11,
            agreeing: 10,
            expected: 0,
        };
        assert!(above.observed_above(MIN_AGREEMENT_PERCENT));
    }

    #[test]
    fn chance_corrected_agreement_excludes_its_own_boundary() {
        // Ten rows agreeing in nine with p_e = 0.5 is kappa = 0.8 exactly.
        let boundary = Agreement {
            rows: 10,
            agreeing: 9,
            expected: 50,
        };
        assert!(!boundary.kappa_above(MIN_KAPPA_PERCENT));

        let above = Agreement {
            rows: 10,
            agreeing: 9,
            expected: 49,
        };
        assert!(above.kappa_above(MIN_KAPPA_PERCENT));
    }

    #[test]
    fn high_agreement_is_not_enough_without_beating_chance() {
        // Nineteen of twenty rows agree, but one value fills the column, so
        // the agreement says nothing that chance does not already explain.
        let nearly_constant = Agreement {
            rows: 20,
            agreeing: 19,
            expected: 380,
        };

        assert!(nearly_constant.observed_above(MIN_AGREEMENT_PERCENT));
        assert!(!nearly_constant.is_close());
    }

    #[test]
    fn a_constant_pair_is_rejected_before_kappa_is_undefined() {
        let constant = Agreement {
            rows: 5,
            agreeing: 5,
            expected: 25,
        };

        // Every row agrees, and none of that agreement means anything.
        assert_eq!(constant.agreeing, constant.rows);
        assert!(!constant.informative());
        assert!(!constant.is_close());
    }

    #[test]
    fn a_rewritten_column_is_distant_and_a_kept_one_is_not() {
        let rewritten = Agreement {
            rows: 10,
            agreeing: 4,
            expected: 0,
        };
        let half = Agreement {
            rows: 10,
            agreeing: 5,
            expected: 0,
        };

        assert!(rewritten.is_distant());
        // Half is not fewer than half, and neither is distant nor close: the
        // gap between the two predicates is evidence of nothing.
        assert!(!half.is_distant());
        assert!(!half.is_close());
    }

    #[test]
    fn a_memoized_measurement_charges_no_further_unit() {
        let old = table! {
            "id" => [1, 2],
            "value" => [10, 20],
        };
        let new = table! {
            "id" => [1, 2],
            "value" => [10, 20],
        };
        let rows = matched(&old, &new);
        let sample = RowSample::full();
        let plan =
            ComparisonPlan::new(old.column(1).data_type(), new.column(1).data_type()).unwrap();
        let mut values = Aligned::new(&old, &new, &rows, &sample);
        let mut meter = Meter::new(1);

        let first = values.measure_full(&mut meter, plan, 1, 1);
        let again = values.measure_full(&mut meter, plan, 1, 1);

        // One unit funded the first measurement; the repeat is the memo, so it
        // answers even though the meter is spent.
        assert!(first.is_some());
        assert_eq!(again, first);
        // A distinct pair is a distinct examination, which nothing funds now.
        assert_eq!(values.measure_full(&mut meter, plan, 1, 0), None);
    }

    #[test]
    fn a_sampled_measurement_with_a_full_sample_shares_the_full_memo() {
        let old = table! {
            "id" => [1, 2],
            "value" => [10, 20],
        };
        let new = table! {
            "id" => [1, 2],
            "value" => [10, 20],
        };
        let rows = matched(&old, &new);
        let sample = RowSample::full();
        let plan =
            ComparisonPlan::new(old.column(1).data_type(), new.column(1).data_type()).unwrap();
        let mut values = Aligned::new(&old, &new, &rows, &sample);
        let mut meter = Meter::new(1);

        let full = values.measure_full(&mut meter, plan, 1, 1);
        let sampled = values.measure_sampled(&mut meter, plan, 1, 1);

        // The same rows answer both questions, so the second asks nothing new.
        assert_eq!(sampled, full);
    }

    #[test]
    fn a_verification_is_memoized_like_a_measurement() {
        let old = table! {
            "id" => [1, 2],
            "value" => [10, 20],
        };
        let new = table! {
            "id" => [1, 2],
            "value" => [10, 20],
        };
        let rows = matched(&old, &new);
        let sample = RowSample::full();
        let plan =
            ComparisonPlan::new(old.column(1).data_type(), new.column(1).data_type()).unwrap();
        let mut values = Aligned::new(&old, &new, &rows, &sample);
        let mut meter = Meter::new(1);

        assert_eq!(values.verify(&mut meter, plan, 1, 1), Some(true));
        assert_eq!(values.verify(&mut meter, plan, 1, 1), Some(true));
        assert_eq!(values.verify(&mut meter, plan, 1, 0), None);
    }

    #[test]
    fn the_sample_is_every_matched_row_at_or_below_the_cap() {
        let old = table! {
            "id" => [1, 2, 3],
            "value" => [10, 20, 30],
        };
        let new = table! {
            "id" => [1, 2, 3],
            "value" => [10, 20, 30],
        };
        let options = DiffOptions {
            key: vec!["id".into()],
            ..DiffOptions::default()
        };
        let key = resolve_key(&old, &new, &options).unwrap();
        let rows = match_rows(&key);

        assert!(RowSample::select(&key, &rows, 3).is_full());
        assert!(!RowSample::select(&key, &rows, 2).is_full());
    }

    #[test]
    fn the_sample_selects_by_key_hash_and_is_stable() {
        let old = table! {
            "id" => [1, 2, 3, 4],
            "value" => [10, 20, 30, 40],
        };
        let new = table! {
            "id" => [1, 2, 3, 4],
            "value" => [10, 20, 30, 40],
        };
        let options = DiffOptions {
            key: vec!["id".into()],
            ..DiffOptions::default()
        };
        let key = resolve_key(&old, &new, &options).unwrap();
        let rows = match_rows(&key);

        let first = RowSample::select(&key, &rows, 2);
        let again = RowSample::select(&key, &rows, 2);

        assert_eq!(first.0.as_ref().map(Vec::len), Some(2));
        assert_eq!(first.0, again.0);
    }

    #[test]
    fn colliding_key_hashes_fall_back_to_old_position() {
        let old = table! {
            "id" => [1, 2, 3, 4],
            "value" => [10, 20, 30, 40],
        };
        let new = table! {
            "id" => [4, 3, 2, 1],
            "value" => [40, 30, 20, 10],
        };
        let options = DiffOptions {
            key: vec!["id".into()],
            ..DiffOptions::default()
        };
        let key = resolve_key(&old, &new, &options).unwrap();
        let rows = match_rows(&key);

        // Every key digests alike, so nothing but the tie-break chooses, and
        // the tie-break is position in the matching — old-row order.
        let sample = RowSample::with_hash(&key, &rows, 2, |_: &[CanonicalValue]| 0);

        assert_eq!(sample.0, Some(vec![0, 1]));
    }

    #[test]
    fn a_sampled_measurement_reads_only_the_sample() {
        let old = table! {
            "id" => [1, 2, 3, 4],
            "value" => [10, 20, 30, 40],
        };
        let new = table! {
            "id" => [1, 2, 3, 4],
            "value" => [10, 20, 99, 98],
        };
        let options = DiffOptions {
            key: vec!["id".into()],
            ..DiffOptions::default()
        };
        let key = resolve_key(&old, &new, &options).unwrap();
        let rows = match_rows(&key);
        // Force the sample onto the two agreeing rows.
        let sample = RowSample::with_hash(&key, &rows, 2, |_: &[CanonicalValue]| 0);
        let plan =
            ComparisonPlan::new(old.column(1).data_type(), new.column(1).data_type()).unwrap();
        let mut values = Aligned::new(&old, &new, &rows, &sample);
        let mut meter = Meter::unlimited();

        let sampled = values
            .measure_sampled(&mut meter, plan, 1, 1)
            .expect("an unlimited meter funds every measurement");
        let full = values
            .measure_full(&mut meter, plan, 1, 1)
            .expect("an unlimited meter funds every measurement");

        assert_eq!(sampled.rows, 2);
        assert_eq!(sampled.agreeing, 2);
        assert_eq!(full.rows, 4);
        assert_eq!(full.agreeing, 2);
    }
}
