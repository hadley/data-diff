//! Reconsider the key once, when the first resolution proves wrong.
//!
//! A key the tool chose — guessed or fallen back to — is a judgement, and the
//! diff it produces is evidence about the judgement itself. Two findings warrant
//! a second look: pass one's inference established an identity that makes a
//! better key guessable, or a guessed key produced a diff in which more changed
//! than stayed the same. Either way the key is re-resolved once, with what pass
//! one learned, and whatever the second pass produces is final. A declared key
//! is never reconsidered: it is the user's assertion, not the tool's to
//! withdraw.

use arrow_array::RecordBatch;

use crate::cells::CellChanges;
use crate::key::{self, ResolvedKey};
use crate::rows::RowMatches;
use crate::schema::ColumnMap;
use crate::{ChangeMass, IdentityBasis, KeyBasis, KeyComponent, KeyRetraction};

/// The share of a comparison's cell mass a diff may account as changed and
/// still be read as a story of edits.
///
/// Past it, change outweighs sameness and the description has stopped
/// compressing: a guessed key producing such a diff is retracted, and a final
/// diff past it under a judgement basis is reported as a regeneration.
pub(crate) const MAX_PLAUSIBLE_CHANGE_PERCENT: usize = 50;

/// Whether a diff accounts more of the files as changed than the limit allows.
///
/// Exact integer arithmetic, strict at the limit, so an empty comparison — a
/// mass of nothing over nothing — is never implausible.
pub(crate) fn implausible(mass: ChangeMass) -> bool {
    mass.changed * 100 > mass.total * MAX_PLAUSIBLE_CHANGE_PERCENT
}

/// Measure how much of the two files a pass's diff accounts as changed.
///
/// Cell-denominated and symmetric: a dropped or added row contributes its whole
/// width once, and a changed matched cell exists in both files and contributes
/// two. Rows are too coarse a unit — one changed cell in a wide row is a normal
/// edit, while a positional matching over reordered rows changes nearly every
/// cell, and it is the second this measure exists to catch.
///
/// Fanout groups leave both counts. Their own limit caps affected keys, not
/// rows, so one key may fan out to arbitrarily many new rows; counting those
/// rows in the total while nothing of them can appear in the changed mass would
/// let a large fanout dilute the ratio and hide an implausible matching.
///
/// Under the fallback basis, added rows contribute nothing to `changed` while
/// staying in `total`: positional matching puts every addition at the tail, and
/// a longer new file is read as rows appended at the end — the one operation
/// positional matching is exactly right about, corroborated or refuted by the
/// matched prefix's own cells. The assumption is deliberately not symmetric.
/// Dropped rows always count, because rows are usually deleted by filtering,
/// from anywhere, and the position shift that causes is precisely the
/// misreading being tested for. Under a guessed key both count in full: there
/// the key identifies rows by value and an addition is a claim it vouches for.
pub(crate) fn change_mass(
    old: &RecordBatch,
    new: &RecordBatch,
    basis: KeyBasis,
    rows: &RowMatches,
    cells: &CellChanges,
) -> ChangeMass {
    let old_width = old.num_columns();
    let new_width = new.num_columns();
    let fanout_new = rows
        .fanout
        .iter()
        .map(|group| group.new.len())
        .sum::<usize>();
    let changed_cells = cells
        .columns
        .iter()
        .map(|column| column.rows.len())
        .sum::<usize>();
    let added = match basis {
        KeyBasis::Fallback => 0,
        KeyBasis::Declared | KeyBasis::Guessed => rows.added.len(),
    };
    ChangeMass {
        changed: rows.dropped.len() * old_width + added * new_width + 2 * changed_cells,
        total: (old.num_rows() - rows.fanout.len()) * old_width
            + (new.num_rows() - fanout_new) * new_width,
    }
}

/// What reconsideration decided about pass one.
pub(crate) struct Reconsideration {
    /// The key a second pass should run with, or `None` when pass one stands.
    pub key: Option<ResolvedKey>,
    /// The guessed key that was withdrawn, where trigger was implausibility.
    pub retraction: Option<KeyRetraction>,
}

/// Judge pass one's key against its own diff, once.
///
/// Two triggers, either sufficient. A guessed key whose diff is implausible is
/// retracted — reported, excluded, and the guess rerun without it, landing on
/// the next candidate or the fallback. And rerunning the guess with pass one's
/// final map may find a winner the first resolution could not see, because an
/// inferred identity is now a candidate; evaluating that trigger *is* running
/// the pass-two guess, so a candidate that does not qualify or does not outrank
/// the incumbent changes nothing. An implausible fallback with nothing new to
/// offer goes directly to regeneration reporting instead: there is nothing
/// below the fallback to retract it to.
///
/// The caller runs at most one second pass and never calls this on its result,
/// which is the once-only rule, held structurally rather than by a counter.
pub(crate) fn reconsider(
    old: &RecordBatch,
    new: &RecordBatch,
    key: &ResolvedKey,
    map: &ColumnMap,
    rows: &RowMatches,
    cells: &CellChanges,
) -> Reconsideration {
    if key.basis == KeyBasis::Declared {
        return Reconsideration {
            key: None,
            retraction: None,
        };
    }

    let mut excluded = Vec::new();
    let mut retraction = None;
    if key.basis == KeyBasis::Guessed {
        let mass = change_mass(old, new, key.basis, rows, cells);
        if implausible(mass) {
            excluded = key
                .columns
                .iter()
                .map(|column| (column.old, column.new))
                .collect();
            retraction = Some(KeyRetraction {
                columns: key
                    .columns
                    .iter()
                    .map(|column| KeyComponent {
                        old: old.schema().field(column.old).name().clone(),
                        new: new.schema().field(column.new).name().clone(),
                    })
                    .collect(),
                mass,
            });
        }
    }

    let candidate = key::guess_key(old, new, map, &excluded);
    let second = if retraction.is_some() {
        // A retracted guess always yields the chain: the next candidate, or
        // the fallback when nothing else can identify a row.
        Some(candidate.unwrap_or_else(|| key::positional_key(old, new, KeyBasis::Fallback)))
    } else {
        // Without a retraction, only a different winner is worth a second
        // pass; from a fallback, any winner at all differs.
        candidate.filter(|candidate| endpoints(candidate) != endpoints(key))
    };
    let Some(mut second) = second else {
        return Reconsideration {
            key: None,
            retraction: None,
        };
    };
    // The declared rejection survives whichever pass wins, so the whole chain
    // stays visible: a rejected declaration, a retracted guess, a final key.
    second.rejection = key.rejection.clone();
    Reconsideration {
        key: Some(second),
        retraction,
    }
}

fn endpoints(key: &ResolvedKey) -> Vec<(usize, usize)> {
    key.columns
        .iter()
        .map(|column| (column.old, column.new))
        .collect()
}

/// The identities pass two starts with beyond what hints claimed.
///
/// Exactly the pair the adopted key is made of, with the basis pass one
/// established for it, so the rename it rests on keeps saying how it was found.
/// Everything else pass one inferred was derived from a matching the caller has
/// just set aside, and must be re-derived over the new one or not at all.
///
/// A `swapped` basis carries one thing more: the exchange's companion identity.
/// The map creates `swapped` pairs only by exchanging two at once and every
/// consumer may assume they occur in pairs, so adopting half an exchange would
/// leave a `swapped` identity with no exchange behind it. The companion is the
/// pair whose names cross back — its old column bears the adopted new end's
/// name and its new column the adopted old end's — which is what a swap of two
/// same-named identities always leaves.
pub(crate) fn adopted_claims(
    old: &RecordBatch,
    new: &RecordBatch,
    key: &ResolvedKey,
    first: &ColumnMap,
) -> Vec<(usize, usize, IdentityBasis)> {
    let old_schema = old.schema();
    let new_schema = new.schema();
    let mut claims = Vec::new();
    for column in &key.columns {
        let basis = first
            .pairs()
            .iter()
            .find(|pair| pair.old == column.old && pair.new == column.new)
            .map(|pair| pair.basis)
            // A winner the map does not hold reached the guess by name, and by
            // name is what its claim will say.
            .unwrap_or(IdentityBasis::Name);
        claims.push((column.old, column.new, basis));
        if basis == IdentityBasis::Swapped {
            let adopted_old = old_schema.field(column.old).name();
            let adopted_new = new_schema.field(column.new).name();
            if let Some(companion) = first.pairs().iter().find(|pair| {
                pair.basis == IdentityBasis::Swapped
                    && pair.old != column.old
                    && old_schema.field(pair.old).name() == adopted_new
                    && new_schema.field(pair.new).name() == adopted_old
            }) {
                claims.push((companion.old, companion.new, IdentityBasis::Swapped));
            }
        }
    }
    claims
}

#[cfg(test)]
mod tests {
    use test_support::table;

    use super::{change_mass, implausible};
    use crate::cells::{CellChanges, ColumnChanges};
    use crate::rows::{FanoutGroup, RowMatches};
    use crate::{ChangeMass, KeyBasis};

    #[test]
    fn exactly_half_is_still_plausible() {
        // Strict at the limit: half the mass changed is the boundary, and the
        // boundary reads as edits.
        assert!(!implausible(ChangeMass {
            changed: 1,
            total: 2
        }));
        assert!(implausible(ChangeMass {
            changed: 51,
            total: 100
        }));
    }

    #[test]
    fn an_empty_comparison_is_never_implausible() {
        assert!(!implausible(ChangeMass {
            changed: 0,
            total: 0
        }));
    }

    #[test]
    fn mass_counts_drops_adds_and_both_sides_of_a_changed_cell() {
        let old = table! { "a" => [1, 2], "b" => [1, 2] };
        let new = table! { "a" => [1, 3], "b" => [1, 3] };
        let rows = RowMatches {
            matched: vec![(0, 0)],
            dropped: vec![1],
            added: vec![1],
            fanout: vec![],
        };
        let cells = CellChanges {
            columns: vec![ColumnChanges {
                old: 1,
                new: 1,
                type_changed: false,
                rows: vec![(0, 0)],
            }],
            fanout: vec![],
        };

        // One dropped row of width two, one added row of width two, and one
        // changed cell present in both files.
        assert_eq!(
            change_mass(&old, &new, KeyBasis::Guessed, &rows, &cells),
            ChangeMass {
                changed: 2 + 2 + 2,
                total: 8
            }
        );
    }

    #[test]
    fn fanout_rows_leave_both_counts() {
        let old = table! { "v" => [1, 2, 3, 4] };
        let new = table! { "v" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] };
        let rows = RowMatches {
            matched: vec![(0, 0)],
            dropped: vec![1, 2],
            added: vec![9],
            fanout: vec![FanoutGroup {
                old: 3,
                new: (1..9).collect(),
            }],
        };
        let cells = CellChanges::default();

        // The fanout's one old row and eight new rows are outside the total,
        // so a large fanout can neither look like change nor dilute it: with
        // those rows counted the ratio would fall to three of fourteen, and
        // this matching would pass as plausible.
        let mass = change_mass(&old, &new, KeyBasis::Guessed, &rows, &cells);
        assert_eq!(
            mass,
            ChangeMass {
                changed: 3,
                total: 5
            }
        );
        assert!(implausible(mass));
    }

    #[test]
    fn a_fallback_reads_a_longer_file_as_appended_rows() {
        let old = table! { "v" => [1, 2] };
        let new = table! { "v" => [1, 2, 3, 4, 5, 6, 7, 8] };
        let rows = RowMatches {
            matched: vec![(0, 0), (1, 1)],
            dropped: vec![],
            added: (2..8).collect(),
            fanout: vec![],
        };
        let cells = CellChanges::default();

        // Appending preserves every pre-existing row's position, so the tail
        // is what positional matching is exactly right about: no change at
        // all. Under a guessed key the same additions are ordinary evidence
        // and count in full.
        assert_eq!(
            change_mass(&old, &new, KeyBasis::Fallback, &rows, &cells),
            ChangeMass {
                changed: 0,
                total: 10
            }
        );
        assert_eq!(
            change_mass(&old, &new, KeyBasis::Guessed, &rows, &cells),
            ChangeMass {
                changed: 6,
                total: 10
            }
        );
    }

    #[test]
    fn a_fallback_gives_a_shorter_file_no_such_reading() {
        let old = table! { "v" => [1, 2, 3, 4, 5, 6, 7, 8] };
        let new = table! { "v" => [1, 2] };
        let rows = RowMatches {
            matched: vec![(0, 0), (1, 1)],
            dropped: (2..8).collect(),
            added: vec![],
            fanout: vec![],
        };
        let cells = CellChanges::default();

        // Deletion usually happens by filtering, from anywhere, so a shorter
        // file earns no appended-tail assumption in reverse: the dropped rows
        // keep their full width and this truncation reads as implausible.
        let mass = change_mass(&old, &new, KeyBasis::Fallback, &rows, &cells);
        assert_eq!(
            mass,
            ChangeMass {
                changed: 6,
                total: 10
            }
        );
        assert!(implausible(mass));
    }
}
