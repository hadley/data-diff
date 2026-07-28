//! What the user asserts when reconciliation cannot work it out.
//!
//! A hint is written in the same line grammar the human format prints, so the
//! operation a user wants to see is the one they type. Only the subset hints
//! occupy is read here — a kind applied to a name, or to a pair of names —
//! because a hint asserts identity and nothing else.

use std::collections::HashMap;

use arrow_array::RecordBatch;

use crate::compare::ComparisonPlan;
use crate::{DiffError, DiffOptions, HintClaim, HintKind, Issue, IssueKind, Side};

/// The identities hints established, and what had to be declined to get them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Hints {
    /// Accepted identities, by column position, ascending by old position.
    pub identities: Vec<HintIdentity>,
    pub issues: Vec<Issue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HintIdentity {
    pub old: usize,
    pub new: usize,
}

impl Hints {
    /// The new column a hint identified this old column with.
    pub(crate) fn new_for_old(&self, old: usize) -> Option<usize> {
        self.identities
            .iter()
            .find(|identity| identity.old == old)
            .map(|identity| identity.new)
    }

    /// Whether a hint asserted this identity.
    pub(crate) fn asserted(&self, old: usize, new: usize) -> bool {
        self.new_for_old(old) == Some(new)
    }

    /// The old column a hint identified this new column with.
    pub(crate) fn old_for_new(&self, new: usize) -> Option<usize> {
        self.identities
            .iter()
            .find(|identity| identity.new == new)
            .map(|identity| identity.old)
    }
}

/// Parse, validate, and apply every hint, given what the key already claims.
///
/// `key_claims` are the name pairs a declared key asserts. They are settled
/// before hints are considered rather than competing with them: a key is
/// load-bearing for row matching, so letting a mistyped hint invalidate one
/// would trade an ignored instruction for a wrong answer about every row.
pub(crate) fn resolve(
    old: &RecordBatch,
    new: &RecordBatch,
    options: &DiffOptions,
    key_claims: &[(String, String)],
) -> Result<Hints, DiffError> {
    let claims = parse_all(&options.hints)?;
    let mut result = Hints::default();

    // Endpoints first, so a hint naming a column that is not there is reported
    // as the mistake it is rather than counted as a claim on something.
    let mut resolved: Vec<(HintClaim, HintIdentity)> = Vec::new();
    for claim in claims {
        match endpoints(old, new, &claim) {
            // Identity is judged after resolution, so a quoted and a bare
            // spelling of one claim collapse rather than contradicting.
            Ok(endpoints) if resolved.iter().any(|(_, seen)| *seen == endpoints) => {}
            Ok(endpoints) => resolved.push((claim, endpoints)),
            Err(issue) => result.issues.push(issue),
        }
    }

    let rejected = contradictions(&resolved, key_claims);
    for group in &rejected {
        result.issues.push(Issue {
            kind: IssueKind::ContradictoryHints,
            hints: group
                .iter()
                .map(|&index| resolved[index].0.clone())
                .collect(),
        });
    }
    let rejected = rejected.concat();

    for (index, (_, endpoints)) in resolved.iter().enumerate() {
        if !rejected.contains(&index) {
            result.identities.push(*endpoints);
        }
    }
    result.identities.sort_by_key(|identity| identity.old);
    Ok(result)
}

/// Resolve a claim's two names to column positions.
///
/// Both columns must exist, and their values must be comparable. An identity
/// between a boolean and an integer would be accepted by everything up to cell
/// comparison and rejected there, taking the whole diff with it; a hint the data
/// cannot support is declined like any other, and the rest of the comparison
/// stands.
fn endpoints(
    old: &RecordBatch,
    new: &RecordBatch,
    claim: &HintClaim,
) -> Result<HintIdentity, Issue> {
    let issue = |kind: IssueKind| Issue {
        kind,
        hints: vec![claim.clone()],
    };
    let missing = |side: Side, column: &str| {
        issue(IssueKind::HintMissingTarget {
            side,
            column: column.to_owned(),
        })
    };
    let position = |table: &RecordBatch, name: &str| {
        table
            .schema()
            .fields()
            .iter()
            .position(|field| field.name() == name)
    };
    let old_index = position(old, &claim.old).ok_or_else(|| missing(Side::Old, &claim.old))?;
    let new_index = position(new, &claim.new).ok_or_else(|| missing(Side::New, &claim.new))?;

    let old_type = old.column(old_index).data_type();
    let new_type = new.column(new_index).data_type();
    if ComparisonPlan::new(old_type, new_type).is_none() {
        return Err(issue(IssueKind::HintIncompatibleTypes {
            old_type: format!("{old_type:?}"),
            new_type: format!("{new_type:?}"),
        }));
    }
    Ok(HintIdentity {
        old: old_index,
        new: new_index,
    })
}

/// Group the claims that cannot all hold, so that none of a group is applied.
///
/// Claims form a bipartite graph, each an edge from an old endpoint to a new
/// one, and a valid set is a matching. Rejecting a whole connected group rather
/// than picking a winner is what keeps input order out of the answer: given
/// `a -> b` and `a -> c`, keeping the first would mean the result depended on
/// which flag came first. Groups rather than single edges because a chain of
/// claims can be contradictory without any one endpoint looking wrong alone.
fn contradictions(
    resolved: &[(HintClaim, HintIdentity)],
    key_claims: &[(String, String)],
) -> Vec<Vec<usize>> {
    let mut by_old: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut by_new: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, (_, endpoints)) in resolved.iter().enumerate() {
        by_old.entry(endpoints.old).or_default().push(index);
        by_new.entry(endpoints.new).or_default().push(index);
    }

    let mut contested = vec![false; resolved.len()];
    // A hint sharing one endpoint with a key component but not the other is
    // contested on its own, no second hint required: the key has already
    // claimed that column for something else.
    for (index, (claim, _)) in resolved.iter().enumerate() {
        if key_claims
            .iter()
            .any(|(old, new)| (old == &claim.old) != (new == &claim.new))
        {
            contested[index] = true;
        }
    }
    for shared in by_old.values().chain(by_new.values()) {
        if shared.len() > 1 {
            for &index in shared {
                contested[index] = true;
            }
        }
    }

    // Grow each contested claim into its connected component, so a group is
    // rejected whole even where only one of its edges looked wrong.
    let mut grouped = vec![false; resolved.len()];
    let mut groups = Vec::new();
    for start in 0..resolved.len() {
        if !contested[start] || grouped[start] {
            continue;
        }
        let mut group = vec![start];
        grouped[start] = true;
        let mut frontier = vec![start];
        while let Some(index) = frontier.pop() {
            let endpoints = resolved[index].1;
            let neighbours = by_old
                .get(&endpoints.old)
                .into_iter()
                .chain(by_new.get(&endpoints.new))
                .flatten();
            for &neighbour in neighbours {
                if !grouped[neighbour] {
                    grouped[neighbour] = true;
                    group.push(neighbour);
                    frontier.push(neighbour);
                }
            }
        }
        group.sort_unstable();
        groups.push(group);
    }
    groups
}

/// Parse every supplied spelling, rejecting the first that is not a hint.
fn parse_all(spellings: &[String]) -> Result<Vec<HintClaim>, DiffError> {
    spellings.iter().map(|spelling| parse(spelling)).collect()
}

/// Read one line of the grammar and interpret it as a claim.
fn parse(spelling: &str) -> Result<HintClaim, DiffError> {
    let malformed = || DiffError::MalformedHint {
        hint: spelling.to_owned(),
    };
    let trimmed = spelling.trim();
    let open = trimmed.find('(').ok_or_else(malformed)?;
    if !trimmed.ends_with(')') {
        return Err(malformed());
    }
    let kind = trimmed[..open].trim();
    let arguments = &trimmed[open + 1..trimmed.len() - 1];

    let kind = match kind {
        "col_rename" => HintKind::Rename,
        _ => {
            return Err(DiffError::UnknownHintKind {
                hint: spelling.to_owned(),
                kind: kind.to_owned(),
            });
        }
    };
    let (old, new) = split_pair(arguments).ok_or_else(malformed)?;
    Ok(HintClaim {
        kind,
        old: name(old).ok_or_else(malformed)?,
        new: name(new).ok_or_else(malformed)?,
    })
}

/// Split an argument list on the grammar's `old -> new` arrow.
///
/// The arrow is found outside quotes, so a name containing one can be spelled
/// by quoting it.
fn split_pair(arguments: &str) -> Option<(&str, &str)> {
    let bytes = arguments.as_bytes();
    let mut quoted = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if quoted => index += 1,
            b'"' => quoted = !quoted,
            b'-' if !quoted && arguments[index..].starts_with("->") => {
                return Some((&arguments[..index], &arguments[index + 2..]));
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// Read one argument as a column name.
///
/// A bare name is trimmed, so `col_rename(a -> b)` names what it looks like; a
/// quoted one is decoded exactly by the same JSON rules the output encodes
/// with, which is what lets any legal column name be written.
///
/// A bare name may not contain the grammar's own punctuation. Otherwise
/// `col_rename(a -> b -> c)` would quietly name a column `b -> c`, and
/// `col_rename(a, b)` a column `a, b` — both far likelier to be a user meaning
/// something else than a column really called that. Quoting is how such a name
/// is written, and the error says so.
fn name(argument: &str) -> Option<String> {
    const PUNCTUATION: [char; 6] = ['"', ',', '(', ')', '[', ']'];

    let trimmed = argument.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('"') {
        return serde_json::from_str(trimmed).ok();
    }
    (!trimmed.contains(PUNCTUATION) && !trimmed.contains("->")).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use arrow_array::RecordBatch;
    use test_support::table;

    use super::{Hints, resolve};
    use crate::{DiffError, DiffOptions, HintKind, IssueKind, Side};

    fn hint_for(old: &RecordBatch, new: &RecordBatch, hints: &[&str]) -> Hints {
        try_hints(old, new, hints, &[]).unwrap()
    }

    fn try_hints(
        old: &RecordBatch,
        new: &RecordBatch,
        hints: &[&str],
        key_claims: &[(&str, &str)],
    ) -> Result<Hints, DiffError> {
        let options = DiffOptions {
            key: Vec::new(),
            hints: hints.iter().map(|hint| (*hint).to_owned()).collect(),
        };
        let claims = key_claims
            .iter()
            .map(|(old, new)| ((*old).to_owned(), (*new).to_owned()))
            .collect::<Vec<_>>();
        resolve(old, new, &options, &claims)
    }

    fn tables() -> (RecordBatch, RecordBatch) {
        (
            table! { "id" => [1], "gone" => [1], "other" => [1] },
            table! { "id" => [1], "fresh" => [1], "extra" => [1] },
        )
    }

    fn pairs(hints: &Hints) -> Vec<(usize, usize)> {
        hints
            .identities
            .iter()
            .map(|identity| (identity.old, identity.new))
            .collect()
    }

    #[test]
    fn a_bare_and_a_quoted_spelling_mean_the_same_thing() {
        let (old, new) = tables();

        assert_eq!(
            pairs(&hint_for(&old, &new, &["col_rename(gone -> fresh)"])),
            [(1, 1)]
        );
        assert_eq!(
            pairs(&hint_for(&old, &new, &[r#"col_rename("gone" -> "fresh")"#])),
            [(1, 1)]
        );
    }

    #[test]
    fn identical_claims_collapse_rather_than_conflicting() {
        let (old, new) = tables();

        // Two spellings of one claim are one claim; treating them as two would
        // make a duplicate contradict itself.
        let hints = hint_for(
            &old,
            &new,
            &[
                "col_rename(gone -> fresh)",
                r#"col_rename("gone" -> "fresh")"#,
            ],
        );

        assert_eq!(pairs(&hints), [(1, 1)]);
        assert!(hints.issues.is_empty());
    }

    #[test]
    fn quoting_reaches_names_the_grammar_would_otherwise_eat() {
        let old = table! {
            "id" => [1],
            "a, b" => [1],
            "c -> d" => [1],
            "line\n\"quoted\"" => [1],
        };
        let new = table! {
            "id" => [1],
            "x" => [1],
            "y" => [1],
            "z" => [1],
        };

        let hints = hint_for(
            &old,
            &new,
            &[
                r#"col_rename("a, b" -> x)"#,
                r#"col_rename("c -> d" -> y)"#,
                r#"col_rename("line\n\"quoted\"" -> z)"#,
            ],
        );

        assert_eq!(pairs(&hints), [(1, 1), (2, 2), (3, 3)]);
    }

    #[test]
    fn bare_names_are_trimmed_and_quoted_names_are_not() {
        let old = table! { "id" => [1], " padded " => [1] };
        let new = table! { "id" => [1], "trimmed" => [1] };

        // A bare argument cannot name a column with significant spaces, which
        // is what quoting is for.
        assert!(
            hint_for(&old, &new, &["col_rename( padded  -> trimmed )"])
                .identities
                .is_empty()
        );
        assert_eq!(
            pairs(&hint_for(
                &old,
                &new,
                &[r#"col_rename(" padded " -> trimmed)"#]
            )),
            [(1, 1)]
        );
    }

    #[test]
    fn a_hint_that_is_not_a_hint_is_an_error() {
        let (old, new) = tables();

        for spelling in [
            "col_rename(gone -> fresh",
            "col_rename gone -> fresh",
            "col_rename(gone)",
            "col_rename(gone -> )",
            // Both of these would otherwise name a column containing the
            // grammar's punctuation, which is almost certainly not what was
            // meant; quoting is how such a name is written.
            "col_rename(gone -> fresh -> other)",
            "col_rename(gone, fresh)",
        ] {
            assert!(
                try_hints(&old, &new, &[spelling], &[]).is_err(),
                "{spelling}"
            );
        }
    }

    #[test]
    fn an_unrecognized_kind_names_itself() {
        let (old, new) = tables();

        assert_eq!(
            try_hints(&old, &new, &["col_drop(gone)"], &[]).unwrap_err(),
            DiffError::UnknownHintKind {
                hint: "col_drop(gone)".into(),
                kind: "col_drop".into(),
            }
        );
    }

    #[test]
    fn a_missing_target_is_reported_on_the_side_that_lacks_it() {
        let (old, new) = tables();

        let missing_new = hint_for(&old, &new, &["col_rename(gone -> absent)"]);
        let missing_old = hint_for(&old, &new, &["col_rename(absent -> fresh)"]);

        assert!(missing_new.identities.is_empty());
        assert_eq!(
            missing_new.issues[0].kind,
            IssueKind::HintMissingTarget {
                side: Side::New,
                column: "absent".into(),
            }
        );
        assert_eq!(missing_new.issues[0].hints[0].kind, HintKind::Rename);
        assert_eq!(
            missing_old.issues[0].kind,
            IssueKind::HintMissingTarget {
                side: Side::Old,
                column: "absent".into(),
            }
        );
    }

    #[test]
    fn a_claim_the_values_cannot_support_is_declined() {
        let old = table! { "id" => [1], "flag" => [true] };
        let new = table! { "id" => [1], "count" => [1] };

        // A boolean and an integer cannot be compared, so this identity would
        // have been accepted here and then killed the whole diff at cell
        // comparison. Declining it leaves the rest of the run standing.
        let hints = hint_for(&old, &new, &["col_rename(flag -> count)"]);

        assert!(hints.identities.is_empty());
        assert_eq!(
            hints.issues[0].kind,
            IssueKind::HintIncompatibleTypes {
                old_type: "Boolean".into(),
                new_type: "Int64".into(),
            }
        );
    }

    #[test]
    fn one_old_column_claimed_for_two_new_ones_rejects_both() {
        let (old, new) = tables();

        let hints = hint_for(
            &old,
            &new,
            &["col_rename(gone -> fresh)", "col_rename(gone -> extra)"],
        );

        assert!(hints.identities.is_empty());
        assert_eq!(hints.issues.len(), 1);
        assert_eq!(hints.issues[0].kind, IssueKind::ContradictoryHints);
        assert_eq!(hints.issues[0].hints.len(), 2);
    }

    #[test]
    fn two_old_columns_claiming_one_new_one_reject_both() {
        let (old, new) = tables();

        let hints = hint_for(
            &old,
            &new,
            &["col_rename(gone -> fresh)", "col_rename(other -> fresh)"],
        );

        assert!(hints.identities.is_empty());
        assert_eq!(hints.issues[0].hints.len(), 2);
    }

    #[test]
    fn a_chain_of_claims_is_rejected_whole() {
        let old = table! { "id" => [1], "a" => [1], "b" => [1] };
        let new = table! { "id" => [1], "x" => [1], "y" => [1] };

        // Only "a" is doubly claimed, but rejecting its two edges alone would
        // leave "b -> x" standing on an endpoint whose rival just vanished.
        let hints = hint_for(
            &old,
            &new,
            &[
                "col_rename(a -> x)",
                "col_rename(a -> y)",
                "col_rename(b -> x)",
            ],
        );

        assert!(hints.identities.is_empty());
        assert_eq!(hints.issues.len(), 1);
        assert_eq!(hints.issues[0].hints.len(), 3);
    }

    #[test]
    fn an_independent_hint_survives_a_rejected_group() {
        let old = table! { "id" => [1], "a" => [1], "b" => [1], "keep" => [1] };
        let new = table! { "id" => [1], "x" => [1], "y" => [1], "kept" => [1] };

        let hints = hint_for(
            &old,
            &new,
            &[
                "col_rename(a -> x)",
                "col_rename(a -> y)",
                "col_rename(keep -> kept)",
            ],
        );

        // Group rejection is local: it drops the claims that conflict, not
        // every claim in the invocation.
        assert_eq!(pairs(&hints), [(3, 3)]);
        assert_eq!(hints.issues.len(), 1);
    }

    #[test]
    fn a_key_component_beats_a_hint_that_contradicts_it() {
        let old = table! { "id" => [1], "gone" => [1] };
        let new = table! { "code" => [1], "fresh" => [1] };

        // The key pairs old "id" with new "code"; the hint wants old "id"
        // elsewhere. The key is load-bearing for row matching, so the hint is
        // what gives way.
        let hints = try_hints(&old, &new, &["col_rename(id -> fresh)"], &[("id", "code")]).unwrap();

        assert!(hints.identities.is_empty());
        assert_eq!(hints.issues[0].kind, IssueKind::ContradictoryHints);
    }

    #[test]
    fn a_hint_agreeing_with_a_key_component_is_left_alone() {
        let old = table! { "id" => [1], "gone" => [1] };
        let new = table! { "code" => [1], "fresh" => [1] };

        let hints = try_hints(
            &old,
            &new,
            &["col_rename(id -> code)", "col_rename(gone -> fresh)"],
            &[("id", "code")],
        )
        .unwrap();

        // Redundant rather than contradictory: both say the same thing.
        assert_eq!(pairs(&hints), [(0, 0), (1, 1)]);
        assert!(hints.issues.is_empty());
    }
}
