use arrow_array::RecordBatch;

use crate::compare::ComparisonPlan;
use crate::key::ResolvedKey;
use crate::{DiffError, Side};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SchemaMatches {
    pub identities: Vec<ColumnIdentity>,
    pub added: Vec<usize>,
    pub dropped: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ColumnIdentity {
    pub old: usize,
    pub new: usize,
    pub type_changed: bool,
    pub is_key: bool,
}

/// The partial bijection between old and new columns, built up in stages.
///
/// This is the identity model `design.md` describes, in one place rather than
/// several: rename hints claim first, key components next, same-named columns
/// take what is left, and inference fills in from there. Every stage claims
/// through this, so no endpoint can be spent twice and no stage has to be
/// trusted to check that for itself.
///
/// An endpoint can also be spent on having no partner at all. `col_drop` and
/// `col_add` reserve one that way, and holding the reservation here rather than
/// filtering for it at each stage is what keeps reconciliation free of rules
/// about hints: a reserved column is passed over by name matching because the
/// map refuses to pair it, and becomes a drop or an addition because the map has
/// no pair for it, which is how every drop and addition is already derived.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ColumnMap {
    /// Ascending by old position, which is what `detect_order` requires of the
    /// identities derived from it.
    pairs: Vec<ColumnPair>,
    /// Old columns reserved as having no partner, ascending.
    unmatched_old: Vec<usize>,
    /// New columns reserved as having no partner, ascending.
    unmatched_new: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ColumnPair {
    pub old: usize,
    pub new: usize,
    /// Whether a hint asserted this pair rather than reconciliation choosing it.
    ///
    /// One flag rather than a full account of where an identity came from: the
    /// only question asked of it today is whether inference may reconsider the
    /// pair, and it may not reconsider an instruction. Recording the source
    /// properly, and rendering it, is its own queued step.
    pub hinted: bool,
}

impl ColumnMap {
    /// Claim a pair, unless either endpoint is already spoken for.
    ///
    /// Returns whether the map now holds the pair, so a caller that cares can
    /// tell a refusal from a claim it already had. An endpoint reserved as
    /// unmatched is spent like any other: it was claimed for having no partner,
    /// and a pair would give it one.
    pub(crate) fn claim(&mut self, old: usize, new: usize, hinted: bool) -> bool {
        if self.reserved(Side::Old, old) || self.reserved(Side::New, new) {
            return false;
        }
        match (self.new_for_old(old), self.old_for_new(new)) {
            // Both endpoints free.
            (None, None) => {
                let at = self.pairs.partition_point(|pair| pair.old < old);
                self.pairs.insert(at, ColumnPair { old, new, hinted });
                true
            }
            // Exactly this pair is already held, so the claim is redundant
            // rather than contested — two stages agreeing about one column.
            (Some(held_new), Some(held_old)) => held_new == new && held_old == old,
            // One endpoint is spent on a different partner.
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

    /// Whether this endpoint is reserved as having no partner.
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

    /// Whether a hint asserted this pair.
    pub(crate) fn hinted(&self, old: usize, new: usize) -> bool {
        self.pairs
            .iter()
            .any(|pair| pair.old == old && pair.new == new && pair.hinted)
    }

    pub(crate) fn pairs(&self) -> &[ColumnPair] {
        &self.pairs
    }
}

/// Continue the bijection hints began, then read the schema events off it.
///
/// `hinted` arrives already carrying whatever rename hints established. Key
/// components claim next, and only then are names considered, so a declared
/// pair or an asserted rename holds both its endpoints whatever they are
/// called. Nothing here has to check for a contest: `ColumnMap::claim` refuses
/// a spent endpoint, and hints that contradicted the key were rejected before
/// this ran.
pub(crate) fn reconcile_schema(
    old: &RecordBatch,
    new: &RecordBatch,
    key: &ResolvedKey,
    hinted: &ColumnMap,
) -> Result<SchemaMatches, DiffError> {
    let old_schema = old.schema();
    let new_schema = new.schema();
    let mut map = hinted.clone();

    for column in &key.columns {
        map.claim(column.old, column.new, false);
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
            // A same-named column already claimed is unavailable, which leaves
            // this column unmatched rather than sharing an endpoint.
            map.claim(old_index, new_index, false);
        }
    }

    let key_columns = key
        .columns
        .iter()
        .map(|column| (column.old, column.new))
        .collect::<Vec<_>>();
    let mut result = SchemaMatches::default();
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
        result.identities.push(ColumnIdentity {
            old: pair.old,
            new: pair.new,
            type_changed: old_field.data_type() != new_field.data_type(),
            is_key: key_columns.contains(&(pair.old, pair.new)),
        });
    }
    // What the bijection left over: an unmatched old column is a drop and an
    // unmatched new one an addition, which is the whole of the definition.
    result.dropped = (0..old.num_columns())
        .filter(|&index| map.new_for_old(index).is_none())
        .collect();
    result.added = (0..new.num_columns())
        .filter(|&index| map.old_for_new(index).is_none())
        .collect();
    Ok(result)
}

/// Schema reconciliation as it looks to tests whose subject is not hints.
#[cfg(test)]
pub(crate) mod testing {
    use arrow_array::RecordBatch;

    use super::{ColumnMap, SchemaMatches};
    use crate::DiffError;
    use crate::key::ResolvedKey;

    /// Reconcile the two schemas with no hints in play.
    ///
    /// Reconciliation resolves hints first and passes in what they claimed.
    /// Keeping this under the same name spares every test that predates hints
    /// from restating "and nothing was hinted" at each of its call sites.
    pub(crate) fn reconcile_schema(
        old: &RecordBatch,
        new: &RecordBatch,
        key: &ResolvedKey,
    ) -> Result<SchemaMatches, DiffError> {
        super::reconcile_schema(old, new, key, &ColumnMap::default())
    }
}

#[cfg(test)]
mod tests {
    use arrow_array::RecordBatch;
    use test_support::table;

    use super::testing::reconcile_schema;
    use super::{ColumnIdentity, SchemaMatches};
    use crate::key::testing::resolve_key;
    use crate::{DiffError, DiffOptions};

    fn reconcile(old: &RecordBatch, new: &RecordBatch) -> Result<SchemaMatches, DiffError> {
        let options = DiffOptions {
            key: vec!["id".into()],
            hints: Vec::new(),
        };
        let key = resolve_key(old, new, &options)?;
        reconcile_schema(old, new, &key)
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

        assert_eq!(
            reconcile(&old, &new).unwrap(),
            SchemaMatches {
                identities: vec![
                    ColumnIdentity {
                        old: 0,
                        new: 1,
                        type_changed: false,
                        is_key: true,
                    },
                    ColumnIdentity {
                        old: 2,
                        new: 0,
                        type_changed: false,
                        is_key: false,
                    },
                ],
                added: vec![2],
                dropped: vec![1],
            }
        );
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

        // The pair claims old "a" and new "b". Both names still exist on the
        // other side, but their endpoints are taken, so old "b" has nothing
        // left to match and new "a" is unclaimed.
        assert_eq!(
            reconcile_schema(&old, &new, &key).unwrap(),
            SchemaMatches {
                identities: vec![ColumnIdentity {
                    old: 0,
                    new: 1,
                    type_changed: false,
                    is_key: true,
                }],
                added: vec![0],
                dropped: vec![1],
            }
        );
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
        let schema = reconcile_schema(&old, &new, &key).unwrap();

        assert_eq!(
            schema.identities,
            [
                ColumnIdentity {
                    old: 0,
                    new: 0,
                    type_changed: false,
                    is_key: true,
                },
                ColumnIdentity {
                    old: 1,
                    new: 1,
                    type_changed: false,
                    is_key: false,
                },
            ]
        );
        assert!(schema.added.is_empty());
        assert!(schema.dropped.is_empty());
    }

    #[test]
    fn records_key_and_non_key_type_changes() {
        let old = table! {
            "id" => i32[1],
            "value" => i32[1],
        };
        let new = table! {
            "id" => [1],
            "value" => [1.0],
        };

        let schema = reconcile(&old, &new).unwrap();

        assert!(schema.identities[0].is_key);
        assert!(schema.identities[0].type_changed);
        assert!(schema.identities[1].type_changed);
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
