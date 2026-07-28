use std::collections::{HashMap, HashSet};

use crate::compare::{CanonicalValue, ComparisonPlan, sequence_hash, stable_hash};
use crate::hint::Hints;
use crate::{DiffError, KeyBasis, KeyOverlap, Side};
use arrow_array::RecordBatch;

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
    /// The component as declared, which for a pair names both columns.
    pub component: String,
    pub old: usize,
    pub new: usize,
}

/// The share of common key values a declared key may duplicate in `new` and
/// still be read as fanout rather than as a broken key.
///
/// `DiffError::ExcessiveFanout` states this limit in its message.
pub(crate) const MAX_FANOUT_PERCENT: usize = 10;

/// Rows grouped by key hash, with equality confirmed on lookup.
///
/// A bucket can hold rows with different keys, so a collision must never decide
/// membership; confirming equality inside the bucket is what key validation and
/// row matching both need, and sharing it keeps that reasoning in one place.
pub(crate) struct KeyIndex<'a> {
    keys: &'a [Vec<CanonicalValue>],
    buckets: HashMap<u128, Vec<usize>>,
    hash: fn(&[CanonicalValue]) -> u128,
}

impl<'a> KeyIndex<'a> {
    pub(crate) fn new(keys: &'a [Vec<CanonicalValue>]) -> Self {
        Self::with_hash(keys, sequence_hash)
    }

    /// The bucketing is parameterized by its hash so a test can force every key
    /// into one bucket, which is the only way to reach the confirmation step.
    fn with_hash(keys: &'a [Vec<CanonicalValue>], hash: fn(&[CanonicalValue]) -> u128) -> Self {
        let mut buckets = HashMap::<u128, Vec<usize>>::new();
        for (row, key) in keys.iter().enumerate() {
            buckets.entry(hash(key)).or_default().push(row);
        }
        Self {
            keys,
            buckets,
            hash,
        }
    }

    /// The rows whose key equals `key`, in ascending row order.
    ///
    /// Buckets are filled in row order and filtered rather than sorted, so the
    /// result never depends on hash iteration order.
    pub(crate) fn rows<'b>(
        &'b self,
        key: &'b [CanonicalValue],
    ) -> impl Iterator<Item = usize> + 'b {
        self.buckets
            .get(&(self.hash)(key))
            .into_iter()
            .flatten()
            .copied()
            .filter(move |&row| self.keys[row] == key)
    }
}

pub(crate) fn resolve_key(
    old: &RecordBatch,
    new: &RecordBatch,
    components: &[Component],
    hints: &Hints,
) -> Result<ResolvedKey, DiffError> {
    if components.is_empty() {
        return guess_key(old, new, hints);
    }
    let mut columns = Vec::with_capacity(components.len());
    let mut old_components = Vec::with_capacity(components.len());
    let mut new_components = Vec::with_capacity(components.len());

    for component in components {
        // Each endpoint is resolved on its own side, so a missing column is
        // reported as the half that is missing rather than as the whole pair.
        let (old_index, new_index) = component_endpoints(old, new, component, hints)?;
        let old_values = old.column(old_index);
        let new_values = new.column(new_index);
        let plan = ComparisonPlan::new(old_values.data_type(), new_values.data_type()).ok_or_else(
            || DiffError::IncompatibleKeyTypes {
                component: component.spelling.to_owned(),
                old_type: format!("{:?}", old_values.data_type()),
                new_type: format!("{:?}", new_values.data_type()),
            },
        )?;
        old_components.push(plan.canonicalize_old(old_values.as_ref()));
        new_components.push(plan.canonicalize_new(new_values.as_ref()));
        columns.push(KeyColumn {
            component: component.spelling.to_owned(),
            old: old_index,
            new: new_index,
        });
    }

    // Uniqueness is checked again on the resolved coordinates, not only on the
    // names. Two components can name different columns and land on the same
    // one: `--key id,customer_id` with a `customer_id -> id` hint resolves both
    // through that identity, which the name check cannot see.
    validate_distinct(&columns)?;

    let old_keys = transpose(old.num_rows(), &old_components);
    let new_keys = transpose(new.num_rows(), &new_components);
    validate_present(&old_keys, &columns, Side::Old)?;
    validate_present(&new_keys, &columns, Side::New)?;
    validate_unique_old(&old_keys)?;
    validate_fanout(&old_keys, &KeyIndex::new(&new_keys))?;

    Ok(ResolvedKey {
        basis: KeyBasis::Declared,
        columns,
        old: old_keys,
        new: new_keys,
        overlap: None,
    })
}

/// The name pairs a declared key asserts on its own, one per component.
///
/// A component claims an identity even when it names one column: `id` claims
/// that old `id` and new `id` are the same column, which a hint can contradict
/// just as a paired component can.
///
/// It claims that only where it can, though. A component naming a column one
/// side does not have asserts nothing, because it cannot be resolved by name at
/// all — that is the case a hint is there to settle, and treating it as a claim
/// would have the key contradicting the very hint it depends on.
pub(crate) fn claims(
    old: &RecordBatch,
    new: &RecordBatch,
    components: &[Component],
) -> Vec<(String, String)> {
    components
        .iter()
        .filter(|component| {
            position(old, &component.old).is_some() && position(new, &component.new).is_some()
        })
        .map(|component| (component.old.clone(), component.new.clone()))
        .collect()
}

/// Key resolution as it looks to tests whose subject is not hints.
#[cfg(test)]
pub(crate) mod testing {
    use arrow_array::RecordBatch;

    use super::{Hints, ResolvedKey, declared_components};
    use crate::{DiffError, DiffOptions};

    /// Resolve a key from options alone, with no hints in play.
    ///
    /// Reconciliation resolves hints first and passes them in. Keeping this
    /// under the same name spares every test that predates hints from
    /// restating "and no hints" at each of its call sites.
    pub(crate) fn resolve_key(
        old: &RecordBatch,
        new: &RecordBatch,
        options: &DiffOptions,
    ) -> Result<ResolvedKey, DiffError> {
        let components = declared_components(&options.key)?;
        super::resolve_key(old, new, &components, &Hints::default())
    }
}

/// Resolve one component's endpoints, consulting hints where a name is absent.
///
/// A component names a column on each side, which is usually the same name
/// twice. Where one side lacks it, a hint identity whose other end carries it
/// supplies the missing endpoint — which is what lets `--key id` work when the
/// old file still calls that column something else.
fn component_endpoints(
    old: &RecordBatch,
    new: &RecordBatch,
    component: &Component,
    hints: &Hints,
) -> Result<(usize, usize), DiffError> {
    let old_found = position(old, &component.old);
    let new_found = position(new, &component.new);
    let old_index = match (old_found, new_found) {
        (Some(index), _) => index,
        (None, Some(new_index)) => hints
            .old_for_new(new_index)
            .ok_or_else(|| missing_key_column(Side::Old, &component.old))?,
        (None, None) => return Err(missing_key_column(Side::Old, &component.old)),
    };
    let new_index = match new_found {
        Some(index) => index,
        None => hints
            .new_for_old(old_index)
            .ok_or_else(|| missing_key_column(Side::New, &component.new))?,
    };
    Ok((old_index, new_index))
}

/// Reject a key whose components resolved to the same column twice.
fn validate_distinct(columns: &[KeyColumn]) -> Result<(), DiffError> {
    let mut old_seen = HashSet::new();
    let mut new_seen = HashSet::new();
    for column in columns {
        if !old_seen.insert(column.old) {
            return Err(DiffError::DuplicateKeyColumn {
                side: Side::Old,
                column: column.component.clone(),
            });
        }
        if !new_seen.insert(column.new) {
            return Err(DiffError::DuplicateKeyColumn {
                side: Side::New,
                column: column.component.clone(),
            });
        }
    }
    Ok(())
}

fn missing_key_column(side: Side, component: &str) -> DiffError {
    DiffError::MissingKeyColumn {
        side,
        component: component.to_owned(),
    }
}

fn position(table: &RecordBatch, name: &str) -> Option<usize> {
    table
        .schema()
        .fields()
        .iter()
        .position(|field| field.name() == name)
}

/// Select the eligible identified single column that shares the most key values.
///
/// Ranking follows the evidence: the candidate identifying the most rows across
/// the two files wins, and freedom from fanout only settles a tie. A true key
/// that duplicated one row is a better guess than a column that happens to be
/// unique but identifies far fewer rows.
fn guess_key(
    old: &RecordBatch,
    new: &RecordBatch,
    hints: &Hints,
) -> Result<ResolvedKey, DiffError> {
    if old.num_rows() == 0 || new.num_rows() == 0 {
        return Err(DiffError::MissingKey);
    }

    struct Candidate {
        old_index: usize,
        new_index: usize,
        name: String,
        old_values: Vec<CanonicalValue>,
        new_values: Vec<CanonicalValue>,
        overlap: Overlap,
    }

    /// Larger is better: shared keys first, then freedom from fanout.
    fn rank(overlap: &Overlap) -> (usize, bool) {
        (overlap.shared, overlap.affected == 0)
    }

    let new_schema = new.schema();
    let mut best: Option<Candidate> = None;
    for (old_index, old_field) in old.schema().fields().iter().enumerate() {
        // An identified column, which a hint may have identified across a
        // rename; a name whose counterpart a hint claimed for another column is
        // not this column's to use.
        let by_name = new_schema
            .fields()
            .iter()
            .position(|field| field.name() == old_field.name())
            .filter(|&index| {
                hints
                    .old_for_new(index)
                    .is_none_or(|owner| owner == old_index)
            });
        let Some(new_index) = hints.new_for_old(old_index).or(by_name) else {
            continue;
        };
        let old_column = old.column(old_index);
        let new_column = new.column(new_index);
        let Some(plan) = ComparisonPlan::new(old_column.data_type(), new_column.data_type()) else {
            continue;
        };
        let old_values = plan.canonicalize_old(old_column.as_ref());
        let new_values = plan.canonicalize_new(new_column.as_ref());
        let Some(overlap) = candidate_overlap(&old_values, &new_values, stable_hash) else {
            continue;
        };
        if overlap.shared == 0 || !within_fanout_limit(overlap.affected, overlap.shared) {
            continue;
        }
        // Strictly greater, so an earlier column keeps a complete tie.
        if best
            .as_ref()
            .is_none_or(|best| rank(&overlap) > rank(&best.overlap))
        {
            best = Some(Candidate {
                old_index,
                new_index,
                name: old_field.name().clone(),
                old_values,
                new_values,
                overlap,
            });
        }
    }

    let Some(candidate) = best else {
        return Err(DiffError::MissingKey);
    };
    Ok(ResolvedKey {
        basis: KeyBasis::Guessed,
        columns: vec![KeyColumn {
            component: candidate.name,
            old: candidate.old_index,
            new: candidate.new_index,
        }],
        old: single_component_rows(candidate.old_values),
        new: single_component_rows(candidate.new_values),
        overlap: Some(KeyOverlap {
            shared: candidate.overlap.shared,
            // Distinct keys on each side. `old` is unique, so its distinct
            // count is its row count; `new`'s is smaller than its row count
            // exactly when it duplicates a value.
            possible: old.num_rows().min(candidate.overlap.distinct_new),
        }),
    })
}

fn single_component_rows(values: Vec<CanonicalValue>) -> Vec<Vec<CanonicalValue>> {
    values.into_iter().map(|value| vec![value]).collect()
}

/// What a candidate column offers as a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Overlap {
    /// Distinct old keys that also occur in `new`.
    shared: usize,
    /// Those that occur more than once in `new`.
    affected: usize,
    /// Distinct key values in `new`, however often each repeats.
    distinct_new: usize,
}

/// Measure what a candidate column shares across sides.
///
/// Returns `None` when the column cannot identify rows at all: a null or `NaN`
/// on either side, or a duplicated value in `old`. New-side duplication is
/// measured rather than disqualifying, and the caller applies the fanout bound.
///
/// `shared` counts distinct keys rather than matching new rows. The two agree
/// unless a candidate fans out, where counting rows would award a point per
/// duplicate and so rank a duplicated column above a cleaner one that genuinely
/// identifies more rows.
///
/// Hashes are bucket indexes only; equality is confirmed inside each bucket, so
/// a collision can neither manufacture a duplicate nor inflate a count.
fn candidate_overlap(
    old: &[CanonicalValue],
    new: &[CanonicalValue],
    hash: impl Fn(&CanonicalValue) -> u128,
) -> Option<Overlap> {
    if old.iter().chain(new).any(CanonicalValue::invalid_key) {
        return None;
    }
    let old_buckets = buckets(old, &hash);
    if first_occurrences(old, &old_buckets, &hash).count() != old.len() {
        return None;
    }

    let new_buckets = buckets(new, &hash);
    let mut shared = 0;
    let mut affected = 0;
    for value in old {
        match new_buckets.get(&hash(value)).map_or(0, |rows| {
            rows.iter().filter(|&&row| new[row] == *value).count()
        }) {
            0 => {}
            1 => shared += 1,
            _ => {
                shared += 1;
                affected += 1;
            }
        }
    }

    Some(Overlap {
        shared,
        affected,
        distinct_new: first_occurrences(new, &new_buckets, &hash).count(),
    })
}

fn buckets(
    values: &[CanonicalValue],
    hash: impl Fn(&CanonicalValue) -> u128,
) -> HashMap<u128, Vec<usize>> {
    let mut buckets = HashMap::<u128, Vec<usize>>::new();
    for (row, value) in values.iter().enumerate() {
        buckets.entry(hash(value)).or_default().push(row);
    }
    buckets
}

/// The rows holding the first occurrence of each distinct value.
///
/// Counting these counts distinct values, and comparing that count with the
/// row count tests uniqueness, both without trusting the hash: a bucket is
/// filled in row order, so a row is the first occurrence of its value when no
/// earlier row in its bucket holds an equal value.
fn first_occurrences<'a>(
    values: &'a [CanonicalValue],
    buckets: &'a HashMap<u128, Vec<usize>>,
    hash: &'a impl Fn(&CanonicalValue) -> u128,
) -> impl Iterator<Item = usize> + 'a {
    values.iter().enumerate().filter_map(move |(row, value)| {
        buckets[&hash(value)]
            .iter()
            .take_while(|&&earlier| earlier < row)
            .all(|&earlier| values[earlier] != *value)
            .then_some(row)
    })
}

/// One declared key component and the column it names on each side.
/// One declared key component, parsed but not yet resolved to columns.
///
/// Owned rather than borrowed because components are parsed before hints are
/// considered and resolved after, so they outlive the strings they came from.
pub(crate) struct Component {
    /// The component as the user wrote it, for messages about the pair.
    spelling: String,
    old: String,
    new: String,
}

/// Parse each component and check that no column is claimed twice.
///
/// Uniqueness is a property of the endpoints rather than of the component
/// string: `id,id/other` claims `id` on the old side twice while spelling its
/// components differently, and `a/b,c/b` claims `b` on the new side twice.
pub(crate) fn declared_components(keys: &[String]) -> Result<Vec<Component>, DiffError> {
    let mut old_seen = HashSet::new();
    let mut new_seen = HashSet::new();
    let mut components = Vec::with_capacity(keys.len());
    for spelling in keys {
        let mut names = spelling.split('/');
        let old = names.next().expect("splitting yields at least one name");
        // An unpaired component names the same column on both sides.
        let new = names.next().unwrap_or(old);
        if names.next().is_some() {
            return Err(DiffError::MalformedKeyComponent {
                component: spelling.clone(),
            });
        }
        if old.is_empty() || new.is_empty() {
            return Err(DiffError::EmptyKeyComponent);
        }
        if !old_seen.insert(old) {
            return Err(DiffError::DuplicateKeyColumn {
                side: Side::Old,
                column: old.to_owned(),
            });
        }
        if !new_seen.insert(new) {
            return Err(DiffError::DuplicateKeyColumn {
                side: Side::New,
                column: new.to_owned(),
            });
        }
        components.push(Component {
            spelling: spelling.clone(),
            old: old.to_owned(),
            new: new.to_owned(),
        });
    }
    Ok(components)
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
                    component: columns[component].component.clone(),
                    row: row + 1,
                });
            }
        }
    }
    Ok(())
}

/// Reject a key that identifies more than one old row.
///
/// Fanout is one-directional: many old rows mapping to one new row could be an
/// aggregation, a deduplication, or an arbitrary pairing, so old-side
/// duplication stays fatal. It is also what makes the fanout rate well defined,
/// and is therefore checked first.
fn validate_unique_old(keys: &[Vec<CanonicalValue>]) -> Result<(), DiffError> {
    let index = KeyIndex::new(keys);
    for (row, key) in keys.iter().enumerate() {
        let first = index.rows(key).next().expect("a row matches its own key");
        if first != row {
            return Err(DiffError::NonUniqueOldKey {
                first_row: first + 1,
                row: row + 1,
            });
        }
    }
    Ok(())
}

/// Reject a key whose new-side duplication is too broad to read as fanout.
///
/// `old` is unique by this point, so each old row contributes one distinct key:
/// `shared` counts old keys that also occur in `new`, and `affected` counts
/// those that occur more than once there, each once however many new rows it
/// produces. A new key absent from `old` is a set of additions rather than a
/// fanout, so it contributes to neither count and cannot invalidate the key;
/// with no shared keys at all both counts are zero, which is the design's
/// convention that the rate is then zero.
fn validate_fanout(old_keys: &[Vec<CanonicalValue>], new: &KeyIndex) -> Result<(), DiffError> {
    let mut shared = 0;
    let mut affected = 0;
    for key in old_keys {
        match new.rows(key).count() {
            0 => {}
            1 => shared += 1,
            _ => {
                shared += 1;
                affected += 1;
            }
        }
    }
    if !within_fanout_limit(affected, shared) {
        return Err(DiffError::ExcessiveFanout { affected, shared });
    }
    Ok(())
}

/// Whether new-side duplication is small enough to read as fanout.
///
/// Declared and guessed keys share the rule so the two cannot drift; only the
/// consequence differs, since a declaration the user asserted becomes an error
/// while a candidate simply becomes ineligible. Exact integer arithmetic,
/// inclusive at the limit.
fn within_fanout_limit(affected: usize, shared: usize) -> bool {
    affected * 100 <= shared * MAX_FANOUT_PERCENT
}

#[cfg(test)]
mod tests {
    use test_support::{rows_without_columns, table};

    use super::testing::resolve_key;
    use super::{KeyIndex, Overlap, candidate_overlap};
    #[cfg(test)]
    use crate::DiffOptions;
    use crate::compare::{CanonicalValue, stable_hash};
    use crate::{DiffError, KeyBasis, KeyOverlap, Side};

    fn options(key: &[&str]) -> DiffOptions {
        DiffOptions {
            key: key.iter().map(|value| (*value).to_owned()).collect(),
            hints: Vec::new(),
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
            resolve_key(&empty, &empty, &options(&["a/b/c"])),
            Err(DiffError::MalformedKeyComponent { .. })
        ));
        assert!(matches!(
            resolve_key(&empty, &empty, &options(&["a/"])),
            Err(DiffError::EmptyKeyComponent)
        ));
        assert!(matches!(
            resolve_key(&empty, &empty, &options(&["/b"])),
            Err(DiffError::EmptyKeyComponent)
        ));
        assert_eq!(
            resolve_key(&empty, &empty, &options(&["id", "id"])).unwrap_err(),
            DiffError::DuplicateKeyColumn {
                side: Side::Old,
                column: "id".into(),
            }
        );
    }

    #[test]
    fn resolves_a_paired_component_to_a_column_on_each_side() {
        let old = table! { "customer_id" => [1, 2] };
        let new = table! { "id" => [1, 2] };

        let key = resolve_key(&old, &new, &options(&["customer_id/id"])).unwrap();

        assert_eq!(key.basis, KeyBasis::Declared);
        assert_eq!(key.columns[0].component, "customer_id/id");
        assert_eq!((key.columns[0].old, key.columns[0].new), (0, 0));
    }

    #[test]
    fn a_pair_of_equal_names_needs_no_special_case() {
        let old = table! { "id" => [1] };

        let key = resolve_key(&old, &old, &options(&["id/id"])).unwrap();

        assert_eq!((key.columns[0].old, key.columns[0].new), (0, 0));
    }

    #[test]
    fn components_may_not_claim_a_column_twice() {
        let old = table! { "a" => [1], "b" => [1], "c" => [1] };

        // The old endpoint repeats, the new endpoint repeats, and a plain
        // component collides with the old half of a pair.
        for (key, side, column) in [
            (vec!["a/b", "a/c"], Side::Old, "a"),
            (vec!["a/b", "c/b"], Side::New, "b"),
            (vec!["a", "a/b"], Side::Old, "a"),
        ] {
            assert_eq!(
                resolve_key(&old, &old, &options(&key)).unwrap_err(),
                DiffError::DuplicateKeyColumn {
                    side,
                    column: column.into(),
                }
            );
        }
    }

    #[test]
    fn components_may_exchange_two_columns() {
        let old = table! { "a" => [1, 2], "b" => [3, 4] };
        let new = table! { "a" => [3, 4], "b" => [1, 2] };

        // Neither endpoint repeats, so the key is a legal pair of pairs.
        let key = resolve_key(&old, &new, &options(&["a/b", "b/a"])).unwrap();

        assert_eq!((key.columns[0].old, key.columns[0].new), (0, 1));
        assert_eq!((key.columns[1].old, key.columns[1].new), (1, 0));
        assert_eq!(key.old, key.new);
    }

    #[test]
    fn a_pair_reports_the_endpoint_that_is_missing() {
        let old = table! { "customer_id" => [1] };
        let new = table! { "other" => [1] };

        assert_eq!(
            resolve_key(&old, &new, &options(&["customer_id/id"])).unwrap_err(),
            DiffError::MissingKeyColumn {
                side: Side::New,
                component: "id".into(),
            }
        );
    }

    #[test]
    fn an_incompatible_pair_names_the_component_as_written() {
        let old = table! { "customer_id" => [true] };
        let new = table! { "id" => [1] };

        assert_eq!(
            resolve_key(&old, &new, &options(&["customer_id/id"])).unwrap_err(),
            DiffError::IncompatibleKeyTypes {
                component: "customer_id/id".into(),
                old_type: "Boolean".into(),
                new_type: "Int64".into(),
            }
        );
    }

    #[test]
    fn a_compound_key_may_mix_plain_and_paired_components() {
        let old = table! {
            "group" => ["a", "a"],
            "customer_id" => [1, 2],
        };
        let new = table! {
            "group" => ["a", "a"],
            "id" => [1, 2],
        };

        let key = resolve_key(&old, &new, &options(&["group", "customer_id/id"])).unwrap();

        assert_eq!(key.columns.len(), 2);
        assert_eq!(key.old, key.new);
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
    fn retains_a_key_that_fans_out_within_the_limit() {
        let old = table! { "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] };
        let new = table! { "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10] };

        let key = resolve_key(&old, &new, &options(&["id"])).unwrap();

        // One of ten shared keys is exactly the 10% limit, which is inclusive.
        assert_eq!(key.basis, KeyBasis::Declared);
        assert_eq!(key.new.len(), 11);
    }

    #[test]
    fn rejects_a_key_that_fans_out_above_the_limit() {
        let old = table! { "id" => [1, 2] };
        let new = table! { "id" => [1, 1, 2] };

        assert_eq!(
            resolve_key(&old, &new, &options(&["id"])).unwrap_err(),
            DiffError::ExcessiveFanout {
                affected: 1,
                shared: 2,
            }
        );
    }

    #[test]
    fn counts_each_fanned_out_key_once() {
        let old = table! { "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] };
        let new = table! { "id" => [1, 2, 3, 4, 4, 4, 5, 6, 7, 8, 9, 10] };

        // Three new rows for one key is still one affected key; counting rows
        // would make this 20% and reject it.
        assert!(resolve_key(&old, &new, &options(&["id"])).is_ok());
    }

    #[test]
    fn measures_fanout_against_shared_keys_rather_than_all_old_keys() {
        let old = table! { "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20] };
        let new = table! { "id" => [1, 2, 3, 4, 4, 5] };

        // One of five shared keys is 20% and rejects; one of twenty old keys
        // would be 5% and would wrongly retain.
        assert_eq!(
            resolve_key(&old, &new, &options(&["id"])).unwrap_err(),
            DiffError::ExcessiveFanout {
                affected: 1,
                shared: 5,
            }
        );
    }

    #[test]
    fn new_only_duplicates_are_additions_rather_than_fanout() {
        let old = table! { "id" => [1, 2] };
        let new = table! { "id" => [1, 2, 3, 3] };

        // A key absent from `old` has no row to fan out from, so however often
        // it repeats it cannot invalidate the declared key.
        assert!(resolve_key(&old, &new, &options(&["id"])).is_ok());
    }

    #[test]
    fn duplicates_without_any_shared_key_leave_the_key_valid() {
        let old = table! { "id" => [1] };
        let new = table! { "id" => [2, 2] };

        assert!(resolve_key(&old, &new, &options(&["id"])).is_ok());
    }

    #[test]
    fn old_side_duplication_is_fatal_even_when_new_fans_out() {
        let old = table! { "id" => [1, 1] };
        let new = table! { "id" => [1, 1] };

        assert_eq!(
            resolve_key(&old, &new, &options(&["id"])).unwrap_err(),
            DiffError::NonUniqueOldKey {
                first_row: 1,
                row: 2,
            }
        );
    }

    #[test]
    fn guessing_rejects_a_candidate_that_fans_out_too_broadly() {
        let old = table! {
            "id" => [1, 2],
            "other" => [5, 6],
        };
        let new = table! {
            "id" => [1, 1],
            "other" => [5, 6],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // "id" is the obvious identity and comes first, but its one shared key
        // is duplicated, and 100% is far above the bound.
        assert_eq!(key.columns[0].component, "other");
    }

    #[test]
    fn guesses_a_candidate_that_fans_out_when_it_is_the_only_one() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "status" => ["x", "x", "x", "x", "x", "x", "x", "x", "x", "x"],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
            "status" => ["x", "x", "x", "x", "x", "x", "x", "x", "x", "x", "x"],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // "status" repeats in `old` and can never identify rows, so the only
        // candidate left is one that fans out.
        assert_eq!(key.basis, KeyBasis::Guessed);
        assert_eq!(key.columns[0].component, "id");
        assert_eq!(
            key.overlap,
            Some(KeyOverlap {
                shared: 10,
                possible: 10,
            })
        );
    }

    #[test]
    fn guessing_prefers_more_shared_keys_over_freedom_from_fanout() {
        let old = table! {
            "a" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            "b" => [101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112],
        };
        let new = table! {
            "a" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            "b" => [101, 102, 103, 104, 105, 201, 202, 203, 204, 205, 206, 207, 208],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // "a" identifies twelve rows and duplicated one; "b" is spotless but
        // identifies five. The evidence wins.
        assert_eq!(key.columns[0].component, "a");
    }

    #[test]
    fn guessing_prefers_a_clean_candidate_only_to_break_a_tie() {
        let old = table! {
            "a" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "b" => [101, 102, 103, 104, 105, 106, 107, 108, 109, 110],
        };
        let new = table! {
            "a" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
            "b" => [101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 999],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // Both share ten keys and "a" comes first, so the tie-break is what
        // chooses the candidate that does not fan out.
        assert_eq!(key.columns[0].component, "b");
    }

    #[test]
    fn ranking_counts_distinct_keys_rather_than_matching_rows() {
        let old = table! {
            "a" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            "b" => [101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112],
        };
        let new = table! {
            "a" => [1, 2, 3, 4, 4, 4, 5, 6, 7, 8, 9, 10],
            "b" => [101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 999],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // "a" shares ten keys over twelve matching rows because one key repeats
        // three times; "b" shares eleven over eleven. Counting rows would pick
        // "a" whatever the column order, and counting keys picks "b".
        assert_eq!(key.columns[0].component, "b");
    }

    #[test]
    fn new_only_duplicates_do_not_disqualify_a_candidate() {
        let old = table! { "id" => [1, 2] };
        let new = table! { "id" => [1, 2, 3, 3] };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // Key 3 is absent from `old`, so its rows are additions rather than a
        // fanout and the candidate is unaffected by them.
        assert_eq!(key.columns[0].component, "id");
        assert_eq!(
            key.overlap,
            Some(KeyOverlap {
                shared: 2,
                possible: 2,
            })
        );
    }

    #[test]
    fn the_fanout_bound_applies_to_guesses() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "other" => [101, 102, 103, 104, 105, 106, 107, 108, 109, 110],
        };
        let new = table! {
            "id" => [1, 1, 2, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "other" => [101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 998, 999],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // Two of ten shared keys is 20%, so "id" is ineligible rather than
        // merely outranked; "other" shares the same ten keys cleanly.
        assert_eq!(key.columns[0].component, "other");
    }

    #[test]
    fn overlap_is_normalized_by_distinct_keys_not_row_counts() {
        let old = table! {
            "id" => [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20
            ],
        };
        let new = table! { "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10] };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // Eleven new rows hold ten distinct keys, all shared. Dividing by the
        // row counts would give 10/11; dividing by distinct keys gives 10/10.
        assert_eq!(
            key.overlap,
            Some(KeyOverlap {
                shared: 10,
                possible: 10,
            })
        );
    }

    #[test]
    fn a_compound_key_fans_out_on_the_whole_tuple() {
        let old = table! {
            "group" => ["a", "a", "a", "a", "a", "b", "b", "b", "b", "b"],
            "id" => [1, 2, 3, 4, 5, 1, 2, 3, 4, 5],
        };
        let new = table! {
            "group" => ["a", "a", "a", "a", "a", "b", "b", "b", "b", "b", "b"],
            "id" => ["1", "2", "3", "4", "5", "1", "2", "3", "3", "4", "5"],
        };

        let key = resolve_key(&old, &new, &options(&["group", "id"])).unwrap();

        // ("b", 3) is duplicated while ("a", 3) is not, so a rule that read one
        // component would count two affected keys and reject at 20%.
        assert_eq!(key.basis, KeyBasis::Declared);
        assert_eq!(key.columns.len(), 2);
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
        assert_eq!(key.columns[0].component, "id");
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

        assert_eq!(key.columns[0].component, "full");
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

        assert_eq!(key.columns[0].component, "b");
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
        assert_eq!(key.columns[0].component, "weak");
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

        assert_eq!(first.columns[0].component, second.columns[0].component);
        assert_eq!(first.overlap, second.overlap);
        assert_eq!(first.old, second.old);
        assert_eq!(first.new, second.new);
    }

    #[test]
    fn a_key_index_confirms_equality_within_a_bucket() {
        let keys = vec![
            vec![CanonicalValue::Int(1)],
            vec![CanonicalValue::Int(2)],
            vec![CanonicalValue::Int(1)],
        ];
        let index = KeyIndex::with_hash(&keys, |_| 0);

        // Every key shares one bucket, so only the confirmation step can
        // separate them, and the rows stay in ascending order.
        assert_eq!(index.rows(&keys[0]).collect::<Vec<_>>(), [0, 2]);
        assert_eq!(index.rows(&keys[1]).collect::<Vec<_>>(), [1]);
        assert!(index.rows(&[CanonicalValue::Int(3)]).next().is_none());
    }

    #[test]
    fn forced_hash_collisions_cannot_fake_duplicates_or_overlap() {
        let constant = |_: &CanonicalValue| 0_u128;
        let old = vec![CanonicalValue::Int(1), CanonicalValue::Int(2)];
        let new = vec![CanonicalValue::Int(2), CanonicalValue::Int(3)];

        let expected = Overlap {
            shared: 1,
            affected: 0,
            distinct_new: 2,
        };
        assert_eq!(candidate_overlap(&old, &new, constant), Some(expected));
        assert_eq!(candidate_overlap(&old, &new, stable_hash), Some(expected));
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
    fn overlap_counts_distinct_keys_rather_than_matching_rows() {
        let old = vec![CanonicalValue::Int(1), CanonicalValue::Int(2)];
        let new = vec![
            CanonicalValue::Int(1),
            CanonicalValue::Int(1),
            CanonicalValue::Int(1),
            CanonicalValue::Int(2),
            CanonicalValue::Int(9),
            CanonicalValue::Int(9),
        ];

        // Key 1 matches three new rows and key 9 is a new-only duplicate, so a
        // row count would report five shared and one affected key would be
        // invisible.
        assert_eq!(
            candidate_overlap(&old, &new, stable_hash),
            Some(Overlap {
                shared: 2,
                affected: 1,
                distinct_new: 3,
            })
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
