//! Reinterpret two rewritten same-name columns as one exchange.

use arrow_array::RecordBatch;

use crate::IdentityBasis;
use crate::agreement::Aligned;
use crate::compare::ComparisonPlan;
use crate::hint::{EditHint, edit_protects};
use crate::rows::RowMatches;
use crate::schema::{ColumnMap, ColumnPair};

/// Exchange the ends of two identities whose columns hold each other's values.
///
/// When two same-named columns both change beyond recognition, the likelier
/// account is not that both were rewritten but that their contents were
/// swapped. This reads and rewrites the pairs only: it neither consumes nor
/// produces a drop or an addition, which is what keeps it independent of
/// rename inference.
pub(crate) fn infer(
    old: &RecordBatch,
    new: &RecordBatch,
    map: &mut ColumnMap,
    rows: &RowMatches,
    edits: &[EditHint],
) {
    if rows.matched.is_empty() {
        return;
    }
    let mut values = Aligned::new(old, new, rows);
    // Copies rather than positions in the map, so that the candidates can be
    // weighed against each other before any of them rewires it.
    let eligible = eligible(map, edits);

    let mut candidates = Vec::new();
    for (first, pair) in eligible.iter().enumerate() {
        for (offset, other) in eligible[first + 1..].iter().enumerate() {
            if exchanged(old, new, &mut values, pair, other) {
                candidates.push((first, first + 1 + offset));
            }
        }
    }

    // Competing swaps cancel rather than compete: a column that could plausibly
    // have been exchanged with either of two others is evidence of neither, and
    // the design leaves it to the user rather than to a tie-break.
    let mut claims = vec![0_usize; eligible.len()];
    for &(first, second) in &candidates {
        claims[first] += 1;
        claims[second] += 1;
    }
    for (first, second) in candidates {
        if claims[first] == 1 && claims[second] == 1 {
            // By old position, which identifies a pair whatever has happened to
            // the map since: an exchange moves new ends and leaves old ones be.
            map.exchange(eligible[first].old, eligible[second].old);
        }
    }
}

/// The identities a swap may consume: provisional, same-named, and not a key.
///
/// A pair whose basis is `Name` is exactly a provisional same-name identity,
/// which is what recording the basis was for: every other basis is excluded on
/// its own account. `Declared` and `Hinted` are assertions, and inference does
/// not overrule an instruction — a point `design.md` makes by name, and the one
/// that bites, since a rename hint whose two ends carry the same name would
/// otherwise look exactly like a candidate. `Exact` and `Approximate` pairs
/// agree far too closely to be two columns rewritten past recognition, and
/// `Swapped` has been through here already.
///
/// A guessed key's column is identified by its name like any other, so `is_key`
/// is still a separate question rather than one the basis answers.
///
/// An edit hint excludes an identity too, and does so *as well as* the basis
/// rather than instead of it. A rename hint's identity is protected because the
/// pair records that a hint established it; an edit claims no endpoint and so is
/// not in the map at all, which is why it takes a second question. Withdrawing a
/// swap is the design's stated purpose for the kind, and it is the one place two
/// same-named columns that really were both rewritten can say so.
fn eligible(map: &ColumnMap, edits: &[EditHint]) -> Vec<ColumnPair> {
    map.pairs()
        .iter()
        .filter(|pair| {
            pair.basis == IdentityBasis::Name
                && !pair.is_key
                && !edit_protects(edits, pair.old, pair.new)
        })
        .copied()
        .collect()
}

/// Whether two identities look like each other's exchange.
///
/// Both columns must have been rewritten under their own names, and both
/// crossings must agree closely enough to stand as renames in their own right.
fn exchanged(
    old: &RecordBatch,
    new: &RecordBatch,
    values: &mut Aligned,
    first: &ColumnPair,
    second: &ColumnPair,
) -> bool {
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
    identity: &ColumnPair,
) -> bool {
    let plan = plan_for(old, new, identity.old, identity.new);
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
        && values
            .measure(
                plan_for(old, new, old_index, new_index),
                old_index,
                new_index,
            )
            .is_close()
}

fn plan_for(
    old: &RecordBatch,
    new: &RecordBatch,
    old_index: usize,
    new_index: usize,
) -> ComparisonPlan {
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
    use crate::key::testing::resolve_key;
    use crate::rename;
    use crate::rows::match_rows;
    use crate::schema::ColumnMap;
    use crate::schema::testing::reconcile_schema;
    use crate::{DiffOptions, IdentityBasis};

    fn infer_swaps(old: &RecordBatch, new: &RecordBatch) -> ColumnMap {
        let options = DiffOptions {
            key: vec!["id".into()],
            hints: Vec::new(),
        };
        let key = resolve_key(old, new, &options).unwrap();
        let rows = match_rows(&key);
        let mut schema = reconcile_schema(old, new, &key);
        infer(old, new, &mut schema, &rows, &[]);
        schema
    }

    fn pairs(schema: &ColumnMap) -> Vec<(usize, usize)> {
        schema
            .pairs()
            .iter()
            .filter(|pair| !pair.is_key)
            .map(|pair| (pair.old, pair.new))
            .collect()
    }

    /// Whether any identity the map holds spans two different types.
    ///
    /// Derived from the pairs the way `compare_cells` derives it, no pair
    /// carrying a type change of its own for an exchange to invalidate.
    fn type_changed(old: &RecordBatch, new: &RecordBatch, schema: &ColumnMap) -> bool {
        schema
            .pairs()
            .iter()
            .any(|pair| old.column(pair.old).data_type() != new.column(pair.new).data_type())
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
        // why: each column was being compared with the wrong one. Nothing
        // carries that conclusion around: the type change is derived from the
        // pair the map ends up holding, so exchanging the ends dissolves it.
        assert_eq!(pairs(&schema), [(1, 2), (2, 1)]);
        assert_eq!(basis(&schema, 1), IdentityBasis::Swapped);
        assert_eq!(basis(&schema, 2), IdentityBasis::Swapped);
        assert!(!type_changed(&old, &new, &schema));
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

        assert!(schema.pairs()[0].is_key);
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
        let mut schema = reconcile_schema(&old, &new, &key);
        rename::infer(&old, &new, &mut schema, &rows);
        let inferred = schema.clone();
        infer(&old, &new, &mut schema, &rows, &[]);

        // "kept" was rewritten and "gone" became "fresh", and the two stages
        // do not interact: the inferred identity carries different names at
        // its ends, and could not be a swap endpoint regardless, since
        // inference only ever establishes identities that agree closely.
        assert_eq!(schema, inferred);
        assert_eq!(pairs(&schema), [(1, 1), (2, 2)]);
        assert!(schema.dropped().is_empty());
        assert!(schema.added().is_empty());
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
        let hints = crate::hint::resolve(
            old.schema_ref(),
            new.schema_ref(),
            &options.hints,
            ColumnMap::new(old.schema_ref(), new.schema_ref()),
        )
        .unwrap();
        let mut schema = hints.map.clone();
        crate::schema::reconcile_schema(&old, &new, &key, &mut schema);
        infer(&old, &new, &mut schema, &rows, &hints.edits);

        // The values would read as an exchange, and a hint says otherwise. Every
        // other exclusion here is about a default reconciliation chose; this one
        // is about an instruction, which inference does not get to overrule.
        assert_eq!(pairs(&schema), [(1, 1), (2, 2)]);
    }

    #[test]
    fn an_edited_identity_is_not_reinterpreted_as_a_swap() {
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
            hints: vec!["col_edit(a)".into()],
        };
        let key = resolve_key(&old, &new, &options).unwrap();
        let rows = match_rows(&key);
        let hints = crate::hint::resolve(
            old.schema_ref(),
            new.schema_ref(),
            &options.hints,
            ColumnMap::new(old.schema_ref(), new.schema_ref()),
        )
        .unwrap();
        let mut schema = hints.map.clone();
        crate::schema::reconcile_schema(&old, &new, &key, &mut schema);
        infer(&old, &new, &mut schema, &rows, &hints.edits);

        // An edit claims no endpoint, so the map knows nothing about it and the
        // hinted-pair exclusion cannot see it. Naming one of the two columns is
        // enough to withdraw the swap: an exchange takes two, and this one has
        // lost an end.
        assert!(hints.map.pairs().is_empty());
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
