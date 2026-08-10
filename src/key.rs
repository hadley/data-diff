use std::collections::HashSet;

use std::cmp::Reverse;

use crate::compare::{CanonicalValue, ComparisonPlan, sequence_hash, stable_hash};
use crate::maps::DigestMap;
use crate::schema::ColumnMap;
use crate::{
    Budgets, DiffError, IdentityBasis, KeyBasis, KeyComponent, KeyOverlap, KeyRejection,
    KeySubject, RejectionReason, RowBudget, Side,
};
use arrow_array::RecordBatch;
use arrow_schema::Schema;

#[derive(Clone, Debug)]
pub(crate) struct ResolvedKey {
    pub basis: KeyBasis,
    pub columns: Vec<KeyColumn>,
    pub old: KeyValues,
    pub new: KeyValues,
    pub overlap: Option<KeyOverlap>,
    pub rejection: Option<KeyRejection>,
    /// Whether the guess behind this key ran out of budget before it examined
    /// every candidate.
    ///
    /// Carried on the key rather than reported at the search, because the
    /// exhaustion outlives the guess: a cut-short search that found nothing
    /// still leaves a fallback whose story includes the candidates that went
    /// unexamined, and the pass that keeps the key is the one that reports it.
    pub exhausted: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct KeyColumn {
    pub old: usize,
    pub new: usize,
}

/// One side's key tuples: a flat width-strided store with each row's digest
/// computed once at construction.
///
/// Row `i` is `values[i * width .. (i + 1) * width]`. The digests are the
/// same `sequence_hash` every consumer used to recompute — index bucketing,
/// row matching, and sample selection now all read them from here, so each
/// tuple is hashed exactly once per side per pass. Storing rows flat rather
/// than as one heap `Vec` each is the other half of the same bill.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KeyValues {
    width: usize,
    values: Vec<CanonicalValue>,
    digests: Vec<u128>,
}

impl KeyValues {
    fn new(width: usize, values: Vec<CanonicalValue>) -> Self {
        Self::with_hash(width, values, sequence_hash)
    }

    /// The hash is injectable so a test can force every digest to collide,
    /// which is the only way to reach the equality confirmations and
    /// tie-breaks behind the digests.
    pub(crate) fn with_hash(
        width: usize,
        values: Vec<CanonicalValue>,
        hash: fn(&[CanonicalValue]) -> u128,
    ) -> Self {
        assert!(width > 0, "a key has at least one component");
        let digests = values.chunks_exact(width).map(hash).collect();
        Self {
            width,
            values,
            digests,
        }
    }

    /// Interleave per-component columns into rows. The single-component key —
    /// the common case — moves its column in whole instead of cloning it
    /// value by value.
    fn from_columns(mut columns: Vec<Vec<CanonicalValue>>, rows: usize) -> Self {
        let width = columns.len();
        if width == 1 {
            return Self::new(1, columns.pop().expect("width is one"));
        }
        let mut values = Vec::with_capacity(rows * width);
        for row in 0..rows {
            values.extend(columns.iter().map(|column| column[row].clone()));
        }
        Self::new(width, values)
    }

    pub(crate) fn len(&self) -> usize {
        self.digests.len()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.digests.is_empty()
    }

    pub(crate) fn row(&self, row: usize) -> &[CanonicalValue] {
        &self.values[row * self.width..(row + 1) * self.width]
    }

    pub(crate) fn digest(&self, row: usize) -> u128 {
        self.digests[row]
    }

    /// The rows as owned tuples, for asserting against literal expectations.
    #[cfg(test)]
    pub(crate) fn tuples(&self) -> Vec<Vec<CanonicalValue>> {
        self.values
            .chunks_exact(self.width)
            .map(<[CanonicalValue]>::to_vec)
            .collect()
    }

    /// The same rows under a different digest, for tests that force
    /// collisions after resolution built the real thing.
    #[cfg(test)]
    pub(crate) fn rehashed(mut self, hash: fn(&[CanonicalValue]) -> u128) -> Self {
        self.digests = self.values.chunks_exact(self.width).map(hash).collect();
        self
    }
}

/// The share of common key values a declared key may duplicate in `new` and
/// still be read as fanout rather than as a broken key.
///
/// `RejectionReason::ExcessiveFanout` carries the counts it was measured from.
pub(crate) const MAX_FANOUT_PERCENT: usize = 10;

/// Rows grouped by digest into one map entry and three flat vectors.
///
/// A digest only chooses the bucket; every consumer confirms equality on the
/// bucket's members, so a collision can never decide membership. The layout is
/// what keeps bulk construction cheap: a heap `Vec` per bucket would cost a
/// mostly-unique column an allocation per row, where this costs a handful
/// however the digests fall. Bucket ids are assigned in first-occurrence
/// order, so iterating buckets in id order is deterministic without reading
/// the map's own order.
struct DigestIndex {
    /// Digest to bucket id, ids assigned in first-occurrence order.
    buckets: DigestMap<usize>,
    /// Bucket id to its range in `rows`; one final entry closes the last.
    offsets: Vec<usize>,
    /// Row indices grouped by bucket, ascending within each bucket.
    rows: Vec<usize>,
}

impl DigestIndex {
    fn new(digests: impl ExactSizeIterator<Item = u128>) -> Self {
        // Pre-sized to the worst case of all-distinct digests, and each row's
        // bucket id is remembered from this pass, so no digest is ever looked
        // up twice and the map never rehashes as it grows. Taking the digests
        // as an iterator spares every caller a materialized digest vector.
        let mut buckets = DigestMap::with_capacity_and_hasher(digests.len(), Default::default());
        let mut counts: Vec<usize> = Vec::new();
        let mut ids = Vec::with_capacity(digests.len());
        for digest in digests {
            let next = counts.len();
            let id = *buckets.entry(digest).or_insert(next);
            if id == next {
                counts.push(0);
            }
            counts[id] += 1;
            ids.push(id);
        }
        let mut offsets = Vec::with_capacity(counts.len() + 1);
        let mut total = 0;
        offsets.push(0);
        for count in &counts {
            total += count;
            offsets.push(total);
        }
        // Per-bucket write cursors; visiting rows in ascending order keeps
        // every bucket ascending.
        let mut cursors: Vec<usize> = offsets[..counts.len()].to_vec();
        let mut rows = vec![0; ids.len()];
        for (row, &id) in ids.iter().enumerate() {
            rows[cursors[id]] = row;
            cursors[id] += 1;
        }
        Self {
            buckets,
            offsets,
            rows,
        }
    }

    /// The rows in `digest`'s bucket, in ascending row order; a digest match,
    /// not yet an equality.
    fn rows(&self, digest: u128) -> &[usize] {
        self.buckets.get(&digest).map_or(&[], |&id| {
            &self.rows[self.offsets[id]..self.offsets[id + 1]]
        })
    }

    /// Every bucket, in first-occurrence order.
    fn buckets(&self) -> impl Iterator<Item = &[usize]> {
        self.offsets
            .windows(2)
            .map(|window| &self.rows[window[0]..window[1]])
    }
}

/// Rows grouped by key digest, with equality confirmed on lookup.
///
/// A bucket can hold rows with different keys, so a collision must never decide
/// membership; confirming equality inside the bucket is what key validation and
/// row matching both need, and sharing it keeps that reasoning in one place.
/// The digests come precomputed from [`KeyValues`], so building the index and
/// looking rows up never hashes a tuple.
pub(crate) struct KeyIndex<'a> {
    keys: &'a KeyValues,
    index: DigestIndex,
}

impl<'a> KeyIndex<'a> {
    pub(crate) fn new(keys: &'a KeyValues) -> Self {
        Self {
            keys,
            index: DigestIndex::new(keys.digests.iter().copied()),
        }
    }

    /// The rows whose key equals `key`, in ascending row order.
    ///
    /// The digest accompanies the key rather than being recomputed here; it
    /// only chooses the bucket, and the equality filter decides membership, so
    /// a collision cannot manufacture a match.
    pub(crate) fn rows<'b>(
        &'b self,
        key: &'b [CanonicalValue],
        digest: u128,
    ) -> impl Iterator<Item = usize> + 'b {
        self.index
            .rows(digest)
            .iter()
            .copied()
            .filter(move |&row| self.keys.row(row) == key)
    }
}

/// Resolve the key, falling back until something can identify rows.
///
/// Declared, then guessed, then position. A declared key this data cannot
/// support is a rejection rather than an error, so the comparison continues
/// and the rejection travels with the key that replaced it. Nothing below here
/// learns which attempt won except through `ResolvedKey`.
pub(crate) fn resolve_key(
    old: &RecordBatch,
    new: &RecordBatch,
    declared: &Declared,
    hinted: &ColumnMap,
    budgets: &Budgets,
) -> ResolvedKey {
    let rejection = match declared {
        Declared::Positional => return positional_key(old, new, KeyBasis::Declared),
        Declared::Components(components) => match declared_key(old, new, components, hinted) {
            Ok(key) => return key,
            Err(rejection) => Some(rejection),
        },
        Declared::Guess => None,
    };

    let guess = guess_key(old, new, hinted, &[], budgets);
    let exhausted = guess.exhausted;
    let mut key = guess
        .key
        .unwrap_or_else(|| positional_key(old, new, KeyBasis::Fallback));
    key.exhausted = exhausted;
    key.rejection = rejection;
    key
}

/// The key that matches rows by position.
///
/// Row positions satisfy everything `match_rows` assumes of a key — distinct,
/// so unique in `old` and incapable of fanout; never null or `NaN`; and equal
/// across sides exactly when the positions are equal — so positional matching
/// is the ordinary algorithm over these values rather than a path beside it.
pub(crate) fn positional_key(old: &RecordBatch, new: &RecordBatch, basis: KeyBasis) -> ResolvedKey {
    fn positions(rows: usize) -> KeyValues {
        KeyValues::new(
            1,
            (0..rows)
                .map(|row| CanonicalValue::Int(row as i64))
                .collect(),
        )
    }

    ResolvedKey {
        basis,
        columns: Vec::new(),
        old: positions(old.num_rows()),
        new: positions(new.num_rows()),
        overlap: None,
        rejection: None,
        exhausted: false,
    }
}

/// Resolve and validate the components the user declared.
///
/// Resolution and validation share one result because the split between them is
/// not the split between fatal and recoverable: a missing column is discovered
/// while resolving and a duplicate while validating, and both are a key this
/// data cannot support. Only parsing, above this, still fails outright.
fn declared_key(
    old: &RecordBatch,
    new: &RecordBatch,
    components: &[Component],
    hinted: &ColumnMap,
) -> Result<ResolvedKey, KeyRejection> {
    let mut columns = Vec::with_capacity(components.len());
    let mut old_components = Vec::with_capacity(components.len());
    let mut new_components = Vec::with_capacity(components.len());

    for component in components {
        // Each endpoint is resolved on its own side, so a missing column is
        // reported as the half that is missing rather than as the whole pair.
        let (old_index, new_index) = component_endpoints(old, new, component, hinted)?;
        let old_values = old.column(old_index);
        let new_values = new.column(new_index);
        let plan = ComparisonPlan::new(old_values.data_type(), new_values.data_type()).ok_or_else(
            || {
                component.rejected(RejectionReason::IncompatibleTypes {
                    old_type: format!("{:?}", old_values.data_type()),
                    new_type: format!("{:?}", new_values.data_type()),
                })
            },
        )?;
        old_components.push(plan.canonicalize_old(old_values.as_ref()));
        new_components.push(plan.canonicalize_new(new_values.as_ref()));
        columns.push(KeyColumn {
            old: old_index,
            new: new_index,
        });
    }

    // Uniqueness is checked again on the resolved coordinates, not only on the
    // names. Two components can name different columns and land on the same
    // one: `--key id,customer_id` with a `customer_id -> id` hint resolves both
    // through that identity, which the name check cannot see.
    validate_distinct(&columns, components)?;

    let old_keys = KeyValues::from_columns(old_components, old.num_rows());
    let new_keys = KeyValues::from_columns(new_components, new.num_rows());
    validate_present(&old_keys, components, Side::Old)?;
    validate_present(&new_keys, components, Side::New)?;
    // Uniqueness and fanout are properties of the tuple rather than of any one
    // component, so they blame the declared key entire.
    let whole = || KeySubject::Key(components.iter().map(Component::named).collect());
    validate_unique_old(&old_keys).map_err(|reason| KeyRejection {
        subject: whole(),
        reason,
    })?;
    validate_fanout(&old_keys, &KeyIndex::new(&new_keys)).map_err(|reason| KeyRejection {
        subject: whole(),
        reason,
    })?;

    Ok(ResolvedKey {
        basis: KeyBasis::Declared,
        columns,
        old: old_keys,
        new: new_keys,
        overlap: None,
        rejection: None,
        exhausted: false,
    })
}

/// The identities a declared key asserts on its own, claimed into a fresh map.
///
/// A component claims an identity even when it names one column: `id` claims
/// that old `id` and new `id` are the same column, which a hint can contradict
/// just as a paired component can. Claiming these before hints are considered
/// is what settles the precedence between them — a key decides how every row is
/// matched, so an ignored hint is much the better failure — and it leaves the
/// contest itself to `ColumnMap`, which refuses a spent endpoint whoever asks.
///
/// A component claims only where it can. One naming a column that a side does
/// not have asserts nothing, because it cannot be resolved by name at all: that
/// is the case a hint is there to settle, and treating it as a claim would have
/// the key contradicting the very hint it depends on.
pub(crate) fn claimed_identities(
    old: &Schema,
    new: &Schema,
    components: &[Component],
) -> ColumnMap {
    let mut map = ColumnMap::new(old, new);
    for component in components {
        if let (Some(old_index), Some(new_index)) = (
            schema_position(old, &component.old),
            schema_position(new, &component.new),
        ) {
            map.claim(old_index, new_index, IdentityBasis::Declared);
        }
    }
    map
}

fn schema_position(schema: &Schema, name: &str) -> Option<usize> {
    schema
        .fields()
        .iter()
        .position(|field| field.name() == name)
}

#[cfg(test)]
pub(crate) mod testing {
    use arrow_array::RecordBatch;

    use super::{ColumnMap, ResolvedKey, declared_components};
    use crate::{DiffError, DiffOptions, KeyRejection};

    /// Resolve a key from options alone, with no hints in play.
    ///
    /// Reconciliation resolves hints first and passes them in. Keeping this
    /// under the same name spares every test that predates hints from
    /// restating "and no hints" at each of its call sites.
    ///
    /// The `Err` is now only a `--key` string that could not be read;
    /// everything the data refuses arrives as `ResolvedKey::rejection` on the
    /// key that replaced it.
    pub(crate) fn resolve_key(
        old: &RecordBatch,
        new: &RecordBatch,
        options: &DiffOptions,
    ) -> Result<ResolvedKey, DiffError> {
        let declared = declared_components(&options.key)?;
        let map = ColumnMap::new(old.schema_ref(), new.schema_ref());
        Ok(super::resolve_key(
            old,
            new,
            &declared,
            &map,
            &options.budgets,
        ))
    }

    pub(crate) fn rejection(
        old: &RecordBatch,
        new: &RecordBatch,
        key: &[&str],
    ) -> Option<KeyRejection> {
        let options = DiffOptions {
            key: key.iter().map(|name| (*name).to_owned()).collect(),
            ..DiffOptions::default()
        };
        resolve_key(old, new, &options).unwrap().rejection
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
    hinted: &ColumnMap,
) -> Result<(usize, usize), KeyRejection> {
    let missing = |side| component.rejected(RejectionReason::MissingColumn { side });
    let old_found = position(old, &component.old);
    let new_found = position(new, &component.new);
    let old_index = match (old_found, new_found) {
        (Some(index), _) => index,
        (None, Some(new_index)) => hinted
            .old_for_new(new_index)
            .ok_or_else(|| missing(Side::Old))?,
        (None, None) => return Err(missing(Side::Old)),
    };
    let new_index = match new_found {
        Some(index) => index,
        None => hinted
            .new_for_old(old_index)
            .ok_or_else(|| missing(Side::New))?,
    };
    Ok((old_index, new_index))
}

/// Reject a key whose components resolved to the same column twice.
fn validate_distinct(columns: &[KeyColumn], components: &[Component]) -> Result<(), KeyRejection> {
    let mut old_seen = HashSet::new();
    let mut new_seen = HashSet::new();
    for (column, component) in columns.iter().zip(components) {
        for (side, seen, index) in [
            (Side::Old, &mut old_seen, column.old),
            (Side::New, &mut new_seen, column.new),
        ] {
            if !seen.insert(index) {
                return Err(component.rejected(RejectionReason::DuplicateColumn { side }));
            }
        }
    }
    Ok(())
}

fn position(table: &RecordBatch, name: &str) -> Option<usize> {
    table
        .schema()
        .fields()
        .iter()
        .position(|field| field.name() == name)
}

/// The least a proportional `key_rows` budget resolves to.
///
/// Enough to fund the whole lattice of a small table however many columns it
/// has — the per-candidate costs there are a handful of rows each, so the
/// floor covers thousands of candidates — while adding nothing at sizes where
/// the proportional allowance already dwarfs it.
pub(crate) const KEY_ROWS_FLOOR: usize = 65_536;

/// A guess and whether the search behind it was exhaustive.
///
/// The two travel together because exhaustion outlives the guess: a search cut
/// short that found nothing still leaves a fallback whose story includes the
/// candidates that went unexamined, so the caller copies `exhausted` onto
/// whichever key it ends up keeping.
pub(crate) struct Guess {
    pub key: Option<ResolvedKey>,
    pub exhausted: bool,
}

/// Select the eligible candidate — one identified column or a combination of
/// them — that shares the most key tuples.
///
/// Ranking follows the evidence, then parsimony: most shared tuples, then
/// fewer columns, then freedom from fanout, then old-side column order. A
/// single column keeps its bounded new-side fanout allowance; a wider
/// candidate must be unique in both sides, so the fanout tie-break can only
/// ever separate single columns, and a guessed compound key is incapable of
/// fanout by construction.
///
/// The search is a breadth-first walk of the column-combination lattice in the
/// HyUCC family, bounded three ways: `key_width` defines the space, and
/// `key_rows` and `key_candidates` meter the work of covering it. On
/// exhaustion the best candidate already examined wins, which is the useful
/// partial result, and `Guess::exhausted` says the search was cut short.
///
/// `excluded` holds candidates a caller has already tried and withdrawn, each
/// an exact column-set: a retracted guess must not be guessed again, while its
/// individual columns stay available to other combinations. The map, not this
/// function, is how reconsideration widens the field: an identity inference
/// established makes its pair a candidate here exactly as a hinted identity
/// always has.
pub(crate) fn guess_key(
    old: &RecordBatch,
    new: &RecordBatch,
    hinted: &ColumnMap,
    excluded: &[Vec<(usize, usize)>],
    budgets: &Budgets,
) -> Guess {
    let none = Guess {
        key: None,
        exhausted: false,
    };
    if old.num_rows() == 0 || new.num_rows() == 0 || budgets.key_width == 0 {
        return none;
    }
    let mut pool = eligible_columns(old, new, hinted);
    if pool.is_empty() {
        return none;
    }

    // Key guessing runs before any rows are matched, so the proportional
    // budget resolves against the cells the unavoidable input pass reads —
    // and it keeps a floor, because the lattice's cost scales with column
    // combinations where the yardstick scales with cells, and a small wide
    // table would otherwise be refused a search that costs almost nothing in
    // absolute terms. The absolute form stays exact, so tests can meter to
    // the row.
    let cells = old
        .num_rows()
        .saturating_mul(old.num_columns())
        .saturating_add(new.num_rows().saturating_mul(new.num_columns()));
    let rows = match budgets.key_rows {
        RowBudget::PerCell(_) => budgets.key_rows.resolve(cells).max(KEY_ROWS_FLOOR),
        RowBudget::Rows(rows) => rows,
    };
    let mut meter = Meter {
        rows,
        candidates: budgets.key_candidates,
        exhausted: false,
    };

    let best = search(&pool, excluded, budgets.key_width, &mut meter);
    let Some(best) = best else {
        return Guess {
            key: None,
            exhausted: meter.exhausted,
        };
    };

    // Only the winner materializes key tuples; its component columns move out
    // of the pool rather than being cloned.
    let columns = best
        .columns
        .iter()
        .map(|&column| KeyColumn {
            old: pool[column].old_index,
            new: pool[column].new_index,
        })
        .collect();
    let old_components = best
        .columns
        .iter()
        .map(|&column| std::mem::take(&mut pool[column].old.values))
        .collect();
    let new_components = best
        .columns
        .iter()
        .map(|&column| std::mem::take(&mut pool[column].new.values))
        .collect();
    Guess {
        key: Some(ResolvedKey {
            basis: KeyBasis::Guessed,
            columns,
            old: KeyValues::from_columns(old_components, old.num_rows()),
            new: KeyValues::from_columns(new_components, new.num_rows()),
            rejection: None,
            overlap: Some(best.overlap),
            exhausted: false,
        }),
        exhausted: meter.exhausted,
    }
}

/// One identified, comparable column pair whose values could ever key a row.
struct PoolColumn {
    old_index: usize,
    new_index: usize,
    old: SideColumn,
    new: SideColumn,
}

impl PoolColumn {
    fn side(&self, side: Side) -> &SideColumn {
        match side {
            Side::Old => &self.old,
            Side::New => &self.new,
        }
    }
}

/// One side of a pool column: its canonical values, their digests, and the
/// rows it duplicates.
struct SideColumn {
    values: Vec<CanonicalValue>,
    digests: Vec<u128>,
    /// The rows sharing each duplicated value; empty exactly when the column
    /// is unique on this side.
    clusters: Vec<Vec<usize>>,
}

impl SideColumn {
    fn new(values: Vec<CanonicalValue>) -> Self {
        Self::with_hash(values, stable_hash)
    }

    /// The hash is injectable so a test can force every digest to collide,
    /// which is the only way to prove the equality confirmations behind the
    /// digests keep colliding values apart.
    fn with_hash(values: Vec<CanonicalValue>, hash: fn(&CanonicalValue) -> u128) -> Self {
        let digests: Vec<u128> = values.iter().map(hash).collect();
        let all: Vec<usize> = (0..values.len()).collect();
        let clusters = duplicate_clusters(&all, &values, &digests);
        Self {
            values,
            digests,
            clusters,
        }
    }

    fn unique(&self) -> bool {
        self.clusters.is_empty()
    }

    /// Distinct values on this side: every row, minus each cluster's rows
    /// beyond its first.
    fn distinct(&self) -> usize {
        self.values.len()
            - self
                .clusters
                .iter()
                .map(|cluster| cluster.len() - 1)
                .sum::<usize>()
    }
}

/// The identified, comparable column pairs whose values could ever key a row,
/// in old-side column order.
///
/// A column with a null or `NaN` anywhere on either side leaves here, before
/// the lattice exists: no candidate of any width may contain a missing value,
/// so such a column can take part in nothing the search enumerates.
///
/// Like the projections behind rename inference, the pool holds one canonical
/// copy of each eligible column per side — the search reads values throughout,
/// so the bill is paid once, linear in cells.
fn eligible_columns(old: &RecordBatch, new: &RecordBatch, hinted: &ColumnMap) -> Vec<PoolColumn> {
    let new_schema = new.schema();
    let mut pool = Vec::new();
    for (old_index, old_field) in old.schema().fields().iter().enumerate() {
        // An identified column, which a hint may have identified across a
        // rename; a name whose counterpart a hint claimed for another column is
        // not this column's to use.
        let by_name = new_schema
            .fields()
            .iter()
            .position(|field| field.name() == old_field.name())
            .filter(|&index| {
                hinted
                    .old_for_new(index)
                    .is_none_or(|owner| owner == old_index)
            });
        let Some(new_index) = hinted.new_for_old(old_index).or(by_name) else {
            continue;
        };
        let old_column = old.column(old_index);
        let new_column = new.column(new_index);
        let Some(plan) = ComparisonPlan::new(old_column.data_type(), new_column.data_type()) else {
            continue;
        };
        let old_values = plan.canonicalize_old(old_column.as_ref());
        let new_values = plan.canonicalize_new(new_column.as_ref());
        if old_values
            .iter()
            .chain(new_values.iter())
            .any(CanonicalValue::invalid_key)
        {
            continue;
        }
        pool.push(PoolColumn {
            old_index,
            new_index,
            old: SideColumn::new(old_values),
            new: SideColumn::new(new_values),
        });
    }
    pool
}

/// Group the rows among `rows` that share a value, in deterministic order.
///
/// Digests choose buckets and equality decides membership, as everywhere else,
/// so a collision cannot merge two values into one cluster. Buckets arrive in
/// first-occurrence order from the flat index, so the result is a pure
/// function of the input, and only a genuinely duplicated value — never a
/// singleton — costs a cluster allocation.
fn duplicate_clusters(
    rows: &[usize],
    values: &[CanonicalValue],
    digests: &[u128],
) -> Vec<Vec<usize>> {
    let index = DigestIndex::new(rows.iter().map(|&row| digests[row]));
    let mut result = Vec::new();
    for bucket in index.buckets() {
        if bucket.len() < 2 {
            continue;
        }
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for &position in bucket {
            let row = rows[position];
            match groups
                .iter_mut()
                .find(|group| values[group[0]] == values[row])
            {
                Some(group) => group.push(row),
                None => groups.push(vec![row]),
            }
        }
        result.extend(groups.into_iter().filter(|group| group.len() > 1));
    }
    result
}

/// The search's two counted allowances, spent in enumeration order.
///
/// Exhaustion is sticky: the first charge the remainder cannot fund kills the
/// meter, so the unexamined candidates are one deterministic tail of the
/// enumeration. A zero-cost charge always succeeds, so empty inputs can never
/// exhaust anything.
struct Meter {
    rows: usize,
    candidates: usize,
    exhausted: bool,
}

impl Meter {
    /// Admit one candidate to the lattice, or exhaust.
    fn admit(&mut self) -> bool {
        if self.exhausted || self.candidates == 0 {
            self.exhausted = true;
            return false;
        }
        self.candidates -= 1;
        true
    }

    /// Fund an examination that reads this many rows, or exhaust.
    fn charge(&mut self, rows: usize) -> bool {
        if self.exhausted || rows > self.rows {
            self.exhausted = true;
            return false;
        }
        self.rows -= rows;
        true
    }
}

/// The winning candidate: its pool columns and the overlap it reports.
struct BestCandidate {
    columns: Vec<usize>,
    rank: (usize, Reverse<usize>, bool),
    overlap: KeyOverlap,
}

/// One lattice node still worth extending: no subset of it can be a key, and
/// each side's duplicate clusters carry the evidence forward for refinement.
struct Extendable {
    /// Pool indexes, ascending; extension appends columns past the last, so
    /// every combination is enumerated exactly once, in prefix order.
    columns: Vec<usize>,
    /// Empty exactly when the tuple is unique on that side.
    old_clusters: Vec<Vec<usize>>,
    new_clusters: Vec<Vec<usize>>,
}

/// Walk the lattice breadth-first and return the best eligible candidate.
///
/// Width 1 is today's single-column scan restated: unique in `old`, bounded
/// fanout in `new`, at least one shared value. Wider candidates extend only
/// combinations that failed uniqueness on at least one side, because two
/// prunes close every other door. A superset of an *eligible* candidate can
/// never win — it shares at most as many tuples, since every shared wider
/// tuple projects to a shared narrower one, and it loses the fewer-columns
/// tie-break — and a superset of a unique-both-sides candidate sharing nothing
/// shares nothing itself. Both are `terminal`.
///
/// The agree sets are the search's HyUCC-style row evidence: a failed
/// uniqueness validation records every pool column its first duplicate pair
/// agrees on, and any later candidate inside that set is known non-unique on
/// that side without reading a row. At the final width, where no child needs
/// clusters, that answer is the whole cost of the candidate.
fn search(
    pool: &[PoolColumn],
    excluded: &[Vec<(usize, usize)>],
    key_width: usize,
    meter: &mut Meter,
) -> Option<BestCandidate> {
    let rows_old = pool[0].old.values.len();
    let rows_new = pool[0].new.values.len();
    let mut best: Option<BestCandidate> = None;
    // Candidates whose supersets are never generated: the eligible and the
    // dead. Excluded eligible candidates count too — a superset of a retracted
    // key inherits its condemned matching minus rows, which can only read as
    // more of a rewrite, not less.
    let mut terminal: Vec<Vec<usize>> = Vec::new();
    let mut old_agree: Vec<Vec<usize>> = Vec::new();
    let mut new_agree: Vec<Vec<usize>> = Vec::new();
    let mut frontier: Vec<Extendable> = Vec::new();

    // Larger is better, and enumeration order — width ascending, columns
    // lexicographic — settles complete ties in favor of the earliest.
    let rank =
        |shared: usize, width: usize, affected: usize| (shared, Reverse(width), affected == 0);
    let is_excluded = |columns: &[usize]| {
        let pairs: Vec<(usize, usize)> = columns
            .iter()
            .map(|&column| (pool[column].old_index, pool[column].new_index))
            .collect();
        excluded.contains(&pairs)
    };

    // Width 1: uniqueness fell out of the pool's construction, so the only
    // charged work is the overlap measurement.
    for (index, column) in pool.iter().enumerate() {
        if !meter.admit() {
            break;
        }
        if !column.old.unique() {
            // Never eligible at this width — old-side duplication has no
            // allowance — but one more column can cure it.
            record_agree(pool, Side::Old, &column.old.clusters, &mut old_agree, meter);
            if !column.new.unique() {
                record_agree(pool, Side::New, &column.new.clusters, &mut new_agree, meter);
            }
            if meter.exhausted {
                break;
            }
            if key_width > 1 {
                frontier.push(Extendable {
                    columns: vec![index],
                    old_clusters: column.old.clusters.clone(),
                    new_clusters: column.new.clusters.clone(),
                });
            }
            continue;
        }
        if !meter.charge(rows_old + rows_new) {
            break;
        }
        let (shared, affected) = overlap_of(pool, &[index]);
        if shared > 0 && within_fanout_limit(affected, shared) {
            terminal.push(vec![index]);
            if !is_excluded(&[index]) {
                let candidate = BestCandidate {
                    columns: vec![index],
                    rank: rank(shared, 1, affected),
                    overlap: KeyOverlap {
                        shared,
                        // Distinct keys on each side. `old` is unique, so its
                        // distinct count is its row count; `new`'s is smaller
                        // than its row count exactly when it duplicates one.
                        possible: rows_old.min(column.new.distinct()),
                    },
                };
                if best.as_ref().is_none_or(|best| candidate.rank > best.rank) {
                    best = Some(candidate);
                }
            }
        } else if shared == 0 {
            // `old` is unique here, so a wider tuple shares at most what this
            // one shares: nothing. Terminal rather than extendable.
            terminal.push(vec![index]);
        } else {
            // Fanout beyond the allowance: new-side duplication one more
            // column can cure.
            record_agree(pool, Side::New, &column.new.clusters, &mut new_agree, meter);
            if meter.exhausted {
                break;
            }
            if key_width > 1 {
                frontier.push(Extendable {
                    columns: vec![index],
                    old_clusters: Vec::new(),
                    new_clusters: column.new.clusters.clone(),
                });
            }
        }
    }

    let mut width = 1;
    while width < key_width && !frontier.is_empty() && !meter.exhausted {
        width += 1;
        let last = width == key_width;
        let mut next = Vec::new();
        'level: for parent in &frontier {
            let extension = parent.columns.last().copied().expect("no empty candidate") + 1;
            for column in extension..pool.len() {
                let mut columns = parent.columns.clone();
                columns.push(column);
                if terminal.iter().any(|set| subset(set, &columns)) {
                    continue;
                }
                if !meter.admit() {
                    break 'level;
                }
                let Some(old_clusters) = validate_side(
                    pool,
                    Side::Old,
                    &columns,
                    &parent.old_clusters,
                    &mut old_agree,
                    last,
                    meter,
                ) else {
                    break 'level;
                };
                // At the final width nothing extends, so a candidate already
                // ineligible on the old side is done without touching `new`.
                if last && !old_clusters.unique {
                    continue;
                }
                let Some(new_clusters) = validate_side(
                    pool,
                    Side::New,
                    &columns,
                    &parent.new_clusters,
                    &mut new_agree,
                    last,
                    meter,
                ) else {
                    break 'level;
                };
                if old_clusters.unique && new_clusters.unique {
                    // Unique in both sides: eligible exactly when it shares a
                    // tuple, dead either way for every superset.
                    if !meter.charge(rows_old + rows_new) {
                        break 'level;
                    }
                    let (shared, _) = overlap_of(pool, &columns);
                    terminal.push(columns.clone());
                    if shared > 0 && !is_excluded(&columns) {
                        let candidate = BestCandidate {
                            rank: rank(shared, width, 0),
                            overlap: KeyOverlap {
                                shared,
                                possible: rows_old.min(rows_new),
                            },
                            columns,
                        };
                        if best.as_ref().is_none_or(|best| candidate.rank > best.rank) {
                            best = Some(candidate);
                        }
                    }
                } else if !last {
                    next.push(Extendable {
                        columns,
                        old_clusters: old_clusters.clusters,
                        new_clusters: new_clusters.clusters,
                    });
                }
            }
        }
        frontier = next;
    }
    best
}

/// One side's answer for one candidate: whether the tuple is unique there,
/// with the surviving duplicate clusters when the caller still needs them.
struct SideValidation {
    unique: bool,
    clusters: Vec<Vec<usize>>,
}

/// Validate one side of a candidate, or return `None` on exhaustion.
///
/// A parent unique on this side stays unique under any extension, free. A
/// candidate inside a recorded agree set is known duplicated without reading a
/// row, which at the final width — no child needs the clusters — is the whole
/// cost. Everything else pays for its refinement: only the parent's duplicate
/// cluster rows are re-examined, so the cost concentrates exactly where
/// duplication does.
fn validate_side(
    pool: &[PoolColumn],
    side: Side,
    columns: &[usize],
    parent_clusters: &[Vec<usize>],
    agree: &mut Vec<Vec<usize>>,
    last: bool,
    meter: &mut Meter,
) -> Option<SideValidation> {
    if parent_clusters.is_empty() {
        return Some(SideValidation {
            unique: true,
            clusters: Vec::new(),
        });
    }
    let known_duplicated = agree.iter().any(|set| subset(columns, set));
    if known_duplicated && last {
        return Some(SideValidation {
            unique: false,
            clusters: Vec::new(),
        });
    }
    let rows: usize = parent_clusters.iter().map(Vec::len).sum();
    if !meter.charge(rows) {
        return None;
    }
    let added = pool[*columns.last().expect("no empty candidate")].side(side);
    let mut clusters = Vec::new();
    for cluster in parent_clusters {
        clusters.extend(duplicate_clusters(cluster, &added.values, &added.digests));
    }
    if !clusters.is_empty() && !known_duplicated {
        record_agree(pool, side, &clusters, agree, meter);
        if meter.exhausted {
            return None;
        }
    }
    Some(SideValidation {
        unique: clusters.is_empty(),
        clusters,
    })
}

/// Record the agree set a failed uniqueness validation leaves behind.
///
/// The first duplicate pair agrees on the candidate's own columns and possibly
/// more; every pool column it agrees on joins the set, and any later candidate
/// inside the set is non-unique on this side without being validated at all.
/// Reading the pair across the pool is charged as the two rows it is.
fn record_agree(
    pool: &[PoolColumn],
    side: Side,
    clusters: &[Vec<usize>],
    agree: &mut Vec<Vec<usize>>,
    meter: &mut Meter,
) {
    if !meter.charge(2) {
        return;
    }
    let pair = &clusters[0];
    let (a, b) = (pair[0], pair[1]);
    agree.push(
        pool.iter()
            .enumerate()
            .filter(|(_, column)| {
                let column = column.side(side);
                column.values[a] == column.values[b]
            })
            .map(|(index, _)| index)
            .collect(),
    );
}

/// Whether `candidate` is contained in `set`, both ascending.
fn subset(candidate: &[usize], set: &[usize]) -> bool {
    let mut set = set.iter();
    'candidate: for &column in candidate {
        for &member in set.by_ref() {
            if member == column {
                continue 'candidate;
            }
            if member > column {
                return false;
            }
        }
        return false;
    }
    true
}

/// Measure what a candidate tuple shares across sides, reading both in full.
///
/// `shared` counts distinct old tuples that occur in `new`, and `affected`
/// counts those that occur there more than once. Counting distinct keys rather
/// than matching rows keeps a fanning candidate from earning a point per
/// duplicate. The caller has already established uniqueness in `old`, so each
/// old row contributes one distinct tuple.
///
/// The per-row digests combine into a tuple digest that only chooses buckets;
/// equality of the component values decides membership, so a collision can
/// neither manufacture a match nor inflate a count.
fn overlap_of(pool: &[PoolColumn], columns: &[usize]) -> (usize, usize) {
    let rows_new = pool[0].new.values.len();
    let index =
        DigestIndex::new((0..rows_new).map(|row| tuple_digest(pool, columns, Side::New, row)));
    let rows_old = pool[0].old.values.len();
    let mut shared = 0;
    let mut affected = 0;
    for row in 0..rows_old {
        let digest = tuple_digest(pool, columns, Side::Old, row);
        match index
            .rows(digest)
            .iter()
            .filter(|&&new_row| tuples_equal(pool, columns, row, new_row))
            .count()
        {
            0 => {}
            1 => shared += 1,
            _ => {
                shared += 1;
                affected += 1;
            }
        }
    }
    (shared, affected)
}

/// Combine a row's per-column digests into one tuple digest.
///
/// The combination need only be deterministic and order-sensitive, because a
/// tuple digest is a bucket index and never an equality: `tuples_equal`
/// decides membership on the values themselves.
fn tuple_digest(pool: &[PoolColumn], columns: &[usize], side: Side, row: usize) -> u128 {
    columns.iter().fold(0u128, |digest, &column| {
        digest
            .rotate_left(11)
            .wrapping_add(pool[column].side(side).digests[row])
            .wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835)
    })
}

fn tuples_equal(pool: &[PoolColumn], columns: &[usize], old_row: usize, new_row: usize) -> bool {
    columns
        .iter()
        .all(|&column| pool[column].old.values[old_row] == pool[column].new.values[new_row])
}

/// One declared key component, parsed but not yet resolved to columns.
///
/// Owned rather than borrowed because components are parsed before hints are
/// considered and resolved after, so they outlive the strings they came from.
pub(crate) struct Component {
    old: String,
    new: String,
}

impl Component {
    fn named(&self) -> KeyComponent {
        KeyComponent {
            old: self.old.clone(),
            new: self.new.clone(),
        }
    }

    fn rejected(&self, reason: RejectionReason) -> KeyRejection {
        KeyRejection {
            subject: KeySubject::Component(self.named()),
            reason,
        }
    }
}

/// The component naming the key that matches rows by position.
///
/// Reserved rather than looked up. A bare name in this format is letters,
/// digits and underscores and never begins with `:`, so this cannot collide
/// with any column the output writes bare; a column genuinely called `:row`
/// prints quoted and stays distinguishable, at the cost of never being
/// declarable as a key itself.
pub const POSITIONAL_COMPONENT: &str = ":row";

pub(crate) enum Declared {
    /// No `--key` at all, so a key is to be guessed.
    Guess,
    /// `--key :row`: match rows by position, deliberately.
    Positional,
    Components(Vec<Component>),
}

impl Declared {
    pub(crate) fn components(&self) -> &[Component] {
        match self {
            Declared::Components(components) => components,
            Declared::Guess | Declared::Positional => &[],
        }
    }
}

/// Parse each component and check that no column is claimed twice.
///
/// This is the only stage of key resolution that still fails outright, because
/// it is the only one whose failures are faults in the `--key` string rather
/// than in what the data can support. Everything below it produces a
/// `KeyRejection` and falls back.
///
/// Uniqueness is a property of the endpoints rather than of the component
/// string: `id,id/other` claims `id` on the old side twice while spelling its
/// components differently, and `a/b,c/b` claims `b` on the new side twice.
pub(crate) fn declared_components(keys: &[String]) -> Result<Declared, DiffError> {
    if keys.iter().any(|key| key == POSITIONAL_COMPONENT) {
        // A positional key is the whole key or none of it: there is nothing for
        // a column to compound with.
        if keys.len() > 1 {
            return Err(DiffError::CompoundPositionalKey);
        }
        return Ok(Declared::Positional);
    }
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
        // Reaching here means the spelling is not `:row` alone, so an endpoint
        // naming it is one half of a pair, which has no reading.
        if old == POSITIONAL_COMPONENT || new == POSITIONAL_COMPONENT {
            return Err(DiffError::CompoundPositionalKey);
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
            old: old.to_owned(),
            new: new.to_owned(),
        });
    }
    Ok(if components.is_empty() {
        Declared::Guess
    } else {
        Declared::Components(components)
    })
}

fn validate_present(
    keys: &KeyValues,
    components: &[Component],
    side: Side,
) -> Result<(), KeyRejection> {
    for row in 0..keys.len() {
        for (position, value) in keys.row(row).iter().enumerate() {
            if value.invalid_key() {
                return Err(components[position]
                    .rejected(RejectionReason::InvalidValue { side, row: row + 1 }));
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
fn validate_unique_old(keys: &KeyValues) -> Result<(), RejectionReason> {
    let index = KeyIndex::new(keys);
    for row in 0..keys.len() {
        let first = index
            .rows(keys.row(row), keys.digest(row))
            .next()
            .expect("a row matches its own key");
        if first != row {
            return Err(RejectionReason::NonUniqueOld {
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
fn validate_fanout(old_keys: &KeyValues, new: &KeyIndex) -> Result<(), RejectionReason> {
    let mut shared = 0;
    let mut affected = 0;
    for row in 0..old_keys.len() {
        match new.rows(old_keys.row(row), old_keys.digest(row)).count() {
            0 => {}
            1 => shared += 1,
            _ => {
                shared += 1;
                affected += 1;
            }
        }
    }
    if !within_fanout_limit(affected, shared) {
        return Err(RejectionReason::ExcessiveFanout { affected, shared });
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
    use arrow_array::RecordBatch;
    use test_support::{rows_without_columns, table};

    use super::testing::{rejection, resolve_key};
    use super::{
        KeyIndex, KeyValues, PoolColumn, ResolvedKey, SideColumn, declared_components, guess_key,
        overlap_of, validate_fanout, validate_unique_old,
    };
    #[cfg(test)]
    use crate::DiffOptions;
    use crate::compare::{CanonicalValue, stable_hash};
    use crate::schema::ColumnMap;
    use crate::{
        Budgets, DiffError, IdentityBasis, KeyBasis, KeyComponent, KeyOverlap, KeyRejection,
        KeySubject, RejectionReason, Side,
    };

    /// Guess under default budgets, which ordinary fixtures never bind.
    fn guess(
        old: &RecordBatch,
        new: &RecordBatch,
        map: &ColumnMap,
        excluded: &[Vec<(usize, usize)>],
    ) -> Option<ResolvedKey> {
        let guess = guess_key(old, new, map, excluded, &Budgets::default());
        assert!(!guess.exhausted, "default budgets must not bind a fixture");
        guess.key
    }

    /// A pool column over two literal canonical columns, for exercising the
    /// search's measurements directly.
    fn pool_column(
        old: Vec<CanonicalValue>,
        new: Vec<CanonicalValue>,
        hash: fn(&CanonicalValue) -> u128,
    ) -> PoolColumn {
        PoolColumn {
            old_index: 0,
            new_index: 0,
            old: SideColumn::with_hash(old, hash),
            new: SideColumn::with_hash(new, hash),
        }
    }

    /// One component naming the same column on both sides.
    fn shared(name: &str) -> KeyComponent {
        KeyComponent {
            old: name.to_owned(),
            new: name.to_owned(),
        }
    }

    /// One component naming a column that differs between the files.
    fn paired(old: &str, new: &str) -> KeyComponent {
        KeyComponent {
            old: old.to_owned(),
            new: new.to_owned(),
        }
    }

    /// The old-side name of one resolved key component.
    fn key_name(old: &RecordBatch, key: &super::ResolvedKey, component: usize) -> String {
        old.schema()
            .field(key.columns[component].old)
            .name()
            .clone()
    }

    fn options(key: &[&str]) -> DiffOptions {
        DiffOptions {
            key: key.iter().map(|value| (*value).to_owned()).collect(),
            ..DiffOptions::default()
        }
    }

    #[test]
    fn the_positional_component_is_a_whole_key_or_none_of_it() {
        // A positional key has no components to compound with, so both the
        // compound and the paired form are faults in the --key string itself.
        for key in [&["id", ":row"][..], &[":row", "id"][..], &["id/:row"][..]] {
            assert!(matches!(
                declared_components(&key.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>()),
                Err(DiffError::CompoundPositionalKey)
            ));
        }
    }

    #[test]
    fn the_positional_key_satisfies_what_row_matching_assumes() {
        let old = table! { "label" => ["x", "x", "x"] };
        let new = table! { "label" => ["x", "x"] };

        let key = resolve_key(&old, &new, &options(&[":row"])).unwrap();

        // One key per row, distinct and therefore unique in `old` and incapable
        // of fanout, and no rejection because there is nothing to validate.
        assert_eq!(key.basis, KeyBasis::Declared);
        assert!(key.columns.is_empty());
        assert_eq!(key.old.len(), 3);
        assert_eq!(key.new.len(), 2);
        assert_eq!(key.old.tuples()[..2], key.new.tuples()[..]);
        assert!(key.rejection.is_none());
        assert!(validate_unique_old(&key.old).is_ok());
        assert!(validate_fanout(&key.old, &KeyIndex::new(&key.new)).is_ok());
    }

    #[test]
    fn a_refused_declaration_falls_through_to_a_guess_and_keeps_its_reason() {
        let old = table! { "id" => [1, 1], "other" => [7, 8] };
        let new = table! { "id" => [1, 1], "other" => [7, 8] };

        // `id` repeats in `old`, so it is refused as declared and is equally
        // ineligible as a guess; `other` identifies rows and is guessed instead.
        let key = resolve_key(&old, &new, &options(&["id"])).unwrap();

        assert_eq!(key.basis, KeyBasis::Guessed);
        assert_eq!(key_name(&old, &key, 0), "other");
        assert_eq!(
            key.rejection,
            Some(KeyRejection {
                subject: KeySubject::Key(vec![shared("id")]),
                reason: RejectionReason::NonUniqueOld {
                    first_row: 1,
                    row: 2,
                },
            })
        );
    }

    #[test]
    fn a_refused_declaration_reaches_position_when_no_guess_survives() {
        let old = table! { "id" => [1, 1] };
        let new = table! { "id" => [1, 1] };

        let key = resolve_key(&old, &new, &options(&["id"])).unwrap();

        assert_eq!(key.basis, KeyBasis::Fallback);
        assert!(key.columns.is_empty());
        assert!(key.rejection.is_some());
    }

    #[test]
    fn validates_key_syntax() {
        let empty = table! {};
        // Nothing to guess from, so the chain reaches its last resort.
        assert_eq!(
            resolve_key(&empty, &empty, &options(&[])).unwrap().basis,
            KeyBasis::Fallback
        );
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
        assert_eq!(key_name(&old, &key, 0), "customer_id");
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

        // The subject is the component as written, and the side says which of
        // its two ends could not be found.
        assert_eq!(
            rejection(&old, &new, &["customer_id/id"]),
            Some(KeyRejection {
                subject: KeySubject::Component(paired("customer_id", "id")),
                reason: RejectionReason::MissingColumn { side: Side::New },
            })
        );
    }

    #[test]
    fn a_boolean_and_a_numeric_component_pair_validates() {
        let old = table! { "customer_id" => [true, false] };
        let new = table! { "id" => [1, 0] };

        // Once rejected as incompatible; the encoding now compares, so the
        // declared pair identifies both rows.
        let key = resolve_key(&old, &new, &options(&["customer_id/id"])).unwrap();

        assert_eq!(key.rejection, None);
        assert_eq!(key.old, key.new);
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
            rejection(&old, &new, &["id"]),
            Some(KeyRejection {
                subject: KeySubject::Component(shared("id")),
                reason: RejectionReason::MissingColumn { side: Side::New },
            })
        );
    }

    #[test]
    fn rejects_null_and_nan_with_row_context() {
        let old = table! { "id" => [Some(1.0), None] };
        let new = table! { "id" => [1.0, 2.0] };
        assert_eq!(
            rejection(&old, &new, &["id"]),
            Some(KeyRejection {
                subject: KeySubject::Component(shared("id")),
                reason: RejectionReason::InvalidValue {
                    side: Side::Old,
                    row: 2,
                },
            })
        );

        let old = table! { "id" => [f64::NAN] };
        let new = table! { "id" => [1.0] };
        assert!(matches!(
            rejection(&old, &new, &["id"]),
            Some(KeyRejection {
                reason: RejectionReason::InvalidValue { .. },
                ..
            })
        ));
    }

    #[test]
    fn uniqueness_uses_cross_type_canonicalization() {
        let old = table! { "id" => ["1", "1.0"] };
        let new = table! { "id" => [1, 2] };
        // Uniqueness belongs to the tuple, so the whole declared key is named.
        assert_eq!(
            rejection(&old, &new, &["id"]),
            Some(KeyRejection {
                subject: KeySubject::Key(vec![shared("id")]),
                reason: RejectionReason::NonUniqueOld {
                    first_row: 1,
                    row: 2,
                },
            })
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
            rejection(&old, &new, &["id"]),
            Some(KeyRejection {
                subject: KeySubject::Key(vec![shared("id")]),
                reason: RejectionReason::ExcessiveFanout {
                    affected: 1,
                    shared: 2,
                },
            })
        );
    }

    #[test]
    fn counts_each_fanned_out_key_once() {
        let old = table! { "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] };
        let new = table! { "id" => [1, 2, 3, 4, 4, 4, 5, 6, 7, 8, 9, 10] };

        // Three new rows for one key is still one affected key; counting rows
        // would make this 20% and reject it.
        assert!(rejection(&old, &new, &["id"]).is_none());
    }

    #[test]
    fn measures_fanout_against_shared_keys_rather_than_all_old_keys() {
        let old = table! { "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20] };
        let new = table! { "id" => [1, 2, 3, 4, 4, 5] };

        // One of five shared keys is 20% and rejects; one of twenty old keys
        // would be 5% and would wrongly retain.
        assert_eq!(
            rejection(&old, &new, &["id"]),
            Some(KeyRejection {
                subject: KeySubject::Key(vec![shared("id")]),
                reason: RejectionReason::ExcessiveFanout {
                    affected: 1,
                    shared: 5,
                },
            })
        );
    }

    #[test]
    fn new_only_duplicates_are_additions_rather_than_fanout() {
        let old = table! { "id" => [1, 2] };
        let new = table! { "id" => [1, 2, 3, 3] };

        // A key absent from `old` has no row to fan out from, so however often
        // it repeats it cannot invalidate the declared key.
        assert!(rejection(&old, &new, &["id"]).is_none());
    }

    #[test]
    fn duplicates_without_any_shared_key_leave_the_key_valid() {
        let old = table! { "id" => [1] };
        let new = table! { "id" => [2, 2] };

        assert!(rejection(&old, &new, &["id"]).is_none());
    }

    #[test]
    fn old_side_duplication_is_fatal_even_when_new_fans_out() {
        let old = table! { "id" => [1, 1] };
        let new = table! { "id" => [1, 1] };

        // Uniqueness belongs to the tuple, so the whole declared key is named.
        assert_eq!(
            rejection(&old, &new, &["id"]),
            Some(KeyRejection {
                subject: KeySubject::Key(vec![shared("id")]),
                reason: RejectionReason::NonUniqueOld {
                    first_row: 1,
                    row: 2,
                },
            })
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
        assert_eq!(key_name(&old, &key, 0), "other");
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
        assert_eq!(key_name(&old, &key, 0), "id");
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
        assert_eq!(key_name(&old, &key, 0), "a");
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
        assert_eq!(key_name(&old, &key, 0), "b");
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
        assert_eq!(key_name(&old, &key, 0), "b");
    }

    #[test]
    fn new_only_duplicates_do_not_disqualify_a_candidate() {
        let old = table! { "id" => [1, 2] };
        let new = table! { "id" => [1, 2, 3, 3] };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // Key 3 is absent from `old`, so its rows are additions rather than a
        // fanout and the candidate is unaffected by them.
        assert_eq!(key_name(&old, &key, 0), "id");
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
        assert_eq!(key_name(&old, &key, 0), "other");
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
            let key = resolve_key(old, new, &options(&[])).unwrap();
            assert_eq!(key.basis, KeyBasis::Fallback);
            assert!(key.columns.is_empty());
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
        assert_eq!(key_name(&old, &key, 0), "id");
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
            key.old.tuples(),
            vec![vec![CanonicalValue::Int(1)], vec![CanonicalValue::Int(2)]]
        );
        assert_eq!(
            key.new.tuples(),
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
            "disjoint" => [1, 2],
            "missing" => [1, 2],
        };
        let new = table! {
            "null" => [1, 2],
            "nan" => [1.0, 2.0],
            "dup_old" => [5, 6],
            "dup_new" => [1, 1],
            "disjoint" => [3, 4],
        };

        // No single column qualifies, and no combination of the survivors
        // shares a tuple — `dup_old` and `dup_new` are unique together on both
        // sides, but their old tuples pair values the new file never pairs —
        // so the compound search finds nothing either.
        assert_eq!(
            resolve_key(&old, &new, &options(&[])).unwrap().basis,
            KeyBasis::Fallback
        );
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

        assert_eq!(key_name(&old, &key, 0), "full");
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

        assert_eq!(key_name(&old, &key, 0), "b");
        assert_eq!((key.columns[0].old, key.columns[0].new), (0, 1));
    }

    #[test]
    fn guessing_passes_over_an_excluded_candidate() {
        let old = table! { "id" => [1, 2], "code" => [7, 8] };
        let new = table! { "id" => [1, 2], "code" => [7, 8] };
        let map = ColumnMap::new(old.schema_ref(), new.schema_ref());

        // Exclusion narrows the field without changing the ranking: the best
        // remaining candidate wins. Excluding every single column leaves
        // nothing, because an excluded eligible candidate also blocks its
        // supersets — a compound built on a retracted key inherits its
        // condemned matching — so no pair can replace the retracted singles.
        let first = guess(&old, &new, &map, &[]).unwrap();
        assert_eq!(key_name(&old, &first, 0), "id");
        let second = guess(&old, &new, &map, &[vec![(0, 0)]]).unwrap();
        assert_eq!(key_name(&old, &second, 0), "code");
        assert!(guess(&old, &new, &map, &[vec![(0, 0)], vec![(1, 1)]]).is_none());
    }

    #[test]
    fn an_identity_in_the_map_makes_a_cross_name_candidate() {
        let old = table! { "customer_id" => [1, 2], "v" => [1, 1] };
        let new = table! { "id" => [1, 2], "v" => [1, 1] };

        // No name is shared by a usable column, so nothing can be guessed —
        // until the map identifies the renamed pair, which is the mechanism
        // reconsideration widens the field through.
        let bare = ColumnMap::new(old.schema_ref(), new.schema_ref());
        assert!(guess(&old, &new, &bare, &[]).is_none());

        let mut identified = ColumnMap::new(old.schema_ref(), new.schema_ref());
        identified.claim(0, 0, IdentityBasis::Exact);
        let key = guess(&old, &new, &identified, &[]).unwrap();
        assert_eq!((key.columns[0].old, key.columns[0].new), (0, 0));
        assert_eq!(key.basis, KeyBasis::Guessed);
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
    fn guesses_a_compound_key_when_no_single_column_qualifies() {
        let old = table! {
            "group" => ["a", "a", "b", "b"],
            "id" => [1, 2, 1, 2],
            "value" => [10, 10, 10, 10],
        };
        let new = table! {
            "group" => ["a", "a", "b", "b"],
            "id" => [1, 2, 1, 2],
            "value" => [10, 10, 20, 10],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // Every column repeats on its own, and only (group, id) is unique on
        // both sides, so the tuple is the guess and its overlap is complete.
        assert_eq!(key.basis, KeyBasis::Guessed);
        assert_eq!(key_name(&old, &key, 0), "group");
        assert_eq!(key_name(&old, &key, 1), "id");
        assert_eq!(
            key.overlap,
            Some(KeyOverlap {
                shared: 4,
                possible: 4,
            })
        );
        assert_eq!(key.old, key.new);
        assert!(!key.exhausted);
    }

    #[test]
    fn a_compound_sharing_more_tuples_outranks_a_single_column() {
        let old = table! {
            "s" => [1, 2, 3, 4, 5, 6],
            "g" => ["a", "a", "a", "b", "b", "b"],
            "i" => [1, 2, 3, 1, 2, 3],
        };
        let new = table! {
            "s" => [1, 2, 999, 998, 997, 996],
            "g" => ["a", "a", "a", "b", "b", "b"],
            "i" => [1, 2, 3, 1, 2, 3],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // "s" is eligible on two shared values; (g, i) shares all six tuples.
        // The evidence outranks the parsimony tie-break, which never fires.
        assert_eq!(key.columns.len(), 2);
        assert_eq!(key_name(&old, &key, 0), "g");
        assert_eq!(key_name(&old, &key, 1), "i");
        assert_eq!(
            key.overlap,
            Some(KeyOverlap {
                shared: 6,
                possible: 6,
            })
        );
    }

    #[test]
    fn a_single_column_wins_a_shared_tuple_tie_by_fewer_columns() {
        // "s" shares all ten keys and fans one out, within the allowance;
        // (g, i) shares the same ten cleanly. Fewer columns settles the tie
        // before freedom from fanout gets a say, which is the declared
        // ranking: shared tuples, then fewer columns, then column order.
        let old = table! {
            "s" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "g" => ["a", "a", "a", "a", "a", "b", "b", "b", "b", "b"],
            "i" => [1, 2, 3, 4, 5, 1, 2, 3, 4, 5],
        };
        let new = table! {
            "s" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
            "g" => ["a", "a", "a", "a", "a", "a", "b", "b", "b", "b", "b"],
            "i" => [1, 2, 3, 4, 9, 5, 1, 2, 3, 4, 5],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        assert_eq!(key.columns.len(), 1);
        assert_eq!(key_name(&old, &key, 0), "s");
    }

    #[test]
    fn a_compound_guess_requires_uniqueness_on_both_sides() {
        let old = table! {
            "g" => ["a", "a", "b"],
            "i" => [1, 2, 1],
        };
        let new = table! {
            "g" => ["a", "a", "a", "b"],
            "i" => [1, 2, 2, 1],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // (g, i) is unique in `old` but duplicates ("a", 2) in `new`. A single
        // column would be allowed that as bounded fanout; a compound guess is
        // refused it, so no guessed compound key can ever fan out.
        assert_eq!(key.basis, KeyBasis::Fallback);
        assert!(key.columns.is_empty());
    }

    #[test]
    fn a_missing_value_excludes_a_column_from_every_width() {
        let old = table! {
            "g" => ["a", "a", "b", "b"],
            "i" => [Some(1), Some(2), Some(1), None],
        };
        let new = table! {
            "g" => ["a", "a", "b", "b"],
            "i" => [1, 2, 1, 2],
        };

        // (g, i) would identify every row, but a key may not contain a missing
        // value, so `i` leaves before the lattice exists and nothing remains.
        let key = resolve_key(&old, &new, &options(&[])).unwrap();
        assert_eq!(key.basis, KeyBasis::Fallback);
    }

    #[test]
    fn a_compound_component_can_cure_excessive_fanout() {
        let old = table! {
            "id" => [1, 2],
            "sub" => [1, 1],
        };
        let new = table! {
            "id" => [1, 1, 2],
            "sub" => [1, 2, 1],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // "id" alone fans out at 50%, far past the allowance, and "sub"
        // repeats in `old`; together they are unique on both sides. The
        // extendable candidates are exactly the ones a column can still cure.
        assert_eq!(key.basis, KeyBasis::Guessed);
        assert_eq!(key.columns.len(), 2);
        assert_eq!(
            key.overlap,
            Some(KeyOverlap {
                shared: 2,
                possible: 2,
            })
        );
    }

    #[test]
    fn a_compound_overlap_is_normalized_by_the_smaller_side() {
        let old = table! {
            "g" => ["a", "a", "b", "b"],
            "i" => [1, 2, 1, 2],
        };
        let new = table! {
            "g" => ["a", "a", "b"],
            "i" => [1, 2, 1],
        };

        let key = resolve_key(&old, &new, &options(&[])).unwrap();

        // Unique on both sides means the distinct counts are the row counts.
        assert_eq!(key.columns.len(), 2);
        assert_eq!(
            key.overlap,
            Some(KeyOverlap {
                shared: 3,
                possible: 3,
            })
        );
    }

    #[test]
    fn an_exhausted_search_keeps_the_best_candidate_it_examined() {
        let old = table! {
            "code" => [1, 2, 3],
            "g" => ["a", "a", "b"],
            "i" => [1, 2, 1],
        };
        let new = table! {
            "code" => [1, 2, 3],
            "g" => ["a", "a", "b"],
            "i" => [1, 2, 1],
        };
        let map = ColumnMap::new(old.schema_ref(), new.schema_ref());

        // One admitted candidate: "code" is examined and wins before the
        // meter dies, which is the useful partial result.
        let bounded = Budgets {
            key_candidates: 1,
            ..Budgets::default()
        };
        let cut_short = guess_key(&old, &new, &map, &[], &bounded);
        assert!(cut_short.exhausted);
        let key = cut_short.key.unwrap();
        assert_eq!(key_name(&old, &key, 0), "code");

        // No admitted candidates: nothing examined, nothing found, and the
        // exhaustion still reported so the fallback carries the story.
        let starved = Budgets {
            key_candidates: 0,
            ..Budgets::default()
        };
        let nothing = guess_key(&old, &new, &map, &[], &starved);
        assert!(nothing.exhausted);
        assert!(nothing.key.is_none());

        // A row budget of zero refuses the first measurement the same way.
        let rowless = Budgets {
            key_rows: crate::RowBudget::Rows(0),
            ..Budgets::default()
        };
        let unfunded = guess_key(&old, &new, &map, &[], &rowless);
        assert!(unfunded.exhausted);
        assert!(unfunded.key.is_none());
    }

    #[test]
    fn an_exhausted_fallback_reports_the_search_it_cut_short() {
        let old = table! { "id" => [1, 2] };
        let new = table! { "id" => [1, 2] };

        let bounded = DiffOptions {
            budgets: Budgets {
                key_candidates: 0,
                ..Budgets::default()
            },
            ..DiffOptions::default()
        };
        let key = resolve_key(&old, &new, &bounded).unwrap();

        // Nothing was examined, so the fallback stands in — carrying the
        // exhaustion, because the key that went unexamined is part of its
        // story.
        assert_eq!(key.basis, KeyBasis::Fallback);
        assert!(key.exhausted);
    }

    #[test]
    fn repeated_compound_guessing_is_deterministic() {
        let old = table! {
            "g" => ["a", "a", "b", "b"],
            "i" => [1, 2, 1, 2],
            "j" => [1, 2, 2, 1],
        };
        let new = table! {
            "g" => ["a", "b", "a", "b"],
            "i" => [1, 1, 2, 2],
            "j" => [1, 2, 2, 1],
        };

        let first = resolve_key(&old, &new, &options(&[])).unwrap();
        let second = resolve_key(&old, &new, &options(&[])).unwrap();

        assert_eq!(first.columns.len(), second.columns.len());
        assert_eq!(first.overlap, second.overlap);
        assert_eq!(first.old, second.old);
        assert_eq!(first.new, second.new);
    }

    #[test]
    fn rows_without_columns_leave_nothing_to_guess() {
        let old = rows_without_columns(2);

        // No column can be a candidate, so rows are matched by position.
        let key = resolve_key(&old, &old, &options(&[])).unwrap();
        assert_eq!(key.basis, KeyBasis::Fallback);
        assert_eq!(key.old.len(), 2);
        assert_eq!(key.old, key.new);
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
        assert_eq!(key_name(&old, &key, 0), "weak");
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

        assert_eq!(first.columns[0].old, second.columns[0].old);
        assert_eq!(first.overlap, second.overlap);
        assert_eq!(first.old, second.old);
        assert_eq!(first.new, second.new);
    }

    #[test]
    fn key_values_interleave_components_into_rows() {
        let keys = KeyValues::from_columns(
            vec![
                vec![CanonicalValue::Int(1), CanonicalValue::Int(2)],
                vec![CanonicalValue::Int(10), CanonicalValue::Int(20)],
            ],
            2,
        );

        assert_eq!(keys.len(), 2);
        assert_eq!(
            keys.row(0),
            [CanonicalValue::Int(1), CanonicalValue::Int(10)]
        );
        assert_eq!(
            keys.row(1),
            [CanonicalValue::Int(2), CanonicalValue::Int(20)]
        );
        // The stored digest is the same sequence hash any consumer would have
        // computed from the tuple.
        assert_eq!(keys.digest(0), super::sequence_hash(keys.row(0)));
        assert_ne!(keys.digest(0), keys.digest(1));
    }

    #[test]
    fn a_key_index_confirms_equality_within_a_bucket() {
        let keys = KeyValues::with_hash(
            1,
            vec![
                CanonicalValue::Int(1),
                CanonicalValue::Int(2),
                CanonicalValue::Int(1),
            ],
            |_| 0,
        );
        let index = KeyIndex::new(&keys);

        // Every key digests alike, so only the confirmation step can separate
        // them, and the rows stay in ascending order.
        assert_eq!(index.rows(keys.row(0), 0).collect::<Vec<_>>(), [0, 2]);
        assert_eq!(index.rows(keys.row(1), 0).collect::<Vec<_>>(), [1]);
        assert!(index.rows(&[CanonicalValue::Int(3)], 0).next().is_none());
    }

    #[test]
    fn forced_hash_collisions_cannot_fake_duplicates_or_overlap() {
        fn constant(_: &CanonicalValue) -> u128 {
            0
        }
        let old = vec![CanonicalValue::Int(1), CanonicalValue::Int(2)];
        let new = vec![CanonicalValue::Int(2), CanonicalValue::Int(3)];

        // Every digest collides, so only the equality confirmations can keep
        // the values apart: no false duplicate cluster, no inflated overlap.
        for hash in [constant as fn(&CanonicalValue) -> u128, stable_hash] {
            let column = pool_column(old.clone(), new.clone(), hash);
            assert!(column.old.unique());
            assert!(column.new.unique());
            assert_eq!(column.new.distinct(), 2);
            assert_eq!(overlap_of(&[column], &[0]), (1, 0));
        }
        let duplicated = pool_column(
            vec![CanonicalValue::Int(1), CanonicalValue::Int(1)],
            new,
            constant,
        );
        assert_eq!(duplicated.old.clusters, vec![vec![0, 1]]);
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
        let column = pool_column(old, new, stable_hash);
        assert_eq!(column.new.distinct(), 3);
        assert_eq!(overlap_of(&[column], &[0]), (2, 1));
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
