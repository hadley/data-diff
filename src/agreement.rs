//! How far two columns agree over the rows their tables have in common.
//!
//! Rename and swap inference ask the same question of a candidate pair, so the
//! measurement lives here rather than in either of them.

use std::collections::HashMap;

use arrow_array::RecordBatch;

use crate::compare::{CanonicalValue, ComparisonPlan, sequence_hash};
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

/// Candidate columns projected onto the matched rows, measured on demand.
///
/// Canonicalization depends on the pair rather than on the column — a string
/// column keeps its bytes against another string column but is parsed into
/// integers against an integer one — so a projection is cached per column
/// *and* plan. A candidate takes part in few plans in practice, which keeps
/// this a small multiple of one pass per column rather than one pass per pair.
pub(crate) struct Aligned<'a> {
    old: &'a RecordBatch,
    new: &'a RecordBatch,
    rows: &'a RowMatches,
    digest: fn(&[CanonicalValue]) -> u128,
    cache: HashMap<(Side, usize, ComparisonPlan), Projection>,
}

/// One column's values over the matched rows, with what can be read off them.
struct Projection {
    digest: u128,
    values: Vec<CanonicalValue>,
    counts: HashMap<CanonicalValue, u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Side {
    Old,
    New,
}

impl<'a> Aligned<'a> {
    pub(crate) fn new(old: &'a RecordBatch, new: &'a RecordBatch, rows: &'a RowMatches) -> Self {
        Self::with_digest(old, new, rows, sequence_hash)
    }

    /// The digest is injectable so a test can force every column to collide,
    /// which is the only way to reach the verification behind it.
    pub(crate) fn with_digest(
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
    pub(crate) fn agree(&mut self, plan: ComparisonPlan, old: usize, new: usize) -> bool {
        self.fill(plan, Side::Old, old);
        self.fill(plan, Side::New, new);
        let old = &self.cache[&(Side::Old, old, plan)];
        let new = &self.cache[&(Side::New, new, plan)];
        old.digest == new.digest && old.values == new.values
    }

    /// Count what the two columns agree about over the matched rows.
    pub(crate) fn measure(&mut self, plan: ComparisonPlan, old: usize, new: usize) -> Agreement {
        self.fill(plan, Side::Old, old);
        self.fill(plan, Side::New, new);
        let old = &self.cache[&(Side::Old, old, plan)];
        let new = &self.cache[&(Side::New, new, plan)];
        Agreement {
            rows: old.values.len() as u64,
            agreeing: old
                .values
                .iter()
                .zip(&new.values)
                .filter(|(old, new)| old == new)
                .count() as u64,
            // Each term is a product of two row counts, so it is widened
            // before it is formed rather than after.
            expected: old
                .counts
                .iter()
                .filter_map(|(value, count)| {
                    new.counts
                        .get(value)
                        .map(|shared| u128::from(*count) * u128::from(*shared))
                })
                .sum(),
        }
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

fn project(values: &[CanonicalValue], rows: impl Iterator<Item = usize>) -> Vec<CanonicalValue> {
    rows.map(|row| values[row].clone()).collect()
}

#[cfg(test)]
mod tests {
    use arrow_array::RecordBatch;
    use test_support::table;

    use super::{Agreement, Aligned, MIN_AGREEMENT_PERCENT, MIN_KAPPA_PERCENT};
    use crate::DiffOptions;
    use crate::compare::ComparisonPlan;
    use crate::key::resolve_key;
    use crate::rows::match_rows;

    /// Measure the second column of each table against the other.
    fn measure(old: &RecordBatch, new: &RecordBatch) -> Agreement {
        let options = DiffOptions {
            key: vec!["id".into()],
        };
        let key = resolve_key(old, new, &options).unwrap();
        let rows = match_rows(&key);
        let plan =
            ComparisonPlan::new(old.column(1).data_type(), new.column(1).data_type()).unwrap();
        Aligned::new(old, new, &rows).measure(plan, 1, 1)
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
}
