use arrow_array::RecordBatch;
use arrow_schema::Schema;

use crate::compare::ComparisonPlan;
use crate::key::ResolvedKey;
use crate::{DiffError, IdentityBasis, KeyBasis, Side};

/// The partial bijection between old and new columns.
///
/// All reconciliation stages claim through this map, so no endpoint can be
/// spent twice. Drops and additions are derived from missing pairs rather than
/// maintained separately.
///
/// An endpoint can also be spent on having no partner at all. `col_drop` and
/// `col_add` reserve one that way, and holding the reservation here rather than
/// filtering for it at each stage is what keeps reconciliation free of rules
/// about hints: a reserved column is passed over by name matching because the
/// map refuses to pair it, and falls out as a drop or an addition because the
/// map has no pair for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ColumnMap {
    /// Ascending by old position, which is what `detect_order` requires of the
    /// identities derived from it.
    pairs: Vec<ColumnPair>,
    /// Old columns reserved as having no partner, ascending.
    unmatched_old: Vec<usize>,
    /// New columns reserved as having no partner, ascending.
    unmatched_new: Vec<usize>,
    old_width: usize,
    new_width: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ColumnPair {
    pub old: usize,
    pub new: usize,
    /// Recorded because swap inference needs to distinguish provisional
    /// same-name identities from stronger claims.
    pub basis: IdentityBasis,
    /// Orthogonal to `basis`: several kinds of identity can form a key.
    pub is_key: bool,
}

impl ColumnMap {
    pub(crate) fn new(old: &Schema, new: &Schema) -> Self {
        Self {
            pairs: Vec::new(),
            unmatched_old: Vec::new(),
            unmatched_new: Vec::new(),
            old_width: old.fields().len(),
            new_width: new.fields().len(),
        }
    }

    /// Claim a pair unless either endpoint is already spoken for.
    ///
    /// Returns whether the map now holds the pair, so a caller that cares can
    /// tell a refusal from a claim it already had. An endpoint reserved as
    /// unmatched is spent like any other: it was claimed for having no partner,
    /// and a pair would give it one.
    ///
    /// A claim the map already holds leaves the basis it was claimed with. That
    /// is what ranks the sources without any of them being ranked anywhere: the
    /// stages claim in the design's order of authority, so the first claimer is
    /// the one that decided, and a later stage agreeing does not make it the
    /// author.
    pub(crate) fn claim(&mut self, old: usize, new: usize, basis: IdentityBasis) -> bool {
        if self.reserved(Side::Old, old) || self.reserved(Side::New, new) {
            return false;
        }
        match (self.new_for_old(old), self.old_for_new(new)) {
            (None, None) => {
                let at = self.pairs.partition_point(|pair| pair.old < old);
                self.pairs.insert(
                    at,
                    ColumnPair {
                        old,
                        new,
                        basis,
                        is_key: false,
                    },
                );
                true
            }
            // Repeating an existing pair is agreement, not a conflict.
            (Some(held_new), Some(held_old)) => held_new == new && held_old == old,
            _ => false,
        }
    }

    /// Claim one endpoint as having no partner, unless it is already paired.
    ///
    /// Reserving twice is redundant rather than contested, two instructions
    /// saying the same thing.
    pub(crate) fn reserve(&mut self, side: Side, index: usize) -> bool {
        let paired = match side {
            Side::Old => self.new_for_old(index).is_some(),
            Side::New => self.old_for_new(index).is_some(),
        };
        if paired {
            return false;
        }
        let reserved = self.unmatched_mut(side);
        if let Err(at) = reserved.binary_search(&index) {
            reserved.insert(at, index);
        }
        true
    }

    pub(crate) fn reserved(&self, side: Side, index: usize) -> bool {
        let reserved = match side {
            Side::Old => &self.unmatched_old,
            Side::New => &self.unmatched_new,
        };
        reserved.binary_search(&index).is_ok()
    }

    fn unmatched_mut(&mut self, side: Side) -> &mut Vec<usize> {
        match side {
            Side::Old => &mut self.unmatched_old,
            Side::New => &mut self.unmatched_new,
        }
    }

    pub(crate) fn new_for_old(&self, old: usize) -> Option<usize> {
        self.pairs
            .iter()
            .find(|pair| pair.old == old)
            .map(|pair| pair.new)
    }

    pub(crate) fn old_for_new(&self, new: usize) -> Option<usize> {
        self.pairs
            .iter()
            .find(|pair| pair.new == new)
            .map(|pair| pair.old)
    }

    pub(crate) fn mark_key(&mut self, old: usize, new: usize) {
        if let Some(pair) = self
            .pairs
            .iter_mut()
            .find(|pair| pair.old == old && pair.new == new)
        {
            pair.is_key = true;
        }
    }

    /// Exchange two pairs' new ends, atomically and in place.
    ///
    /// Neither old position moves, so the pairs stay sorted by old position and
    /// `minimal_moves` keeps the precondition it asserts. The new positions do
    /// move, which is why an accepted swap can produce a `col_order()` entry:
    /// the column holding one column's values is now where the other one was.
    pub(crate) fn exchange(&mut self, first_old: usize, second_old: usize) {
        let (Some(first), Some(second)) = (self.position(first_old), self.position(second_old))
        else {
            return;
        };
        let first_new = self.pairs[first].new;
        self.pairs[first].new = self.pairs[second].new;
        self.pairs[second].new = first_new;
        self.pairs[first].basis = IdentityBasis::Swapped;
        self.pairs[second].basis = IdentityBasis::Swapped;
    }

    fn position(&self, old: usize) -> Option<usize> {
        self.pairs.iter().position(|pair| pair.old == old)
    }

    pub(crate) fn pairs(&self) -> &[ColumnPair] {
        &self.pairs
    }

    pub(crate) fn dropped(&self) -> Vec<usize> {
        (0..self.old_width)
            .filter(|&index| self.new_for_old(index).is_none())
            .collect()
    }

    pub(crate) fn added(&self) -> Vec<usize> {
        (0..self.new_width)
            .filter(|&index| self.old_for_new(index).is_none())
            .collect()
    }
}

/// Continue the bijection hints began, claiming the key before matching names.
///
/// `map` arrives already carrying whatever the declared key and the accepted
/// rename hints established. The resolved key claims next, and only then are
/// names considered, so a declared pair or an asserted rename holds both its
/// endpoints whatever they are called. Nothing here has to check for a contest:
/// `ColumnMap::claim` refuses a spent endpoint, and hints that contradicted the
/// key were rejected before this ran.
pub(crate) fn reconcile_schema(
    old: &RecordBatch,
    new: &RecordBatch,
    key: &ResolvedKey,
    map: &mut ColumnMap,
) -> Result<(), DiffError> {
    let old_schema = old.schema();
    let new_schema = new.schema();

    // A declared component asserts its pair; a guessed key only picks one of the
    // identities the names would have produced anyway, so it claims as one.
    // A positional key claims nothing, having no columns to claim with.
    let basis = match key.basis {
        KeyBasis::Declared => IdentityBasis::Declared,
        KeyBasis::Guessed | KeyBasis::Fallback => IdentityBasis::Name,
    };
    for column in &key.columns {
        map.claim(column.old, column.new, basis);
        map.mark_key(column.old, column.new);
    }
    for (old_index, old_field) in old_schema.fields().iter().enumerate() {
        if map.new_for_old(old_index).is_some() {
            continue;
        }
        let by_name = new_schema
            .fields()
            .iter()
            .position(|field| field.name() == old_field.name());
        if let Some(new_index) = by_name {
            map.claim(old_index, new_index, IdentityBasis::Name);
        }
    }

    for pair in map.pairs() {
        let old_field = old_schema.field(pair.old);
        let new_field = new_schema.field(pair.new);
        if ComparisonPlan::new(old_field.data_type(), new_field.data_type()).is_none() {
            return Err(DiffError::IncompatibleColumns {
                column: old_field.name().clone(),
                old_type: format!("{:?}", old_field.data_type()),
                new_type: format!("{:?}", new_field.data_type()),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod testing {
    use arrow_array::RecordBatch;

    use super::ColumnMap;
    use crate::DiffError;
    use crate::key::ResolvedKey;

    /// Reconcile the two schemas with no hints in play.
    ///
    /// Reconciliation resolves hints first and passes in the map they claimed
    /// into. Keeping this under the same name spares every test that predates
    /// hints from restating "and nothing was hinted" at each of its call sites.
    pub(crate) fn reconcile_schema(
        old: &RecordBatch,
        new: &RecordBatch,
        key: &ResolvedKey,
    ) -> Result<ColumnMap, DiffError> {
        let mut map = ColumnMap::new(old.schema_ref(), new.schema_ref());
        super::reconcile_schema(old, new, key, &mut map)?;
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use arrow_array::RecordBatch;
    use test_support::table;

    use super::ColumnMap;
    use super::testing::reconcile_schema;
    use crate::key::testing::resolve_key;
    use crate::{DiffError, DiffOptions, IdentityBasis, Side};

    fn reconcile(old: &RecordBatch, new: &RecordBatch) -> Result<ColumnMap, DiffError> {
        let options = DiffOptions {
            key: vec!["id".into()],
            hints: Vec::new(),
        };
        let key = resolve_key(old, new, &options)?;
        reconcile_schema(old, new, &key)
    }

    fn pairs(map: &ColumnMap) -> Vec<(usize, usize, IdentityBasis, bool)> {
        map.pairs()
            .iter()
            .map(|pair| (pair.old, pair.new, pair.basis, pair.is_key))
            .collect()
    }

    fn empty(old: &RecordBatch, new: &RecordBatch) -> ColumnMap {
        ColumnMap::new(old.schema_ref(), new.schema_ref())
    }

    #[test]
    fn identifies_same_names_and_classifies_unmatched_columns() {
        let old = table! {
            "id" => [1],
            "drop" => [1],
            "value" => [1.0],
        };
        let new = table! {
            "value" => [1.0],
            "id" => [1],
            "add" => [1],
        };

        let map = reconcile(&old, &new).unwrap();

        assert_eq!(
            pairs(&map),
            [
                (0, 1, IdentityBasis::Declared, true),
                (2, 0, IdentityBasis::Name, false),
            ]
        );
        assert_eq!(map.dropped(), [1]);
        assert_eq!(map.added(), [2]);
    }

    #[test]
    fn a_key_pair_reserves_its_identity_before_names_are_matched() {
        let old = table! {
            "a" => [1, 2],
            "b" => [10, 20],
        };
        let new = table! {
            "a" => [10, 20],
            "b" => [1, 2],
        };

        let options = DiffOptions {
            key: vec!["a/b".into()],
            hints: Vec::new(),
        };
        let key = resolve_key(&old, &new, &options).unwrap();

        let map = reconcile_schema(&old, &new, &key).unwrap();

        assert_eq!(pairs(&map), [(0, 1, IdentityBasis::Declared, true)]);
        assert_eq!(map.dropped(), [1]);
        assert_eq!(map.added(), [0]);
    }

    #[test]
    fn a_renamed_key_keeps_the_other_columns_matching_by_name() {
        let old = table! {
            "customer_id" => [1, 2],
            "value" => [10, 20],
        };
        let new = table! {
            "id" => [1, 2],
            "value" => [10, 21],
        };

        let options = DiffOptions {
            key: vec!["customer_id/id".into()],
            hints: Vec::new(),
        };
        let key = resolve_key(&old, &new, &options).unwrap();
        let map = reconcile_schema(&old, &new, &key).unwrap();

        assert_eq!(
            pairs(&map),
            [
                (0, 0, IdentityBasis::Declared, true),
                (1, 1, IdentityBasis::Name, false),
            ]
        );
        assert!(map.dropped().is_empty());
        assert!(map.added().is_empty());
    }

    #[test]
    fn a_guessed_key_is_identified_by_its_name_like_any_other_column() {
        let old = table! {
            "id" => [1, 2],
            "value" => [10, 20],
        };
        let new = table! {
            "id" => [1, 2],
            "value" => [10, 21],
        };

        let options = DiffOptions::default();
        let key = resolve_key(&old, &new, &options).unwrap();
        let map = reconcile_schema(&old, &new, &key).unwrap();

        assert_eq!(
            pairs(&map),
            [
                (0, 0, IdentityBasis::Name, true),
                (1, 1, IdentityBasis::Name, false),
            ]
        );
    }

    #[test]
    fn the_drops_and_additions_ascend_whatever_order_claims_arrived_in() {
        let table = table! {
            "id" => [1],
            "a" => [1],
            "b" => [1],
            "c" => [1],
        };
        let mut map = empty(&table, &table);

        // Inference claims in candidate order rather than in column order, and
        // two stages read these lists positionally against each other, so the
        // order has to come from the map rather than from its callers.
        map.claim(3, 1, IdentityBasis::Exact);
        map.claim(1, 3, IdentityBasis::Approximate);

        assert_eq!(map.dropped(), [0, 2]);
        assert_eq!(map.added(), [0, 2]);
        assert_eq!(
            map.pairs().iter().map(|pair| pair.old).collect::<Vec<_>>(),
            [1, 3]
        );
    }

    #[test]
    fn a_reserved_endpoint_refuses_a_pair_and_falls_out_as_unmatched() {
        let table = table! {
            "id" => [1],
            "value" => [1],
        };
        let mut map = empty(&table, &table);

        assert!(map.reserve(Side::Old, 1));
        assert!(map.reserved(Side::Old, 1));
        assert!(!map.claim(1, 1, IdentityBasis::Name));

        assert_eq!(map.dropped(), [0, 1]);
        assert_eq!(map.added(), [0, 1]);
    }

    #[test]
    fn the_first_claim_settles_the_basis() {
        let table = table! {
            "id" => [1],
            "value" => [1],
        };
        let mut map = empty(&table, &table);

        assert!(map.claim(0, 0, IdentityBasis::Declared));
        assert!(map.claim(0, 0, IdentityBasis::Name));

        assert_eq!(pairs(&map), [(0, 0, IdentityBasis::Declared, false)]);
    }

    #[test]
    fn marking_a_key_leaves_the_basis_alone() {
        let table = table! {
            "id" => [1],
            "value" => [1],
        };
        let mut map = empty(&table, &table);

        map.claim(0, 0, IdentityBasis::Hinted);
        map.mark_key(0, 0);

        assert_eq!(pairs(&map), [(0, 0, IdentityBasis::Hinted, true)]);
    }

    #[test]
    fn exchanging_swaps_two_new_ends_and_records_both() {
        let table = table! {
            "id" => [1],
            "price" => [1],
            "cost" => [1],
        };
        let mut map = empty(&table, &table);

        map.claim(0, 0, IdentityBasis::Declared);
        map.claim(1, 1, IdentityBasis::Name);
        map.claim(2, 2, IdentityBasis::Name);
        map.exchange(1, 2);

        assert_eq!(
            pairs(&map),
            [
                (0, 0, IdentityBasis::Declared, false),
                (1, 2, IdentityBasis::Swapped, false),
                (2, 1, IdentityBasis::Swapped, false),
            ]
        );
        assert!(map.dropped().is_empty());
        assert!(map.added().is_empty());
    }

    #[test]
    fn rejects_incompatible_same_name_columns() {
        let old = table! {
            "id" => [1],
            "flag" => [true],
        };
        let new = table! {
            "id" => [1],
            "flag" => [1],
        };

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
