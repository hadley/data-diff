//! What the user asserts when reconciliation cannot work it out.
//!
//! A hint is written in the same line grammar the human format prints, so the
//! operation a user wants to see is the one they type. Only the subset hints
//! occupy is read here — a kind applied to a name, or to a pair of names.
//!
//! The four kinds make three different kinds of claim. `col_rename` claims two
//! endpoints as one column; `col_drop` and `col_add` claim one endpoint as
//! having no partner; `col_edit` claims no endpoint at all, attaching instead to
//! an identity that something else established. What they share is everything
//! else: the grammar, endpoint resolution, deduplication, and the contest.

use arrow_schema::Schema;

use crate::cells::CellChanges;
use crate::compare::ComparisonPlan;
use crate::schema::ColumnMap;
use crate::{DiffError, HintClaim, HintKind, HintNames, IdentityBasis, Issue, IssueKind, Side};

/// What hints established, and what had to be declined to establish it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hints {
    /// The bijection so far, which reconciliation goes on to complete.
    pub map: ColumnMap,
    /// Edit hints, which attach to identities that do not exist yet.
    pub edits: Vec<EditHint>,
    pub issues: Vec<PendingIssue>,
}

/// An issue and the hint it belongs beside.
///
/// Issues arise on both sides of the comparison: everything but an edit is
/// settled before the key is resolved, while an edit cannot be judged until the
/// cells have been compared. A reader should not have to see that seam, so the
/// supplied position travels with the issue and orders the lot at the end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingIssue {
    pub at: usize,
    pub issue: Issue,
}

/// An edit hint, resolved to whichever of its endpoints exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EditHint {
    pub at: usize,
    pub claim: HintClaim,
    pub old: Option<usize>,
    pub new: Option<usize>,
}

impl EditHint {
    /// Whether this hint is about the identity between these two columns.
    ///
    /// An endpoint the hint did not resolve constrains nothing: `col_edit(total)`
    /// where only the new file has that name is about whichever old column ends
    /// up paired with it, which is what lets a `col_edit()` line printed about
    /// an inferred rename be handed straight back.
    pub(crate) fn attaches_to(&self, old: usize, new: usize) -> bool {
        self.old.is_none_or(|index| index == old) && self.new.is_none_or(|index| index == new)
    }
}

/// Whether an edit hint protects this identity from being reinterpreted.
///
/// Over the edits rather than over `Hints`, because by the time anyone asks the
/// map has left: reconciliation owns it from key resolution onwards, and what
/// remains of the hints is the edits waiting for an identity to attach to.
pub(crate) fn edit_protects(edits: &[EditHint], old: usize, new: usize) -> bool {
    edits.iter().any(|edit| edit.attaches_to(old, new))
}

/// What a resolved hint asserts against the bijection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Claim {
    /// Both endpoints are one column.
    Identity { old: usize, new: usize },
    /// This endpoint has no partner.
    Unmatched { side: Side, index: usize },
    /// The identity holding these endpoints, claiming neither exclusively.
    Edit {
        old: Option<usize>,
        new: Option<usize>,
    },
}

/// What a claim says about one endpoint.
///
/// Reducing every kind to this is what makes one rule cover the conflicts the
/// design lists separately — an endpoint renamed twice, an endpoint both renamed
/// and dropped, an edit contradicted by an add or a drop. They are all two
/// claims saying different things about one endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Assertion {
    /// Paired, with a named partner where the claim knows one.
    Partner(Option<usize>),
    /// Not paired with anything.
    Unmatched,
}

impl Assertion {
    /// Whether two assertions about one endpoint can both hold.
    fn agrees_with(self, other: Self) -> bool {
        match (self, other) {
            (Assertion::Unmatched, Assertion::Unmatched) => true,
            // An unspecified partner agrees with any partner: an edit that
            // resolved only one of its ends says the endpoint is paired without
            // saying to what, which no pairing contradicts.
            (Assertion::Partner(left), Assertion::Partner(right)) => {
                left.is_none() || right.is_none() || left == right
            }
            _ => false,
        }
    }
}

impl Claim {
    /// Every endpoint this claim says something about.
    fn endpoints(&self) -> Vec<(Side, usize)> {
        match *self {
            Claim::Identity { old, new } => vec![(Side::Old, old), (Side::New, new)],
            Claim::Unmatched { side, index } => vec![(side, index)],
            Claim::Edit { old, new } => old
                .map(|index| (Side::Old, index))
                .into_iter()
                .chain(new.map(|index| (Side::New, index)))
                .collect(),
        }
    }

    /// What this claim says about one endpoint, if it says anything.
    fn assertion(&self, side: Side, index: usize) -> Option<Assertion> {
        let (old, new, unmatched) = match *self {
            Claim::Identity { old, new } => (Some(old), Some(new), false),
            Claim::Unmatched { side, index } => match side {
                Side::Old => (Some(index), None, true),
                Side::New => (None, Some(index), true),
            },
            Claim::Edit { old, new } => (old, new, false),
        };
        match side {
            Side::Old if old == Some(index) => Some(if unmatched {
                Assertion::Unmatched
            } else {
                Assertion::Partner(new)
            }),
            Side::New if new == Some(index) => Some(if unmatched {
                Assertion::Unmatched
            } else {
                Assertion::Partner(old)
            }),
            _ => None,
        }
    }

    /// Whether two claims cannot both hold.
    fn conflicts_with(&self, other: &Claim) -> bool {
        self.endpoints().into_iter().any(|(side, index)| {
            match (self.assertion(side, index), other.assertion(side, index)) {
                (Some(mine), Some(theirs)) => !mine.agrees_with(theirs),
                _ => false,
            }
        })
    }

    /// Whether the map already says something different about an endpoint.
    ///
    /// This is how a key component beats a hint without either knowing about the
    /// other: the key claimed its identities into the map before hints were
    /// read, so a hint that wants one of them differently is simply contradicted
    /// by what is already there.
    fn contradicts(&self, map: &ColumnMap) -> bool {
        self.endpoints().into_iter().any(|(side, index)| {
            let held = if map.reserved(side, index) {
                Some(Assertion::Unmatched)
            } else {
                match side {
                    Side::Old => map.new_for_old(index),
                    Side::New => map.old_for_new(index),
                }
                .map(|partner| Assertion::Partner(Some(partner)))
            };
            match (self.assertion(side, index), held) {
                (Some(mine), Some(held)) => !mine.agrees_with(held),
                _ => false,
            }
        })
    }

    /// Whether two claims touch a common endpoint, and so are reported together.
    fn shares_endpoint(&self, other: &Claim) -> bool {
        let theirs = other.endpoints();
        self.endpoints()
            .into_iter()
            .any(|endpoint| theirs.contains(&endpoint))
    }
}

/// Parse, validate, and apply every hint, extending what the key already claimed.
///
/// Schemas rather than tables: a hint is a claim about column identity, and
/// nothing here reads a value. `claimed` arrives holding the identities a
/// declared key asserts, which is how the two are ranked without either knowing
/// about the other — a hint that wants a column the key has spent is simply one
/// the map refuses.
pub(crate) fn resolve(
    old: &Schema,
    new: &Schema,
    spellings: &[String],
    claimed: ColumnMap,
) -> Result<Hints, DiffError> {
    let parsed = spellings
        .iter()
        .map(|spelling| parse(spelling))
        .collect::<Result<Vec<_>, DiffError>>()?;

    let mut result = Hints {
        map: claimed,
        edits: Vec::new(),
        issues: Vec::new(),
    };
    let mut claims: Vec<(usize, HintClaim, Claim)> = Vec::new();
    for (at, hint) in parsed.into_iter().enumerate() {
        match endpoints(old, new, &hint) {
            // Identity is judged after resolution, so a quoted and a bare
            // spelling of one claim collapse rather than contradicting.
            Ok(claim) if claims.iter().any(|(_, _, held)| *held == claim) => {}
            Ok(claim) => claims.push((at, hint, claim)),
            Err(issue) => result.issues.push(PendingIssue { at, issue }),
        }
    }

    let contested = contested(&claims, &result.map);
    for group in rival_groups(&claims, &contested) {
        result.issues.push(PendingIssue {
            at: claims[group[0]].0,
            issue: Issue {
                kind: IssueKind::ContradictoryHints,
                hints: group.iter().map(|&index| claims[index].1.clone()).collect(),
            },
        });
    }

    for (index, (at, hint, claim)) in claims.into_iter().enumerate() {
        if contested[index] {
            continue;
        }
        match claim {
            Claim::Identity { old, new } => {
                result.map.claim(old, new, IdentityBasis::Hinted);
            }
            Claim::Unmatched { side, index } => {
                result.map.reserve(side, index);
            }
            // An edit reserves nothing. Everything it does happens later: it
            // withdraws a swap before that inference runs, and it is judged
            // once the cells are known.
            Claim::Edit { old, new } => result.edits.push(EditHint {
                at,
                claim: hint,
                old,
                new,
            }),
        }
    }
    Ok(result)
}

/// Judge every edit hint against the identity it named, once there is one.
///
/// Returns the identities to force into the edit set. Both of the things this
/// can report need the finished comparison: whether the identity exists at all
/// is settled by inference, and whether anything about it changed by the cells.
pub(crate) fn validate_edits(
    edits: &[EditHint],
    map: &ColumnMap,
    cells: &CellChanges,
) -> (Vec<PendingIssue>, Vec<(usize, usize)>) {
    let mut issues = Vec::new();
    let mut forced = Vec::new();
    for edit in edits {
        let issue = |kind: IssueKind| PendingIssue {
            at: edit.at,
            issue: Issue {
                kind,
                hints: vec![edit.claim.clone()],
            },
        };
        // At most one identity can match: each endpoint the hint resolved
        // belongs to one identity at most, and an unresolved one constrains
        // nothing.
        let identity = map
            .pairs()
            .iter()
            .find(|pair| edit.attaches_to(pair.old, pair.new));
        let Some(identity) = identity else {
            issues.push(issue(IssueKind::HintUnresolvedIdentity));
            continue;
        };
        // `cells.columns` holds an entry only where something changed, so its
        // absence is the whole of the no-change test.
        if !cells
            .columns
            .iter()
            .any(|column| (column.old, column.new) == (identity.old, identity.new))
        {
            issues.push(issue(IssueKind::HintNoChange));
            continue;
        }
        forced.push((identity.old, identity.new));
    }
    (issues, forced)
}

/// Resolve a hint's names to the endpoints they claim.
///
/// A rename's two columns must exist and their values must be comparable. An
/// identity between a boolean and an integer would be accepted by everything up
/// to cell comparison and rejected there, taking the whole diff with it; a hint
/// the data cannot support is declined like any other, and the rest of the
/// comparison stands. Nothing else needs the check: a reserved endpoint is never
/// compared with anything, and an edit attaches to an identity that reconciled
/// on its own account.
fn endpoints(old: &Schema, new: &Schema, hint: &HintClaim) -> Result<Claim, Issue> {
    let issue = |kind: IssueKind| Issue {
        kind,
        hints: vec![hint.clone()],
    };
    let missing = |side: Side, column: &str| {
        issue(IssueKind::HintMissingTarget {
            side,
            column: column.to_owned(),
        })
    };

    match (hint.kind, &hint.names) {
        (HintKind::Rename, HintNames::Pair(old_name, new_name)) => {
            let old_index = position(old, old_name).ok_or_else(|| missing(Side::Old, old_name))?;
            let new_index = position(new, new_name).ok_or_else(|| missing(Side::New, new_name))?;
            let old_type = old.field(old_index).data_type();
            let new_type = new.field(new_index).data_type();
            if ComparisonPlan::new(old_type, new_type).is_none() {
                return Err(issue(IssueKind::HintIncompatibleTypes {
                    old_type: format!("{old_type:?}"),
                    new_type: format!("{new_type:?}"),
                }));
            }
            Ok(Claim::Identity {
                old: old_index,
                new: new_index,
            })
        }
        (HintKind::Drop, HintNames::Single(name)) => Ok(Claim::Unmatched {
            side: Side::Old,
            index: position(old, name).ok_or_else(|| missing(Side::Old, name))?,
        }),
        (HintKind::Add, HintNames::Single(name)) => Ok(Claim::Unmatched {
            side: Side::New,
            index: position(new, name).ok_or_else(|| missing(Side::New, name))?,
        }),
        (HintKind::Edit, HintNames::Pair(old_name, new_name)) => Ok(Claim::Edit {
            old: Some(position(old, old_name).ok_or_else(|| missing(Side::Old, old_name))?),
            new: Some(position(new, new_name).ok_or_else(|| missing(Side::New, new_name))?),
        }),
        (HintKind::Edit, HintNames::Single(name)) => {
            let claim = Claim::Edit {
                old: position(old, name),
                new: position(new, name),
            };
            // One end is enough, the other being whatever it pairs with. Absent
            // from both sides is reported against the new file, which is where a
            // reader took the name from: every operation about a surviving
            // column names it as the new file does.
            match claim {
                Claim::Edit {
                    old: None,
                    new: None,
                } => Err(missing(Side::New, name)),
                claim => Ok(claim),
            }
        }
        // Every remaining pairing of kind and shape is rejected while parsing,
        // where the spelling is still at hand to report.
        _ => unreachable!("parsing accepts only the shapes each kind takes"),
    }
}

fn position(schema: &Schema, name: &str) -> Option<usize> {
    schema
        .fields()
        .iter()
        .position(|field| field.name() == name)
}

/// Which claims cannot stand.
///
/// A claim has to go exactly when another claim says something different about
/// an endpoint of it, or when the map already does. That is one rule for all
/// four kinds, and it produces every conflict shape the design lists without any
/// of them being written down here.
///
/// Rejecting both rivals rather than picking one is what keeps input order out
/// of the answer: given `a -> b` and `a -> c`, keeping the first would make the
/// result depend on which flag came first. A claim that merely agrees with what
/// the map holds is redundant, not contested, and asking the map rather than
/// comparing names means the key needs no rule of its own — an endpoint goes to
/// whoever claimed it first, and the key claims before hints do.
fn contested(claims: &[(usize, HintClaim, Claim)], map: &ColumnMap) -> Vec<bool> {
    claims
        .iter()
        .enumerate()
        .map(|(index, (_, _, claim))| {
            claim.contradicts(map)
                || claims
                    .iter()
                    .enumerate()
                    .any(|(other, (_, _, rival))| other != index && claim.conflicts_with(rival))
        })
        .collect()
}

/// Gather the rejected claims into the sets that contest each other.
///
/// Reporting only: which claims go is already settled above, and no grouping
/// could change it — two claims land together by sharing an endpoint, and a
/// shared endpoint is what being contested is, so this can never reach a claim
/// the conflict rule missed. What it buys is one issue per set of rivals instead
/// of one per claim. Told that `a -> x`, `a -> y` and `b -> x` were dropped
/// together, a reader can see they conflict with each other; three separate
/// lines would leave them to work that out.
///
/// Sets rather than one list of everything rejected, because two unrelated
/// conflicts in one invocation are two separate things to know about.
fn rival_groups(claims: &[(usize, HintClaim, Claim)], contested: &[bool]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for index in (0..claims.len()).filter(|&index| contested[index]) {
        // Whatever this claim touches becomes one set with it, which is what
        // makes the result a partition however the claims were ordered.
        let mut joined = vec![index];
        let mut separate = Vec::new();
        for group in groups {
            let shared = group
                .iter()
                .any(|&other| claims[other].2.shares_endpoint(&claims[index].2));
            if shared {
                joined.extend(group);
            } else {
                separate.push(group);
            }
        }
        joined.sort_unstable();
        groups = separate;
        groups.push(joined);
    }
    groups.sort();
    groups
}

/// Read one line of the grammar as the claim it makes.
///
/// A hint's claim is its first argument. Anything after it is detail the format
/// prints about the operation — `basis: exact` on a rename, `changed: values`
/// and a type pair on an edit — and is ignored rather than read, because none of
/// it is the user's to assert: what a hint contributes is the identity, and
/// supplying `basis: exact` does not make the basis exact but makes it hinted.
///
/// Ignored, but not unchecked. Every argument after the claim must be a field,
/// so that `col_rename(a -> b, c -> d)`, whose second argument is shaped like a
/// second claim, is refused rather than half-honored. One rule and no
/// vocabulary: the grammar's colon is what marks detail, and the format writes
/// nothing else after a claim.
fn parse(spelling: &str) -> Result<HintClaim, DiffError> {
    let malformed = || DiffError::MalformedHint {
        hint: spelling.to_owned(),
    };
    let trimmed = spelling.trim();
    let open = trimmed.find('(').ok_or_else(malformed)?;
    if !trimmed.ends_with(')') {
        return Err(malformed());
    }
    let arguments = arguments(&trimmed[open + 1..trimmed.len() - 1]);
    let (claim, detail) = arguments.split_first().ok_or_else(malformed)?;
    if !detail.iter().all(|argument| is_field(argument)) {
        return Err(malformed());
    }
    let names = names(claim).ok_or_else(malformed)?;

    let kind = match trimmed[..open].trim() {
        "col_rename" => HintKind::Rename,
        "col_add" => HintKind::Add,
        "col_drop" => HintKind::Drop,
        "col_edit" => HintKind::Edit,
        kind => {
            return Err(DiffError::UnknownHintKind {
                hint: spelling.to_owned(),
                kind: kind.to_owned(),
            });
        }
    };
    // A rename needs two names and a reservation one; only an edit takes either,
    // naming an identity whose ends may or may not agree.
    let takes = matches!(
        (kind, &names),
        (HintKind::Rename, HintNames::Pair(..))
            | (HintKind::Add | HintKind::Drop, HintNames::Single(_))
            | (HintKind::Edit, _)
    );
    takes
        .then_some(HintClaim { kind, names })
        .ok_or_else(malformed)
}

/// Split an argument list on the grammar's commas.
///
/// Commas inside quotes and inside a list belong to the argument that contains
/// them, so `col_edit("a,b", changed: values)` is two arguments and not three.
fn arguments(arguments: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0_usize;
    scan(arguments, |index, character| {
        match character {
            b'[' | b'(' => depth += 1,
            b']' | b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push(&arguments[start..index]);
                start = index + 1;
            }
            _ => {}
        }
        false
    });
    parts.push(&arguments[start..]);
    parts
}

/// Whether an argument is a field, which is the only detail a line carries.
///
/// A field is detail whatever it says, because the grammar's colon is what
/// makes it one: `basis: exact` and `changed: values` cannot be mistaken for a
/// claim, and reading the field names here would only duplicate a list the
/// renderer owns.
///
/// That the format writes every detail as a field is what keeps this to one
/// rule. A bare word would have no such marker, and admitting bare words would
/// make `col_edit(price, cost)` — a user naming two columns — quietly mean
/// `col_edit(price)`.
fn is_field(argument: &str) -> bool {
    let trimmed = argument.trim();
    scan(trimmed, |index, character| {
        character == b':' && trimmed[index..].starts_with(": ")
    })
}

/// Walk the bytes outside quotes, stopping where `found` says so.
///
/// Every rule the parser has about punctuation is a rule about punctuation the
/// user did not quote, so one scanner serves them all: a name spelled with the
/// grammar's own characters in it is written as a JSON string and read straight
/// past here.
fn scan(text: &str, mut found: impl FnMut(usize, u8) -> bool) -> bool {
    let bytes = text.as_bytes();
    let mut quoted = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if quoted => index += 1,
            b'"' => quoted = !quoted,
            character if !quoted => {
                if found(index, character) {
                    return true;
                }
            }
            _ => {}
        }
        index += 1;
    }
    false
}

/// Read an argument list as the one or two names a hint is written with.
fn names(arguments: &str) -> Option<HintNames> {
    match split_pair(arguments) {
        Some((old, new)) => Some(HintNames::Pair(name(old)?, name(new)?)),
        None => Some(HintNames::Single(name(arguments)?)),
    }
}

/// Split an argument list on the grammar's `old -> new` arrow.
///
/// The arrow is found outside quotes, so a name containing one can be spelled
/// by quoting it.
fn split_pair(arguments: &str) -> Option<(&str, &str)> {
    let mut at = None;
    scan(arguments, |index, character| {
        let arrow = character == b'-' && arguments[index..].starts_with("->");
        if arrow {
            at = Some(index);
        }
        arrow
    });
    at.map(|index| (&arguments[..index], &arguments[index + 2..]))
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
    use arrow_schema::Schema;
    use test_support::table;

    use super::{Hints, parse, resolve};
    use crate::{DiffError, HintKind, HintNames, IdentityBasis, IssueKind, Side};

    fn hint_for(old: &RecordBatch, new: &RecordBatch, hints: &[&str]) -> Hints {
        try_hints(old, new, hints, &[]).unwrap()
    }

    /// Resolve hints against the identities a key spec would have claimed.
    fn try_hints(
        old: &RecordBatch,
        new: &RecordBatch,
        hints: &[&str],
        key: &[&str],
    ) -> Result<Hints, DiffError> {
        let spellings = hints
            .iter()
            .map(|hint| (*hint).to_owned())
            .collect::<Vec<_>>();
        let components = crate::key::declared_components(
            &key.iter()
                .map(|part| (*part).to_owned())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let claimed =
            crate::key::claimed_identities(schema(old), schema(new), components.components());
        resolve(schema(old), schema(new), &spellings, claimed)
    }

    fn schema(table: &RecordBatch) -> &Schema {
        table.schema_ref()
    }

    fn tables() -> (RecordBatch, RecordBatch) {
        (
            table! { "id" => [1], "gone" => [1], "other" => [1] },
            table! { "id" => [1], "fresh" => [1], "extra" => [1] },
        )
    }

    fn pairs(hints: &Hints) -> Vec<(usize, usize)> {
        hints
            .map
            .pairs()
            .iter()
            .map(|pair| (pair.old, pair.new))
            .collect()
    }

    /// The endpoints reserved as having no partner, by side.
    fn reserved(hints: &Hints, side: Side, count: usize) -> Vec<usize> {
        (0..count)
            .filter(|&index| hints.map.reserved(side, index))
            .collect()
    }

    /// The `(old, new)` endpoints each surviving edit hint resolved.
    fn edits(hints: &Hints) -> Vec<(Option<usize>, Option<usize>)> {
        hints
            .edits
            .iter()
            .map(|edit| (edit.old, edit.new))
            .collect()
    }

    #[test]
    fn every_kind_parses_to_the_names_it_was_written_with() {
        for (spelling, kind, names) in [
            (
                "col_rename(a -> b)",
                HintKind::Rename,
                HintNames::Pair("a".into(), "b".into()),
            ),
            ("col_add(b)", HintKind::Add, HintNames::Single("b".into())),
            ("col_drop(a)", HintKind::Drop, HintNames::Single("a".into())),
            ("col_edit(a)", HintKind::Edit, HintNames::Single("a".into())),
            (
                "col_edit(a -> b)",
                HintKind::Edit,
                HintNames::Pair("a".into(), "b".into()),
            ),
        ] {
            let claim = parse(spelling).unwrap();
            assert_eq!((claim.kind, claim.names), (kind, names), "{spelling}");
        }
    }

    #[test]
    fn the_detail_a_printed_line_carries_is_read_past() {
        // Every suffix the format writes, each on the kind that writes it. The
        // claim is the first argument, so all of these mean what the bare
        // spelling means: a line the tool printed is an instruction, and the
        // detail in it describes the operation rather than asserting anything.
        for (spelling, bare) in [
            ("col_rename(a -> b, basis: exact)", "col_rename(a -> b)"),
            ("col_rename(a -> b, basis: swapped)", "col_rename(a -> b)"),
            ("col_edit(a, changed: values)", "col_edit(a)"),
            ("col_edit(a, type: Int32 -> Int64)", "col_edit(a)"),
            (
                "col_edit(a, type: Int32 -> Int64, changed: values)",
                "col_edit(a)",
            ),
            ("col_edit(a -> b, changed: values)", "col_edit(a -> b)"),
        ] {
            let claim = parse(spelling).unwrap();
            let expected = parse(bare).unwrap();
            assert_eq!(
                (claim.kind, claim.names),
                (expected.kind, expected.names),
                "{spelling}"
            );
        }
    }

    #[test]
    fn an_argument_after_the_claim_that_is_not_a_field_is_refused() {
        // A second argument shaped like a second claim is much likelier to be a
        // user meaning something else than detail the format wrote, so it is
        // refused rather than half-honored. Every detail the format writes
        // carries the grammar's colon, so nothing has to be spelled out here:
        // `values` is refused with the rest, being a column name wherever it is
        // not a field's value.
        for spelling in [
            "col_rename(a -> b, c -> d)",
            "col_edit(a, b)",
            "col_edit(a, values)",
            "col_drop(a, b)",
        ] {
            assert!(parse(spelling).is_err(), "{spelling}");
        }
    }

    #[test]
    fn a_quoted_name_keeps_the_commas_in_it() {
        // Splitting the arguments must not split a name, or quoting would stop
        // reaching the names it exists to reach.
        let claim = parse(r#"col_edit("a, b", changed: values)"#).unwrap();

        assert_eq!(claim.names, HintNames::Single("a, b".into()));
    }

    #[test]
    fn an_unrecognized_kind_names_itself() {
        let (old, new) = tables();

        assert_eq!(
            try_hints(&old, &new, &["col_shuffle(gone)"], &[]).unwrap_err(),
            DiffError::UnknownHintKind {
                hint: "col_shuffle(gone)".into(),
                kind: "col_shuffle".into(),
            }
        );
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
        // make a duplicate contradict itself. That holds for every kind, since
        // identity is judged on what a claim resolved to.
        for spellings in [
            [
                "col_rename(gone -> fresh)",
                r#"col_rename("gone" -> "fresh")"#,
            ],
            ["col_drop(gone)", r#"col_drop("gone")"#],
            ["col_add(fresh)", r#"col_add("fresh")"#],
            ["col_edit(id)", r#"col_edit("id")"#],
        ] {
            let hints = hint_for(&old, &new, &spellings);

            assert!(hints.issues.is_empty(), "{spellings:?}");
        }
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
                .map
                .pairs()
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
            // A one-name kind given a pair is equally unreadable. Only an edit
            // takes either shape, naming an identity whose ends may differ.
            "col_drop(gone -> fresh)",
            "col_add(fresh -> extra)",
        ] {
            assert!(
                try_hints(&old, &new, &[spelling], &[]).is_err(),
                "{spelling}"
            );
        }
    }

    #[test]
    fn a_missing_target_is_reported_on_the_side_that_lacks_it() {
        let (old, new) = tables();

        let missing_new = hint_for(&old, &new, &["col_rename(gone -> absent)"]);
        let missing_old = hint_for(&old, &new, &["col_rename(absent -> fresh)"]);

        assert!(missing_new.map.pairs().is_empty());
        assert_eq!(
            missing_new.issues[0].issue.kind,
            IssueKind::HintMissingTarget {
                side: Side::New,
                column: "absent".into(),
            }
        );
        assert_eq!(missing_new.issues[0].issue.hints[0].kind, HintKind::Rename);
        assert_eq!(
            missing_old.issues[0].issue.kind,
            IssueKind::HintMissingTarget {
                side: Side::Old,
                column: "absent".into(),
            }
        );
    }

    #[test]
    fn a_reservation_names_its_column_on_its_own_side() {
        let (old, new) = tables();

        // A drop reads the old file and an add the new one, so each reports the
        // side it could not find its column on.
        assert_eq!(
            hint_for(&old, &new, &["col_drop(fresh)"]).issues[0]
                .issue
                .kind,
            IssueKind::HintMissingTarget {
                side: Side::Old,
                column: "fresh".into(),
            }
        );
        assert_eq!(
            hint_for(&old, &new, &["col_add(gone)"]).issues[0]
                .issue
                .kind,
            IssueKind::HintMissingTarget {
                side: Side::New,
                column: "gone".into(),
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

        assert!(hints.map.pairs().is_empty());
        assert_eq!(
            hints.issues[0].issue.kind,
            IssueKind::HintIncompatibleTypes {
                old_type: "Boolean".into(),
                new_type: "Int64".into(),
            }
        );
    }

    #[test]
    fn a_drop_and_an_add_reserve_their_endpoints() {
        let (old, new) = tables();

        let hints = hint_for(&old, &new, &["col_drop(gone)", "col_add(extra)"]);

        assert_eq!(reserved(&hints, Side::Old, 3), [1]);
        assert_eq!(reserved(&hints, Side::New, 3), [2]);
        assert!(hints.issues.is_empty());
    }

    #[test]
    fn a_drop_beside_an_add_chooses_replacement_rather_than_conflicting() {
        let (old, new) = tables();

        // The design reads these two together as a deliberate choice of
        // replacement over rename. They name endpoints in different halves of
        // the bijection and so can never meet, which is why one rule about
        // endpoints gets this right without a case for it.
        let hints = hint_for(&old, &new, &["col_drop(gone)", "col_add(fresh)"]);

        assert!(hints.issues.is_empty());
        assert_eq!(reserved(&hints, Side::Old, 3), [1]);
        assert_eq!(reserved(&hints, Side::New, 3), [1]);
    }

    #[test]
    fn an_edit_resolves_whichever_of_its_ends_exist() {
        let (old, new) = tables();

        // A name on both sides pins both endpoints; a name on one side leaves
        // the other to whatever reconciliation pairs it with, which is what
        // lets a col_edit() line about an inferred rename be handed back.
        assert_eq!(
            edits(&hint_for(&old, &new, &["col_edit(id)"])),
            [(Some(0), Some(0))]
        );
        assert_eq!(
            edits(&hint_for(&old, &new, &["col_edit(gone)"])),
            [(Some(1), None)]
        );
        assert_eq!(
            edits(&hint_for(&old, &new, &["col_edit(fresh)"])),
            [(None, Some(1))]
        );
        assert_eq!(
            edits(&hint_for(&old, &new, &["col_edit(gone -> fresh)"])),
            [(Some(1), Some(1))]
        );
    }

    #[test]
    fn an_edit_naming_nothing_at_all_is_reported_against_the_new_file() {
        let (old, new) = tables();

        // Every operation about a surviving column names it as the new file
        // does, so that is where a reader took the name from.
        let hints = hint_for(&old, &new, &["col_edit(absent)"]);

        assert!(hints.edits.is_empty());
        assert_eq!(
            hints.issues[0].issue.kind,
            IssueKind::HintMissingTarget {
                side: Side::New,
                column: "absent".into(),
            }
        );
    }

    #[test]
    fn an_edit_claims_no_endpoint_of_its_own() {
        let (old, new) = tables();

        // Two edits naming the two ends of one prospective identity do not
        // contest it, an edit asserting nothing exclusive about an endpoint.
        let hints = hint_for(&old, &new, &["col_edit(gone)", "col_edit(fresh)"]);

        assert!(hints.issues.is_empty());
        assert!(hints.map.pairs().is_empty());
        assert_eq!(edits(&hints), [(Some(1), None), (None, Some(1))]);
    }

    #[test]
    fn one_old_column_claimed_for_two_new_ones_rejects_both() {
        let (old, new) = tables();

        let hints = hint_for(
            &old,
            &new,
            &["col_rename(gone -> fresh)", "col_rename(gone -> extra)"],
        );

        assert!(hints.map.pairs().is_empty());
        assert_eq!(hints.issues.len(), 1);
        assert_eq!(hints.issues[0].issue.kind, IssueKind::ContradictoryHints);
        assert_eq!(hints.issues[0].issue.hints.len(), 2);
    }

    #[test]
    fn two_old_columns_claiming_one_new_one_reject_both() {
        let (old, new) = tables();

        let hints = hint_for(
            &old,
            &new,
            &["col_rename(gone -> fresh)", "col_rename(other -> fresh)"],
        );

        assert!(hints.map.pairs().is_empty());
        assert_eq!(hints.issues[0].issue.hints.len(), 2);
    }

    #[test]
    fn every_cross_kind_contradiction_rejects_its_group() {
        let (old, new) = tables();

        // One rule about what a claim says of an endpoint, and the design's
        // whole list of conflict shapes falls out of it.
        for spellings in [
            // An old endpoint both renamed and dropped.
            ["col_rename(gone -> fresh)", "col_drop(gone)"],
            // A new endpoint both renamed and added.
            ["col_rename(gone -> fresh)", "col_add(fresh)"],
            // An edit whose identity an add or a drop says cannot exist.
            ["col_edit(gone)", "col_drop(gone)"],
            ["col_edit(fresh)", "col_add(fresh)"],
            // An edit and a rename that disagree about a column's partner.
            ["col_edit(gone -> extra)", "col_rename(gone -> fresh)"],
        ] {
            let hints = hint_for(&old, &new, &spellings);

            assert_eq!(hints.issues.len(), 1, "{spellings:?}");
            assert_eq!(
                hints.issues[0].issue.kind,
                IssueKind::ContradictoryHints,
                "{spellings:?}"
            );
            assert_eq!(hints.issues[0].issue.hints.len(), 2, "{spellings:?}");
            assert!(hints.map.pairs().is_empty(), "{spellings:?}");
            assert!(hints.edits.is_empty(), "{spellings:?}");
            assert!(reserved(&hints, Side::Old, 3).is_empty(), "{spellings:?}");
            assert!(reserved(&hints, Side::New, 3).is_empty(), "{spellings:?}");
        }
    }

    #[test]
    fn an_edit_agreeing_with_a_rename_is_left_alone() {
        let (old, new) = tables();

        // The rename says old "gone" pairs with new "fresh" and the edit says
        // the same, so they are two statements about one identity rather than
        // rivals for it.
        let hints = hint_for(
            &old,
            &new,
            &["col_rename(gone -> fresh)", "col_edit(gone -> fresh)"],
        );

        assert!(hints.issues.is_empty());
        assert_eq!(pairs(&hints), [(1, 1)]);
        assert_eq!(edits(&hints), [(Some(1), Some(1))]);
    }

    #[test]
    fn a_chain_of_claims_is_rejected_whole() {
        let old = table! { "id" => [1], "a" => [1], "b" => [1] };
        let new = table! { "id" => [1], "x" => [1], "y" => [1] };

        // Each of these is contested on its own account — "a" is wanted twice
        // and so is "x" — so what the grouping decides is that they are reported
        // as one set of rivals rather than as three separate disappointments.
        let hints = hint_for(
            &old,
            &new,
            &[
                "col_rename(a -> x)",
                "col_rename(a -> y)",
                "col_rename(b -> x)",
            ],
        );

        assert!(hints.map.pairs().is_empty());
        assert_eq!(hints.issues.len(), 1);
        assert_eq!(hints.issues[0].issue.hints.len(), 3);
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
    fn every_kind_can_apply_in_one_invocation() {
        let old = table! { "id" => [1], "gone" => [1], "stale" => [1], "same" => [1] };
        let new = table! { "id" => [1], "fresh" => [1], "brand" => [1], "same" => [1] };

        let hints = hint_for(
            &old,
            &new,
            &[
                "col_rename(gone -> fresh)",
                "col_drop(stale)",
                "col_add(brand)",
                "col_edit(same)",
            ],
        );

        assert!(hints.issues.is_empty());
        assert_eq!(pairs(&hints), [(1, 1)]);
        assert_eq!(reserved(&hints, Side::Old, 4), [2]);
        assert_eq!(reserved(&hints, Side::New, 4), [2]);
        assert_eq!(edits(&hints), [(Some(3), Some(3))]);
    }

    #[test]
    fn a_key_component_beats_a_hint_that_contradicts_it() {
        let old = table! { "id" => [1], "gone" => [1] };
        let new = table! { "code" => [1], "fresh" => [1] };

        // The key pairs old "id" with new "code"; the hint wants old "id"
        // elsewhere. The key claimed first, so the map simply refuses the hint,
        // and there is no rule about keys anywhere in the contest.
        let hints = try_hints(&old, &new, &["col_rename(id -> fresh)"], &["id/code"]).unwrap();

        assert_eq!(pairs(&hints), [(0, 0)]);
        assert_eq!(hints.map.pairs()[0].basis, IdentityBasis::Declared);
        assert_eq!(hints.issues[0].issue.kind, IssueKind::ContradictoryHints);
    }

    #[test]
    fn a_key_component_beats_a_reservation_too() {
        let old = table! { "id" => [1], "gone" => [1] };
        let new = table! { "id" => [1], "fresh" => [1] };

        // Saying a key column has no partner contradicts the key that pairs it,
        // which the map settles the same way it settles a rename.
        let hints = try_hints(&old, &new, &["col_drop(id)"], &["id"]).unwrap();

        assert!(reserved(&hints, Side::Old, 2).is_empty());
        assert_eq!(hints.issues[0].issue.kind, IssueKind::ContradictoryHints);
    }

    #[test]
    fn a_hint_agreeing_with_a_key_component_is_left_alone() {
        let old = table! { "id" => [1], "gone" => [1] };
        let new = table! { "code" => [1], "fresh" => [1] };

        let hints = try_hints(
            &old,
            &new,
            &["col_rename(id -> code)", "col_rename(gone -> fresh)"],
            &["id/code"],
        )
        .unwrap();

        // Redundant rather than contradictory: both say the same thing.
        assert_eq!(pairs(&hints), [(0, 0), (1, 1)]);
        assert!(hints.issues.is_empty());
    }
}
