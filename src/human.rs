use std::io::{self, Write};

use crate::{
    ColumnSchema, Diff, HintClaim, HintNames, Issue, IssueKind, KeyBasis, KeyComponent,
    KeyRejection, KeyRetraction, KeySubject, POSITIONAL_COMPONENT,
};

/// The line dividing what went wrong from what was found.
const SEPARATOR: &str = "----";

/// Write a compact, operation-oriented description of a diff.
///
/// Anything that went wrong comes first — a rejected key, a declined hint —
/// then a `----` line, then everything the comparison learned. With nothing to
/// report there is no separator and the output opens on the key line, which is
/// informational context rather than a change operation, so `no_changes()`
/// still follows it when nothing changed.
pub fn write_human(mut writer: impl Write, diff: &Diff) -> io::Result<()> {
    let mut operations = Vec::new();

    // A rejected key is not an `Issue`, which is an instruction declined and
    // names the hints it concerns. It comes first because it explains the key
    // line below the separator, and being carried on the key rather than among
    // the issues is what spares the two from needing a common ordering.
    if let Some(rejection) = &diff.key.rejection {
        operations.push(key_rejection(rejection));
    }
    // A retracted guess follows the rejection it may sit beside, keeping the
    // problems in the order they arose: the declaration was refused first, the
    // guess withdrawn after. A key superseded by a better-informed one prints
    // nothing here — nothing went wrong, and the key line tells that story.
    if let Some(retraction) = &diff.key.retraction {
        operations.push(key_retraction(retraction));
    }
    for issue in &diff.issues {
        operations.push(issue_context(issue));
    }
    if !operations.is_empty() {
        operations.push(SEPARATOR.to_owned());
    }

    operations.push(key_context(diff));
    // Where the operations start, so that "nothing changed" stays a statement
    // about the data. A declined hint is a problem, not a change, and a diff of
    // two identical files still has to say so however many hints were dropped.
    let context = operations.len();

    // Renames come first: every operation below names its column as the new
    // file does, which needs explaining when the old file called it something
    // else.
    //
    // The basis says how the identity was reached, because some of the ways are
    // certainties and some are judgements, and the line reads the same either
    // way without it. A same-named identity produces no line at all, so `name`
    // is the one basis this can never write.
    for identity in &diff.columns.identities {
        let (old, new) = identity.column.positions();
        if raw_name(&diff.schemas.old, old) != raw_name(&diff.schemas.new, new) {
            operations.push(format!(
                "col_rename({} -> {}, basis: {})",
                column_name(&diff.schemas.old, old),
                column_name(&diff.schemas.new, new),
                identity.basis.name()
            ));
        }
    }
    for &position in &diff.columns.dropped {
        operations.push(format!(
            "col_drop({})",
            column_name(&diff.schemas.old, position)
        ));
    }
    for &position in &diff.columns.added {
        operations.push(format!(
            "col_add({})",
            column_name(&diff.schemas.new, position)
        ));
    }
    for coordinate in &diff.order.columns {
        let (old, new) = coordinate.positions();
        operations.push(format!(
            "col_order({}, {old} -> {new})",
            column_name(&diff.schemas.new, new)
        ));
    }
    // A regenerated table withholds the row story. Every line below this point
    // is conditional on a row matching the tool has just declared
    // untrustworthy — which rows exist, which cells changed, what moved — so
    // enumerating them would describe the matching rather than the data. What
    // stayed above is everything derived from schemas and identities. Value
    // counts go with the story, so a column edit that also changed type prints
    // its type alone. The model underneath holds everything regardless.
    let regenerated = diff.regeneration.is_some();

    // A count is every changed cell in the column, so a row edit crossing it
    // counts the cell they share too. The two numbers describe their own row and
    // their own column rather than dividing the change between them, which is
    // what makes each of them checkable against the data.
    //
    // A type-only edit has nothing to count and says nothing: `changes: 0` would
    // be a zero to interpret where an absence can be read past.
    for edit in &diff.summary.columns {
        if regenerated && !edit.type_changed {
            continue;
        }
        let (old, new) = edit.column.positions();
        let mut details = Vec::new();
        if edit.type_changed {
            details.push(format!(
                "type: {} -> {}",
                column_type(&diff.schemas.old, old),
                column_type(&diff.schemas.new, new)
            ));
        }
        if edit.changes > 0 && !regenerated {
            details.push(format!("changes: {}", edit.changes));
        }
        let suffix = if details.is_empty() {
            String::new()
        } else {
            format!(", {}", details.join(", "))
        };
        operations.push(format!(
            "col_edit({}{suffix})",
            column_name(&diff.schemas.new, new)
        ));
    }

    if regenerated {
        // No arguments: the subject is the table itself, and the measurement
        // lives in `Diff::regeneration` as a rejection's detail lives in its
        // variant.
        operations.push("table_regenerate()".to_owned());
    } else {
        for &position in &diff.rows.dropped {
            operations.push(format!("row_drop({position})"));
        }
        for &position in &diff.rows.added {
            operations.push(format!("row_add({position})"));
        }
        for event in &diff.rows.fanout {
            // The coordinates cannot say how far the new rows differ from the
            // old one, so the count does; the cells themselves are never
            // enumerated. What it counts is comparisons rather than cells of
            // one table, a one-to-many event having no single cell to point
            // at: two new rows disagreeing in the same column is two.
            let suffix = if event.cells.is_empty() {
                String::new()
            } else {
                format!(", changes: {}", event.cells.len())
            };
            let targets = event
                .new
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            operations.push(format!("row_fanout({} -> [{targets}]{suffix})", event.old));
        }
        for coordinate in &diff.order.rows {
            let (old, new) = coordinate.positions();
            operations.push(format!("row_order({old} -> {new})"));
        }
        for edit in &diff.summary.rows {
            let (old, new) = edit.row.positions();
            let row = if old == new {
                format!("{old}")
            } else {
                format!("{old} -> {new}")
            };
            operations.push(format!("row_edit({row}, changes: {})", edit.changes));
        }
    }

    if operations.len() == context {
        operations.push("no_changes()".to_owned());
    }
    writer.write_all(operations.join("\n").as_bytes())
}

/// Render the resolved key as a bracketed component list.
///
/// A guessed key is single-column today, but it is still bracketed so the
/// format does not change shape once compound guesses exist. A declared pair
/// renders as `"old" -> "new"` rather than as two names, which would make
/// `--key a/b` and `--key a,b` indistinguishable.
///
/// The positional key has no columns and names `#row` in their place, so the
/// list is never empty and `basis` keeps meaning throughout the format what it
/// means for every other key: how this one was arrived at.
fn key_context(diff: &Diff) -> String {
    let components = if diff.key.columns.is_empty() {
        POSITIONAL_COMPONENT.to_owned()
    } else {
        diff.key
            .columns
            .iter()
            .map(|coordinate| {
                let (old, new) = coordinate.positions();
                let old_name = column_name(&diff.schemas.old, old);
                let new_name = column_name(&diff.schemas.new, new);
                if old_name == new_name {
                    old_name
                } else {
                    format!("{old_name} -> {new_name}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    match diff.key.basis {
        KeyBasis::Declared => format!("col_key([{components}], basis: declared)"),
        KeyBasis::Fallback => format!("col_key([{components}], basis: fallback)"),
        KeyBasis::Guessed => {
            // Rounded to two digits for display; `KeyOverlap` keeps the exact
            // shared and possible counts for anything that needs them.
            let overlap = diff
                .key
                .overlap
                .map(|overlap| overlap.ratio())
                .unwrap_or(0.0);
            format!("col_key([{components}], basis: guessed, overlap: {overlap:.2})")
        }
    }
}

/// Render a declared key the data would not support.
///
/// The subject follows the reason. Resolving a component can fail on its own
/// account and names that component; uniqueness and fanout are properties of
/// the whole tuple and blame no one column, so they name the declared key
/// entire and are bracketed like the key line to say so at sight.
///
/// A component is named the way the key line names it, `customer_id -> id`
/// rather than the `customer_id/id` it was declared as, so that a rejection and
/// the `col_key()` line it explains describe one component one way.
fn key_rejection(rejection: &KeyRejection) -> String {
    let subject = match &rejection.subject {
        KeySubject::Component(component) => component_name(component),
        KeySubject::Key(components) => format!(
            "[{}]",
            components
                .iter()
                .map(component_name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    format!(
        "key_invalid({subject}, reason: {})",
        rejection.reason.name()
    )
}

/// Render a guessed key the tool withdrew after seeing the diff it produced.
///
/// Always bracketed: implausibility is a judgement about the key entire, like
/// uniqueness and fanout, however many columns the key had. The measurement
/// that condemned it stays in the model, as a rejection's detail does.
fn key_retraction(retraction: &KeyRetraction) -> String {
    format!(
        "key_retracted([{}], reason: excessive_change)",
        retraction
            .columns
            .iter()
            .map(component_name)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Name a key component the way the key line names it.
///
/// A pair reads `old -> new`, and an unpaired component collapses to its one
/// name, exactly as `key_context` writes them. Each name is quoted on its own
/// account, so the arrow is never inside a quoted string.
fn component_name(component: &KeyComponent) -> String {
    let old = value(&component.old);
    let new = value(&component.new);
    if old == new {
        old
    } else {
        format!("{old} -> {new}")
    }
}

/// Render one declined instruction and the reason it was declined.
///
/// The head is `hint_ignored()` rather than the issue's own kind, which is
/// carried as the reason field instead. That keeps the grammar's field names
/// fixed, and puts the thing a reader most needs — that an instruction was
/// dropped — at the front of the line. `Diff::issues` keeps the stable kinds
/// for anything matching on them.
///
/// The subject is whatever the reason applies to: one hint for a target that is
/// not there, and the whole group for a contradiction, which reports each group
/// once rather than repeating every hint's rivals beside it.
fn issue_context(issue: &Issue) -> String {
    let hints = issue.hints.iter().map(hint_claim).collect::<Vec<_>>();
    match &issue.kind {
        IssueKind::HintMissingTarget { column, .. } => {
            format!(
                "hint_ignored({}, missing: {})",
                hints.join(", "),
                value(column)
            )
        }
        IssueKind::ContradictoryHints => {
            format!(
                "hint_ignored([{}], reason: contradictory)",
                hints.join(", ")
            )
        }
        IssueKind::HintIncompatibleTypes { old_type, new_type } => format!(
            "hint_ignored({}, incompatible: {} -> {})",
            hints.join(", "),
            value(old_type),
            value(new_type)
        ),
        IssueKind::HintUnresolvedIdentity => {
            format!("hint_ignored({}, reason: unresolved)", hints.join(", "))
        }
        IssueKind::HintNoChange => {
            format!("hint_ignored({}, reason: unchanged)", hints.join(", "))
        }
    }
}

/// Render a hint the way the format prints the operation it asserts.
///
/// As written rather than as resolved: a hint reported back to its author should
/// be the line they typed, so a one-name claim keeps its one name.
fn hint_claim(claim: &HintClaim) -> String {
    let names = match &claim.names {
        HintNames::Single(name) => value(name),
        HintNames::Pair(old, new) => format!("{} -> {}", value(old), value(new)),
    };
    format!("{}({names})", claim.kind.name())
}

fn column_name(schema: &[ColumnSchema], one_based_position: usize) -> String {
    raw_name(schema, one_based_position)
        .map(value)
        .unwrap_or_else(|| format!("#{one_based_position}"))
}

fn raw_name(schema: &[ColumnSchema], one_based_position: usize) -> Option<&str> {
    schema
        .get(one_based_position.saturating_sub(1))
        .map(|column| column.name.as_str())
}

fn column_type(schema: &[ColumnSchema], one_based_position: usize) -> String {
    schema
        .get(one_based_position.saturating_sub(1))
        .map(|column| value(&column.source_type))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Write a string as the grammar's `word` where that reads back as itself.
///
/// Quoting everything is never wrong and always noisy: `col_key([id], basis:
/// declared)` says exactly what `col_key(["id"], basis: declared)` says, and
/// most column names are ordinary identifiers. So a name is left bare when it
/// is unmistakably one, and quoted otherwise.
///
/// The rule is deliberately narrower than what the hint parser accepts bare. It
/// has to be one a reader can apply in their head — and it is the rule they
/// need, because quotes are now a signal that a name has something in it worth
/// noticing rather than punctuation they must read past everywhere. "An
/// identifier" is such a rule; "anything without the grammar's punctuation in
/// it" is not. A leading digit is excluded because `value` also admits numbers,
/// and a column named `2024` should not arrive looking like one.
///
/// The cost of being narrow is a quoted name that did not have to be, which
/// costs two characters. The cost of being wide is a name the grammar cannot
/// read back.
fn value(text: &str) -> String {
    let identifier = !text.starts_with(|character: char| character.is_ascii_digit())
        && !text.is_empty()
        && text
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_');
    if identifier {
        text.to_owned()
    } else {
        quote(text)
    }
}

fn quote(text: &str) -> String {
    serde_json::to_string(text).expect("strings always serialize")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use arrow_array::RecordBatch;
    use test_support::table;

    use super::write_human;
    use crate::{DiffOptions, diff_tables};

    fn render_with(old: &RecordBatch, new: &RecordBatch, key: &[&str]) -> String {
        let diff = diff_tables(
            old,
            new,
            &DiffOptions {
                key: key
                    .iter()
                    .map(|component| (*component).to_owned())
                    .collect(),
                hints: Vec::new(),
            },
        )
        .unwrap();
        let mut output = Vec::new();
        write_human(&mut output, &diff).unwrap();
        String::from_utf8(output).unwrap()
    }

    fn render(old: &RecordBatch, new: &RecordBatch) -> String {
        render_with(old, new, &["id"])
    }

    fn render_hinted(old: &RecordBatch, new: &RecordBatch, hints: &[&str]) -> String {
        let diff = diff_tables(
            old,
            new,
            &DiffOptions {
                key: vec!["id".to_owned()],
                hints: hints.iter().map(|hint| (*hint).to_owned()).collect(),
            },
        )
        .unwrap();
        let mut output = Vec::new();
        write_human(&mut output, &diff).unwrap();
        String::from_utf8(output).unwrap()
    }

    /// The `name` of every `name: value` field in some rendered output.
    ///
    /// Fields are the one place the grammar could drift, so they are read back
    /// out of the rendering rather than trusted. Column names are not allowed
    /// to contain `": "` in the fixtures this is used on, which keeps the scan
    /// from mistaking a quoted name for a field.
    fn field_names(output: &str) -> BTreeSet<&str> {
        output
            .match_indices(": ")
            .filter_map(|(end, _)| {
                let start = output[..end]
                    .rfind(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                    .map_or(0, |position| position + 1);
                (start < end).then(|| &output[start..end])
            })
            .collect()
    }

    #[test]
    fn every_field_name_comes_from_the_fixed_set() {
        let old = table! {
            "id" => [1, 2, 4],
            "drop" => ["x", "y", "z"],
            "value" => i32[10, 20, 40],
        };
        let new = table! {
            "value" => [21, 11, 30],
            "id" => [2, 1, 3],
            "add" => ["a", "b", "c"],
        };
        let guessed = table! {
            "id" => [3, 1, 4],
            "value" => [31, 10, 40],
        };
        let key_only = table! { "id" => [1, 2] };
        let renamed_old = table! {
            "id" => [1, 2, 3],
            "amount" => [10, 20, 30],
        };
        let renamed_new = table! {
            "id" => [1, 2, 3],
            "total" => [10, 20, 30],
        };
        let fanned_old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "value" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        let fanned_new = table! {
            "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
            "value" => [0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0],
        };
        let hinted_new = table! {
            "id" => [1, 2, 3],
            "renamed" => [9, 8, 7],
        };
        let flagged_old = table! {
            "id" => [1, 2, 3],
            "flag" => [true, false, true],
        };
        let flagged_new = table! {
            "id" => [1, 2, 3],
            "count" => [1, 0, 1],
        };

        // Every line kind the renderer can write, so a field introduced
        // anywhere in the format has to show up in this set.
        let rendered = [
            render(&old, &new),
            render_with(&old, &guessed, &[]),
            render(&key_only, &key_only),
            render(&renamed_old, &renamed_new),
            render(&fanned_old, &fanned_new),
            // Issue lines are part of the format too, and carry fields of
            // their own, so a rendering that has them belongs here. Every reason
            // the channel can give is rendered, since a new one is exactly where
            // an ad-hoc field would arrive.
            render_hinted(
                &renamed_old,
                &hinted_new,
                &["col_rename(amount -> renamed)"],
            ),
            render_hinted(&renamed_old, &hinted_new, &["col_rename(amount -> absent)"]),
            render_hinted(
                &renamed_old,
                &hinted_new,
                &["col_rename(amount -> renamed)", "col_drop(amount)"],
            ),
            render_hinted(&renamed_old, &renamed_old, &["col_edit(amount)"]),
            render_hinted(&renamed_old, &hinted_new, &["col_edit(amount)"]),
            // A boolean and an integer cannot be compared, so this hint is
            // declined rather than obeyed — and its reason is the one field the
            // fixtures above never reached.
            render_hinted(&flagged_old, &flagged_new, &["col_rename(flag -> count)"]),
            // A retracted guess and a regenerated table are lines of the format
            // too, one with a reason field and one with no arguments at all.
            render_with(
                &table! {
                    "a" => [1, 2, 3, 4],
                    "x" => [1, 1, 2, 2],
                    "y" => [3, 3, 4, 4],
                },
                &table! {
                    "a" => [4, 3, 2, 1],
                    "x" => [1, 1, 2, 2],
                    "y" => [3, 3, 4, 4],
                },
                &[],
            ),
            render_with(
                &table! { "tag" => ["a", "a"], "value" => [1, 2] },
                &table! { "tag" => ["b", "b"], "value" => [3, 4] },
                &[],
            ),
        ]
        .join("\n");

        // The fixtures are only a guard if they reach the lines they claim to,
        // and an issue reason is easy to add a fixture for and never render.
        for reason in [
            "missing:",
            "reason: contradictory",
            "reason: unchanged",
            "reason: unresolved",
            "incompatible:",
            "reason: excessive_change",
            "table_regenerate()",
        ] {
            assert!(rendered.contains(reason), "{reason}");
        }

        assert_eq!(
            field_names(&rendered),
            BTreeSet::from([
                "basis",
                "changes",
                "incompatible",
                "missing",
                "overlap",
                "reason",
                "type"
            ])
        );
    }

    #[test]
    fn writes_mixed_changes_as_one_operation_per_line() {
        let old = table! {
            "id" => [1, 2, 4],
            "drop" => ["x", "y", "z"],
            "value" => i32[10, 20, 40],
        };
        let new = table! {
            "value" => [21, 11, 30],
            "id" => [2, 1, 3],
            "add" => ["a", "b", "c"],
        };

        insta::assert_snapshot!(render(&old, &new), @"
        col_key([id], basis: declared)
        col_drop(drop)
        col_add(add)
        col_order(value, 3 -> 1)
        col_edit(value, type: Int32 -> Int64, changes: 2)
        row_drop(3)
        row_add(3)
        row_order(2 -> 1)
        ");
    }

    #[test]
    fn announces_a_declared_compound_key() {
        let old = table! {
            "group" => ["a"],
            "id" => [1],
        };

        assert_eq!(
            render_with(&old, &old, &["group", "id"]),
            "col_key([group, id], basis: declared)\nno_changes()"
        );
    }

    #[test]
    fn announces_a_guessed_key_with_its_normalized_overlap() {
        let old = table! {
            "id" => [1, 2, 3],
            "value" => [10, 20, 30],
        };
        let new = table! {
            "id" => [3, 1, 4],
            "value" => [31, 10, 40],
        };

        insta::assert_snapshot!(render_with(&old, &new, &[]), @"
        col_key([id], basis: guessed, overlap: 0.67)
        row_drop(2)
        row_add(3)
        row_order(3 -> 1)
        row_edit(3 -> 1, changes: 1)
        ");
    }

    #[test]
    fn a_guessed_key_without_changes_still_reports_no_changes() {
        let old = table! { "line\n\"quoted\"" => [1, 2] };

        assert_eq!(
            render_with(&old, &old, &[]),
            "col_key([\"line\\n\\\"quoted\\\"\"], basis: guessed, overlap: 1.00)\nno_changes()"
        );
    }

    #[test]
    fn places_fanout_among_the_other_row_operations() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "value" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        let new = table! {
            "id" => [2, 1, 3, 4, 4, 5, 6, 7, 8, 9, 10, 99],
            "value" => [0, 0, 0, 0, 7, 0, 0, 5, 0, 0, 0, 0],
        };

        insta::assert_snapshot!(render(&old, &new), @"
        col_key([id], basis: declared)
        row_drop(11)
        row_add(12)
        row_fanout(4 -> [4, 5], changes: 1)
        row_order(2 -> 1)
        row_edit(7 -> 8, changes: 1)
        ");
    }

    #[test]
    fn a_fanout_without_differences_has_no_suffix() {
        let old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            "value" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };
        let new = table! {
            "id" => [1, 2, 3, 4, 4, 5, 6, 7, 8, 9, 10],
            "value" => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        };

        assert_eq!(
            render(&old, &new),
            "col_key([id], basis: declared)\nrow_fanout(4 -> [4, 5])"
        );
    }

    #[test]
    fn a_rename_says_what_established_the_identity() {
        // One rename per basis that can produce one. `name` is missing because
        // it cannot be here: an identity both files call the same thing is not
        // a rename, so the word exists for `Diff` rather than for the output.
        let declared_old = table! { "customer_id" => [1, 2], "value" => [10, 20] };
        let declared_new = table! { "id" => [1, 2], "value" => [10, 20] };
        let inferred_old = table! { "id" => [1, 2], "amount" => [10, 20] };
        let exact_new = table! { "id" => [1, 2], "total" => [10, 20] };
        let hinted_new = table! { "id" => [1, 2], "markdown" => [99, 98] };
        let approximate_old = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "amount" => [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110],
        };
        let approximate_new = table! {
            "id" => [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "total" => [10, 20, 30, 40, 50, 60, 71, 80, 90, 100, 110],
        };
        let swap_old = table! {
            "id" => [1, 2],
            "price" => [10, 20],
            "cost" => [30, 40],
        };
        let swap_new = table! {
            "id" => [1, 2],
            "price" => [30, 40],
            "cost" => [10, 20],
        };

        let rename = |rendered: String| {
            rendered
                .lines()
                .filter(|line| line.starts_with("col_rename("))
                .map(str::to_owned)
                .collect::<Vec<_>>()
                .join("\n")
        };

        assert_eq!(
            rename(render_with(
                &declared_old,
                &declared_new,
                &["customer_id/id"]
            )),
            "col_rename(customer_id -> id, basis: declared)"
        );
        assert_eq!(
            rename(render_hinted(
                &inferred_old,
                &hinted_new,
                &["col_rename(amount -> markdown)"]
            )),
            "col_rename(amount -> markdown, basis: hinted)"
        );
        assert_eq!(
            rename(render(&inferred_old, &exact_new)),
            "col_rename(amount -> total, basis: exact)"
        );
        assert_eq!(
            rename(render(&approximate_old, &approximate_new)),
            "col_rename(amount -> total, basis: approximate)"
        );
        // An exchange stays two lines, one per identity, and says on each that
        // it is half of one. Grouping them would mean detecting a cycle in the
        // bijection, which two rename hints can make just as well as a swap.
        assert_eq!(
            rename(render(&swap_old, &swap_new)),
            "col_rename(price -> cost, basis: swapped)\n\
             col_rename(cost -> price, basis: swapped)"
        );
    }

    #[test]
    fn names_a_renamed_column_as_the_new_file_does() {
        let old = table! {
            "customer_id" => [1, 2, 3],
            "gone" => [1, 2, 3],
            "value" => i32[10, 20, 30],
        };
        let new = table! {
            "value" => [11, 20, 30],
            "id" => [1, 2, 3],
            "fresh" => [4, 5, 6],
        };

        // The key pair is renamed, and "value" is edited and reordered. Every
        // operation about a surviving column names it as "new" does; only the
        // dropped column keeps its old name, having no other.
        insta::assert_snapshot!(render_with(&old, &new, &["customer_id/id"]), @"
        col_key([customer_id -> id], basis: declared)
        col_rename(customer_id -> id, basis: declared)
        col_drop(gone)
        col_add(fresh)
        col_order(value, 3 -> 1)
        col_edit(value, type: Int32 -> Int64, changes: 1)
        ");
    }

    #[test]
    fn a_paired_component_cannot_be_read_as_two_components() {
        let old = table! {
            "a" => [1, 2],
            "b" => [10, 20],
        };
        let new = table! {
            "a" => [30, 40],
            "b" => [1, 2],
        };

        // `--key a/b` identifies one column pair, while `--key a,b` would be a
        // compound key over two, so the two must not render alike.
        assert_eq!(
            render_with(&old, &new, &["a/b"]),
            "col_key([a -> b], basis: declared)\n\
             col_rename(a -> b, basis: declared)\n\
             col_drop(b)\ncol_add(a)"
        );
    }

    #[test]
    fn summarizes_multiple_cells_as_one_row_edit() {
        let old = table! {
            "id" => [1, 2],
            "a" => [10, 20],
            "b" => [30, 40],
        };
        let new = table! {
            "id" => [1, 2],
            "a" => [10, 21],
            "b" => [30, 41],
        };

        assert_eq!(
            render(&old, &new),
            "col_key([id], basis: declared)\nrow_edit(2, changes: 2)"
        );
    }

    #[test]
    fn an_edit_says_how_much_changed() {
        let old = table! {
            "id" => i32[1, 2, 3],
            "kept" => [10, 20, 30],
            "rewritten" => [1, 2, 3],
        };
        let new = table! {
            "id" => [1, 2, 3],
            "kept" => [10, 20, 30],
            "rewritten" => [9, 8, 3],
        };

        // The key was retyped and nothing about its values moved, so it has
        // nothing to count and says so by saying nothing. The other column
        // changed in two of its three rows, and every count the format writes is
        // positive like this one.
        insta::assert_snapshot!(render(&old, &new), @"
        col_key([id], basis: declared)
        col_edit(id, type: Int32 -> Int64)
        col_edit(rewritten, changes: 2)
        ");
    }

    #[test]
    fn crossing_edits_each_count_the_cell_they_share() {
        let old = table! {
            "id" => [1, 2, 3],
            "a" => [0, 0, 0],
            "b" => [0, 0, 0],
            "c" => [0, 0, 0],
        };
        let new = table! {
            "id" => [1, 2, 3],
            "a" => [1, 0, 0],
            "b" => [1, 0, 0],
            "c" => [1, 1, 1],
        };

        // Row 1 changed in three columns and "c" changed in three rows, over
        // five cells in total. Three plus three is deliberately not five: each
        // count describes its own row or column, and the cell where they cross
        // is a changed cell of both.
        insta::assert_snapshot!(render(&old, &new), @"
        col_key([id], basis: declared)
        col_edit(c, changes: 3)
        row_edit(1, changes: 3)
        ");
    }

    #[test]
    fn a_declined_hint_does_not_count_as_a_change() {
        let table = table! {
            "id" => [1, 2],
            "value" => [10, 20],
        };

        // A declined instruction is a problem, not a change. Two identical
        // files still have nothing to report, however many were dropped, and
        // "nothing changed" is a statement about what lies below the separator.
        assert_eq!(
            render_hinted(&table, &table, &["col_rename(value -> absent)"]),
            "hint_ignored(col_rename(value -> absent), missing: absent)\n\
             ----\n\
             col_key([id], basis: declared)\n\
             no_changes()"
        );
    }

    #[test]
    fn writes_an_explicit_operation_when_nothing_changed() {
        let table = table! {
            "id" => [1],
            "value" => [10],
        };

        assert_eq!(
            render(&table, &table),
            "col_key([id], basis: declared)\nno_changes()"
        );
    }

    #[test]
    fn quotes_unusual_column_names() {
        let old = table! {
            "id" => [1],
            "line\n\"quoted\"" => [10],
        };
        let new = table! { "id" => [1] };

        assert_eq!(
            render(&old, &new),
            "col_key([id], basis: declared)\ncol_drop(\"line\\n\\\"quoted\\\"\")"
        );
    }

    #[test]
    fn leaves_an_ordinary_name_bare_and_quotes_the_rest() {
        let old = table! {
            "id" => [1],
            "snake_case" => [1],
            "CamelCase" => [1],
            "_private" => [1],
            "x2" => [1],
            "2024" => [1],
            "total sales" => [1],
            "a,b" => [1],
            "a->b" => [1],
            "café" => [1],
        };
        let new = table! { "id" => [1] };

        // Quotes are a signal that a name holds something worth noticing, so
        // they are spent only where a bare name would not read back as itself:
        // a leading digit could be a number, and everything below it carries
        // the grammar's own punctuation, a space, or a character outside it.
        insta::assert_snapshot!(render(&old, &new), @r#"
        col_key([id], basis: declared)
        col_drop(snake_case)
        col_drop(CamelCase)
        col_drop(_private)
        col_drop(x2)
        col_drop("2024")
        col_drop("total sales")
        col_drop("a,b")
        col_drop("a->b")
        col_drop("café")
        "#);
    }

    #[test]
    fn the_positional_key_names_itself_rather_than_showing_an_empty_list() {
        let old = table! { "label" => ["x", "x"] };
        let new = table! { "label" => ["x", "y"] };

        // Nothing can identify a row, so the chain reaches its last resort.
        assert_eq!(
            render_with(&old, &new, &[]),
            "col_key([#row], basis: fallback)\nrow_edit(2, changes: 1)"
        );
    }

    #[test]
    fn declaring_the_positional_key_changes_only_the_basis() {
        let old = table! { "label" => ["x", "x"] };
        let new = table! { "label" => ["x", "y"] };

        let fallen_back = render_with(&old, &new, &[]);
        let declared = render_with(&old, &new, &["#row"]);

        assert_eq!(
            declared,
            "col_key([#row], basis: declared)\nrow_edit(2, changes: 1)"
        );
        // The two routes reach one key: only the line saying how differs.
        assert_eq!(
            declared.replace("basis: declared", "basis: fallback"),
            fallen_back
        );
    }

    #[test]
    fn a_component_rejection_names_the_component_and_a_key_rejection_brackets_it() {
        // A missing column is one component's own fault.
        let old = table! { "id" => [1, 2] };
        let new = table! { "other" => [1, 2] };
        assert!(
            render_with(&old, &new, &["id"])
                .starts_with("key_invalid(id, reason: missing_column)\n----\n"),
            "{}",
            render_with(&old, &new, &["id"])
        );

        // Uniqueness belongs to the tuple, so the whole key is named and
        // bracketed to say so at sight.
        let old = table! { "a" => [1, 1], "b" => [1, 1] };
        let new = table! { "a" => [1, 1], "b" => [1, 1] };
        assert!(
            render_with(&old, &new, &["a", "b"])
                .starts_with("key_invalid([a, b], reason: non_unique_old)\n----\n"),
            "{}",
            render_with(&old, &new, &["a", "b"])
        );
    }

    #[test]
    fn a_rejected_pair_is_named_the_way_the_key_line_names_it() {
        let old = table! { "customer_id" => [1, 1] };
        let new = table! { "id" => [1, 1] };

        // `customer_id -> id` rather than the `customer_id/id` it was declared
        // as, so the rejection and the key line it explains agree.
        assert!(
            render_with(&old, &new, &["customer_id/id"])
                .starts_with("key_invalid([customer_id -> id], reason: non_unique_old)\n"),
            "{}",
            render_with(&old, &new, &["customer_id/id"])
        );
    }

    #[test]
    fn a_clean_comparison_has_no_separator() {
        let table = table! { "id" => [1, 2] };

        assert_eq!(
            render_with(&table, &table, &["id"]),
            "col_key([id], basis: declared)\nno_changes()"
        );
    }

    #[test]
    fn a_retracted_guess_follows_the_rejection_it_sits_beside() {
        let old = table! {
            "a" => [1, 2, 3, 4],
            "x" => [1, 1, 2, 2],
            "y" => [3, 3, 4, 4],
        };
        let new = table! {
            "a" => [4, 3, 2, 1],
            "x" => [1, 1, 2, 2],
            "y" => [3, 3, 4, 4],
        };

        // The declared key is refused, the guess ("a", whose reversal makes
        // every other cell disagree) is withdrawn by its own diff, and the two
        // problems read in the order they arose. The fallback that remains
        // tells a plausible story — "a" itself changed — so the row story is
        // kept.
        insta::assert_snapshot!(render_with(&old, &new, &["absent"]), @"
        key_invalid(absent, reason: missing_column)
        key_retracted([a], reason: excessive_change)
        ----
        col_key([#row], basis: fallback)
        col_edit(a, changes: 4)
        ");
    }

    #[test]
    fn a_regenerated_table_withholds_the_row_story() {
        let old = table! {
            "tag" => ["a", "a"],
            "value" => i32[1, 2],
        };
        let new = table! {
            "tag" => ["b", "b"],
            "value" => [3, 4],
        };

        // Nothing can identify a row and every cell disagrees, so the row
        // story would describe the positional matching rather than the data.
        // The type change survives, being a fact about the schemas, but its
        // value count goes with the story it was part of.
        insta::assert_snapshot!(render_with(&old, &new, &[]), @"
        col_key([#row], basis: fallback)
        col_edit(value, type: Int32 -> Int64)
        table_regenerate()
        ");
    }
}
