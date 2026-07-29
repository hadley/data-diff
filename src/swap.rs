//! Reinterpret two rewritten same-name columns as one exchange.

use arrow_array::RecordBatch;

use crate::agreement::Aligned;
use crate::compare::ComparisonPlan;
use crate::hint::Hints;
use crate::rows::RowMatches;
use crate::schema::{ColumnIdentity, SchemaMatches};

/// Exchange the ends of two identities whose columns hold each other's values.
///
/// When two same-named columns both change beyond recognition, the likelier
/// account is not that both were rewritten but that their contents were
/// swapped. This reads and rewrites `identities` only: it neither consumes nor
/// produces a drop or an addition, which is what keeps it independent of
/// rename inference.
pub(crate) fn infer(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &mut SchemaMatches,
    rows: &RowMatches,
    hints: &Hints,
) {
    if rows.matched.is_empty() {
        return;
    }
    let mut values = Aligned::new(old, new, rows);
    let eligible = eligible(old, new, schema, hints);

    let mut candidates = Vec::new();
    for (position, &first) in eligible.iter().enumerate() {
        for &second in &eligible[position + 1..] {
            if exchanged(old, new, schema, &mut values, first, second) {
                candidates.push((first, second));
            }
        }
    }

    // Competing swaps cancel rather than compete: a column that could plausibly
    // have been exchanged with either of two others is evidence of neither, and
    // the design leaves it to the user rather than to a tie-break.
    let mut claims = vec![0_usize; schema.identities.len()];
    for &(first, second) in &candidates {
        claims[first] += 1;
        claims[second] += 1;
    }
    for (first, second) in candidates {
        if claims[first] == 1 && claims[second] == 1 {
            exchange(old, new, schema, first, second);
        }
    }
}

/// The identities a swap may consume: provisional, same-named, and not a key.
///
/// Name equality is what makes an identity provisional. A paired key component
/// is excluded by `is_key`, and an identity established by rename inference
/// carries different names at its ends — and could not qualify anyway, since
/// inference only ever establishes identities that agree closely while a swap
/// needs two that barely agree at all.
///
/// A hinted identity is excluded whatever its names are. Every other exclusion
/// here is about a default reconciliation chose, which a swap may override on
/// better evidence; a hint is not a default but an instruction, and inference
/// does not get to overrule one. This only bites for a hint whose two ends carry
/// the same name, every other hinted identity being ineligible already.
fn eligible(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &SchemaMatches,
    hints: &Hints,
) -> Vec<usize> {
    let old_schema = old.schema();
    let new_schema = new.schema();
    schema
        .identities
        .iter()
        .enumerate()
        .filter(|(_, identity)| {
            !identity.is_key
                && !hints.asserted(identity.old, identity.new)
                && old_schema.field(identity.old).name() == new_schema.field(identity.new).name()
        })
        .map(|(position, _)| position)
        .collect()
}

/// Whether two identities look like each other's exchange.
///
/// Both columns must have been rewritten under their own names, and both
/// crossings must agree closely enough to stand as renames in their own right.
fn exchanged(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &SchemaMatches,
    values: &mut Aligned,
    first: usize,
    second: usize,
) -> bool {
    let first = &schema.identities[first];
    let second = &schema.identities[second];
    if !rewritten(old, new, values, first) || !rewritten(old, new, values, second) {
        return false;
    }
    crosses(old, new, values, first.old, second.new)
        && crosses(old, new, values, second.old, first.new)
}

/// Whether an identity's own two ends agree in fewer than half their rows.
fn rewritten(
    old: &RecordBatch,
    new: &RecordBatch,
    values: &mut Aligned,
    identity: &ColumnIdentity,
) -> bool {
    let plan = plan_for(old, new, identity.old, identity.new)
        .expect("schema reconciliation accepted the type pair");
    values
        .measure(plan, identity.old, identity.new)
        .is_distant()
}

/// Whether one identity's old end holds the other's new values, unconverted.
///
/// A crossing has to be the same type on both sides, not merely a comparable
/// one, so a swap never carries a type change. Rename inference is more
/// permissive because it fills a vacuum: the alternative to a cross-type
/// rename is a drop and an addition, which relate the columns not at all. A
/// swap instead overrides an identity that name matching already established,
/// so it answers to a higher bar, and an exchange evidenced by values compared
/// in their own representation is the cleaner claim. Columns that were both
/// exchanged *and* retyped fall back to two `col_edit()` events, which is a
/// truthful description and a less specific one.
fn crosses(
    old: &RecordBatch,
    new: &RecordBatch,
    values: &mut Aligned,
    old_index: usize,
    new_index: usize,
) -> bool {
    let old_column = old.column(old_index);
    let new_column = new.column(new_index);
    old_column.data_type() == new_column.data_type()
        && plan_for(old, new, old_index, new_index)
            .is_some_and(|plan| values.measure(plan, old_index, new_index).is_close())
}

/// Exchange two identities' new ends, atomically and in place.
///
/// Neither old position moves, so `identities` stays sorted by old position
/// and `minimal_moves` keeps the precondition it asserts. The new positions do
/// move, which is why an accepted swap can produce a `col_order()` entry: the
/// column holding one column's values is now where the other one was.
fn exchange(
    old: &RecordBatch,
    new: &RecordBatch,
    schema: &mut SchemaMatches,
    first: usize,
    second: usize,
) {
    let first_new = schema.identities[first].new;
    let second_new = schema.identities[second].new;
    rewire(old, new, &mut schema.identities[first], second_new);
    rewire(old, new, &mut schema.identities[second], first_new);
}

fn rewire(old: &RecordBatch, new: &RecordBatch, identity: &mut ColumnIdentity, new_index: usize) {
    identity.new = new_index;
    // Recomputed rather than carried over: the identity being replaced
    // described a different pair of columns, and its type change said nothing
    // about this one. Since a crossing is the same type on both sides, the
    // answer is always that it did not change, which is the point — a swap
    // dissolves the type changes the two same-name readings were reporting.
    identity.type_changed =
        old.column(identity.old).data_type() != new.column(new_index).data_type();
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

    use super::infer;
    use crate::DiffOptions;
    use crate::hint::Hints;
    use crate::key::testing::resolve_key;
    use crate::rename;
    use crate::rows::match_rows;
    use crate::schema::testing::reconcile_schema;
    use crate::schema::{ColumnIdentity, SchemaMatches};

    fn infer_swaps(old: &RecordBatch, new: &RecordBatch) -> SchemaMatches {
        let options = DiffOptions {
            key: vec!["id".into()],
            hints: Vec::new(),
        };
        let key = resolve_key(old, new, &options).unwrap();
        let rows = match_rows(&key);
        let mut schema = reconcile_schema(old, new, &key).unwrap();
        infer(old, new, &mut schema, &rows, &Hints::default());
        schema
    }

    /// The `(old, new)` pairs of every identity that is not the key.
    fn pairs(schema: &SchemaMatches) -> Vec<(usize, usize)> {
        schema
            .identities
            .iter()
            .filter(|identity| !identity.is_key)
            .map(|identity| (identity.old, identity.new))
            .collect()
    }

    #[test]
    fn a_swap_dissolves_the_type_changes_it_explains() {
        let old = table! {
            "id" => [1, 2, 3],
            "price" => [10, 20, 30],
            "cost" => ["a", "b", "c"],
        };
        let new = table! {
            "id" => [1, 2, 3],
            "price" => ["a", "b", "c"],
            "cost" => [10, 20, 30],
        };

        let schema = infer_swaps(&old, &new);

        // The crossings are integer to integer and string to string. It is the
        // same-name readings that changed type, and the swap is what explains
        // why: each column was being compared with the wrong one.
        assert_eq!(pairs(&schema), [(1, 2), (2, 1)]);
        assert_eq!(
            schema.identities[1],
            ColumnIdentity {
                old: 1,
                new: 2,
                type_changed: false,
                is_key: false,
            }
        );
        assert_eq!(
            schema.identities[2],
            ColumnIdentity {
                old: 2,
                new: 1,
                type_changed: false,
                is_key: false,
            }
        );
    }

    #[test]
    fn a_swap_needs_no_minimum_number_of_rows() {
        let old = table! {
            "id" => [1, 2],
            "a" => [10, 20],
            "b" => [30, 40],
        };
        let new = table! {
            "id" => [1, 2],
            "a" => [30, 40],
            "b" => [10, 20],
        };

        // Deliberate, and the one asymmetry with approximate renames: a
        // perfect crossing clears the agreement threshold at any size, so
        // swaps have no implicit row floor, exactly as exact rename inference
        // has never had one.
        assert_eq!(pairs(&infer_swaps(&old, &new)), [(1, 2), (2, 1)]);
    }

    #[test]
    fn columns_that_kept_their_values_are_not_swapped() {
        let old = table! {
            "id" => [1, 2],
            "a" => [10, 20],
            "b" => [30, 40],
        };
        let new = table! {
            "id" => [1, 2],
            "a" => [10, 20],
            "b" => [30, 40],
        };

        assert_eq!(pairs(&infer_swaps(&old, &new)), [(1, 1), (2, 2)]);
    }

    #[test]
    fn a_competing_third_column_leaves_every_identity_alone() {
        let old = table! {
            "id" => [1, 2],
            "a" => [10, 20],
            "b" => [30, 40],
            "c" => [30, 40],
        };
        let new = table! {
            "id" => [1, 2],
            "a" => [30, 40],
            "b" => [10, 20],
            "c" => [10, 20],
        };

        // "a" could equally have been exchanged with "b" or with "c", so it
        // takes part in two candidates and none of them is accepted.
        assert_eq!(pairs(&infer_swaps(&old, &new)), [(1, 1), (2, 2), (3, 3)]);
    }

    #[test]
    fn a_crossing_that_would_change_type_is_not_a_swap() {
        let old = table! {
            "id" => [1, 2],
            "a" => [1, 2],
            "b" => ["x", "y"],
        };
        let new = table! {
            "id" => [1, 2],
            "a" => ["x", "y"],
            "b" => [1.0, 2.0],
        };

        // Old "a" and new "b" hold the same values and compare equal, an
        // integer against a double. That is enough for a rename, which would
        // otherwise have nothing at all to say about the pair, but not for a
        // swap, which is overriding an identity rather than filling a gap.
        assert_eq!(pairs(&infer_swaps(&old, &new)), [(1, 1), (2, 2)]);
    }

    #[test]
    fn incompatible_crossings_are_not_a_swap() {
        let old = table! {
            "id" => [1, 2],
            "a" => [true, false],
            "b" => [30, 40],
        };
        let new = table! {
            "id" => [1, 2],
            "a" => [false, true],
            "b" => [40, 30],
        };

        // Both columns changed, but a boolean and an integer cannot be
        // compared, so there is no evidence that they were exchanged.
        assert_eq!(pairs(&infer_swaps(&old, &new)), [(1, 1), (2, 2)]);
    }

    #[test]
    fn a_key_column_is_never_a_swap_endpoint() {
        let old = table! {
            "id" => [1, 2],
            "a" => [1, 2],
        };
        let new = table! {
            "id" => [1, 2],
            "a" => [1, 2],
        };

        // Rows are identified by the key, so exchanging it would contradict
        // the matching that produced the aligned rows in the first place.
        let schema = infer_swaps(&old, &new);

        assert!(schema.identities[0].is_key);
        assert_eq!(pairs(&schema), [(1, 1)]);
    }

    #[test]
    fn what_rename_inference_established_is_left_alone() {
        let old = table! {
            "id" => [1, 2],
            "kept" => [10, 20],
            "gone" => [30, 40],
        };
        let new = table! {
            "id" => [1, 2],
            "kept" => [50, 60],
            "fresh" => [30, 40],
        };

        let options = DiffOptions {
            key: vec!["id".into()],
            hints: Vec::new(),
        };
        let key = resolve_key(&old, &new, &options).unwrap();
        let rows = match_rows(&key);
        let mut schema = reconcile_schema(&old, &new, &key).unwrap();
        rename::infer(&old, &new, &mut schema, &rows);
        let inferred = schema.clone();
        infer(&old, &new, &mut schema, &rows, &Hints::default());

        // "kept" was rewritten and "gone" became "fresh", and the two stages
        // do not interact: the inferred identity carries different names at
        // its ends, and could not be a swap endpoint regardless, since
        // inference only ever establishes identities that agree closely.
        assert_eq!(schema, inferred);
        assert_eq!(pairs(&schema), [(1, 1), (2, 2)]);
        assert!(schema.dropped.is_empty());
        assert!(schema.added.is_empty());
    }

    #[test]
    fn a_hinted_identity_is_not_reinterpreted_as_a_swap() {
        let old = table! {
            "id" => [1, 2],
            "a" => [10, 20],
            "b" => [30, 40],
        };
        let new = table! {
            "id" => [1, 2],
            "a" => [30, 40],
            "b" => [10, 20],
        };

        let options = DiffOptions {
            key: vec!["id".into()],
            hints: vec!["col_rename(a -> a)".into()],
        };
        let key = resolve_key(&old, &new, &options).unwrap();
        let rows = match_rows(&key);
        let hints = crate::hint::resolve(&old, &new, &options, &[]).unwrap();
        let mut schema = crate::schema::reconcile_schema(&old, &new, &key, &hints).unwrap();
        infer(&old, &new, &mut schema, &rows, &hints);

        // The values would read as an exchange, and a hint says otherwise. Every
        // other exclusion here is about a default reconciliation chose; this one
        // is about an instruction, which inference does not get to overrule.
        assert_eq!(pairs(&schema), [(1, 1), (2, 2)]);
    }

    #[test]
    fn without_matched_rows_there_is_no_evidence() {
        let old = table! {
            "id" => [1, 2],
            "a" => [10, 20],
            "b" => [30, 40],
        };
        let new = table! {
            "id" => [3, 4],
            "a" => [30, 40],
            "b" => [10, 20],
        };

        assert_eq!(pairs(&infer_swaps(&old, &new)), [(1, 1), (2, 2)]);
    }
}
