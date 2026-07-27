use std::io::{self, Write};

use crate::{ColumnSchema, Diff, KeyBasis};

/// Write a compact, operation-oriented description of a diff.
///
/// The first line always announces the resolved key; it is informational
/// context rather than a change operation, so `no_changes()` still follows it
/// when nothing changed.
pub fn write_human(mut writer: impl Write, diff: &Diff) -> io::Result<()> {
    let mut operations = Vec::new();

    operations.push(key_context(diff));

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
            column_name(&diff.schemas.old, old)
        ));
    }
    for edit in &diff.summary.columns {
        let (old, new) = edit.column.positions();
        let mut details = Vec::new();
        if edit.type_changed {
            details.push(format!(
                "type {} -> {}",
                column_type(&diff.schemas.old, old),
                column_type(&diff.schemas.new, new)
            ));
        }
        if edit.values_changed {
            details.push("values".to_owned());
        }
        let suffix = if details.is_empty() {
            String::new()
        } else {
            format!(", {}", details.join(", "))
        };
        operations.push(format!(
            "col_edit({}{suffix})",
            column_name(&diff.schemas.old, old)
        ));
    }

    for &position in &diff.rows.dropped {
        operations.push(format!("row_drop({position})"));
    }
    for &position in &diff.rows.added {
        operations.push(format!("row_add({position})"));
    }
    for event in &diff.rows.fanout {
        // The coordinates cannot say whether the new rows differ from the old
        // one, so the suffix does; the cells themselves are never enumerated.
        let suffix = if event.cells.is_empty() {
            ""
        } else {
            ", values"
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
    for coordinate in &diff.summary.rows {
        let (old, new) = coordinate.positions();
        if old == new {
            operations.push(format!("row_edit({old})"));
        } else {
            operations.push(format!("row_edit({old} -> {new})"));
        }
    }

    if operations.len() == 1 {
        operations.push("no_changes()".to_owned());
    }
    writer.write_all(operations.join("\n").as_bytes())
}

/// Render the resolved key as a bracketed component list.
///
/// A guessed key is single-column today, but it is still bracketed so the
/// format does not change shape once compound guesses exist.
fn key_context(diff: &Diff) -> String {
    let components = diff
        .key
        .columns
        .iter()
        .map(|coordinate| column_name(&diff.schemas.old, coordinate.positions().0))
        .collect::<Vec<_>>()
        .join(", ");
    match diff.key.basis {
        KeyBasis::Declared => format!("col_key(declared: [{components}])"),
        KeyBasis::Guessed => {
            // Rounded to two digits for display; `KeyOverlap` keeps the exact
            // shared and possible counts for anything that needs them.
            let overlap = diff
                .key
                .overlap
                .map(|overlap| overlap.ratio())
                .unwrap_or(0.0);
            format!("col_key(guessed: [{components}], overlap: {overlap:.2})")
        }
    }
}

fn column_name(schema: &[ColumnSchema], one_based_position: usize) -> String {
    schema
        .get(one_based_position.saturating_sub(1))
        .map(|column| quote(&column.name))
        .unwrap_or_else(|| format!("#{one_based_position}"))
}

fn column_type(schema: &[ColumnSchema], one_based_position: usize) -> String {
    schema
        .get(one_based_position.saturating_sub(1))
        .map(|column| quote(&column.source_type))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn quote(value: &str) -> String {
    serde_json::to_string(value).expect("strings always serialize")
}

#[cfg(test)]
mod tests {
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

        insta::assert_snapshot!(render(&old, &new), @r#"
        col_key(declared: ["id"])
        col_drop("drop")
        col_add("add")
        col_order("value", 3 -> 1)
        col_edit("value", type "Int32" -> "Int64", values)
        row_drop(3)
        row_add(3)
        row_order(2 -> 1)
        "#);
    }

    #[test]
    fn announces_a_declared_compound_key() {
        let old = table! {
            "group" => ["a"],
            "id" => [1],
        };

        assert_eq!(
            render_with(&old, &old, &["group", "id"]),
            "col_key(declared: [\"group\", \"id\"])\nno_changes()"
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

        insta::assert_snapshot!(render_with(&old, &new, &[]), @r#"
        col_key(guessed: ["id"], overlap: 0.67)
        row_drop(2)
        row_add(3)
        row_order(3 -> 1)
        row_edit(3 -> 1)
        "#);
    }

    #[test]
    fn a_guessed_key_without_changes_still_reports_no_changes() {
        let old = table! { "line\n\"quoted\"" => [1, 2] };

        assert_eq!(
            render_with(&old, &old, &[]),
            "col_key(guessed: [\"line\\n\\\"quoted\\\"\"], overlap: 1.00)\nno_changes()"
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

        insta::assert_snapshot!(render(&old, &new), @r#"
        col_key(declared: ["id"])
        row_drop(11)
        row_add(12)
        row_fanout(4 -> [4, 5], values)
        row_order(2 -> 1)
        row_edit(7 -> 8)
        "#);
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
            "col_key(declared: [\"id\"])\nrow_fanout(4 -> [4, 5])"
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
            "col_key(declared: [\"id\"])\nrow_edit(2)"
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
            "col_key(declared: [\"id\"])\nno_changes()"
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
            "col_key(declared: [\"id\"])\ncol_drop(\"line\\n\\\"quoted\\\"\")"
        );
    }
}
