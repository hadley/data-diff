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

fn key_context(diff: &Diff) -> String {
    let component_name =
        |coordinate: &crate::Coordinate| column_name(&diff.schemas.old, coordinate.positions().0);
    match diff.key.basis {
        KeyBasis::Declared => {
            let components = diff
                .key
                .columns
                .iter()
                .map(component_name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("col_key(declared: [{components}])")
        }
        KeyBasis::Guessed => {
            let name = diff
                .key
                .columns
                .first()
                .map(component_name)
                .unwrap_or_else(|| "#0".to_owned());
            let overlap = diff
                .key
                .overlap
                .map(|overlap| serde_json::to_string(&overlap).expect("numbers always serialize"))
                .unwrap_or_else(|| "0".to_owned());
            format!("col_key(guessed: {name}, overlap: {overlap})")
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
    use std::sync::Arc;

    use arrow_array::{ArrayRef, Int32Array, Int64Array, RecordBatch, StringArray};
    use arrow_schema::{Field, Schema};

    use super::write_human;
    use crate::{DiffOptions, diff_tables};

    fn table(columns: Vec<(&str, ArrayRef)>) -> RecordBatch {
        let fields = columns
            .iter()
            .map(|(name, values)| Field::new(*name, values.data_type().clone(), true))
            .collect::<Vec<_>>();
        RecordBatch::try_new(
            Arc::new(Schema::new(fields)),
            columns.into_iter().map(|(_, values)| values).collect(),
        )
        .unwrap()
    }

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
        let old = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2, 4]))),
            ("drop", Arc::new(StringArray::from(vec!["x", "y", "z"]))),
            ("value", Arc::new(Int32Array::from(vec![10, 20, 40]))),
        ]);
        let new = table(vec![
            ("value", Arc::new(Int64Array::from(vec![21, 11, 30]))),
            ("id", Arc::new(Int64Array::from(vec![2, 1, 3]))),
            ("add", Arc::new(StringArray::from(vec!["a", "b", "c"]))),
        ]);

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
        let old = table(vec![
            ("group", Arc::new(StringArray::from(vec!["a"]))),
            ("id", Arc::new(Int64Array::from(vec![1]))),
        ]);

        assert_eq!(
            render_with(&old, &old, &["group", "id"]),
            "col_key(declared: [\"group\", \"id\"])\nno_changes()"
        );
    }

    #[test]
    fn announces_a_guessed_key_with_its_normalized_overlap() {
        let old = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2, 3]))),
            ("value", Arc::new(Int64Array::from(vec![10, 20, 30]))),
        ]);
        let new = table(vec![
            ("id", Arc::new(Int64Array::from(vec![3, 1, 4]))),
            ("value", Arc::new(Int64Array::from(vec![31, 10, 40]))),
        ]);

        insta::assert_snapshot!(render_with(&old, &new, &[]), @r#"
        col_key(guessed: "id", overlap: 0.6666666666666666)
        row_drop(2)
        row_add(3)
        row_order(3 -> 1)
        row_edit(3 -> 1)
        "#);
    }

    #[test]
    fn a_guessed_key_without_changes_still_reports_no_changes() {
        let old = table(vec![(
            "line\n\"quoted\"",
            Arc::new(Int64Array::from(vec![1, 2])),
        )]);

        assert_eq!(
            render_with(&old, &old, &[]),
            "col_key(guessed: \"line\\n\\\"quoted\\\"\", overlap: 1.0)\nno_changes()"
        );
    }

    #[test]
    fn summarizes_multiple_cells_as_one_row_edit() {
        let old = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2]))),
            ("a", Arc::new(Int64Array::from(vec![10, 20]))),
            ("b", Arc::new(Int64Array::from(vec![30, 40]))),
        ]);
        let new = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2]))),
            ("a", Arc::new(Int64Array::from(vec![10, 21]))),
            ("b", Arc::new(Int64Array::from(vec![30, 41]))),
        ]);

        assert_eq!(
            render(&old, &new),
            "col_key(declared: [\"id\"])\nrow_edit(2)"
        );
    }

    #[test]
    fn writes_an_explicit_operation_when_nothing_changed() {
        let table = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1]))),
            ("value", Arc::new(Int64Array::from(vec![10]))),
        ]);

        assert_eq!(
            render(&table, &table),
            "col_key(declared: [\"id\"])\nno_changes()"
        );
    }

    #[test]
    fn quotes_unusual_column_names() {
        let old = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1]))),
            ("line\n\"quoted\"", Arc::new(Int64Array::from(vec![10]))),
        ]);
        let new = table(vec![("id", Arc::new(Int64Array::from(vec![1])))]);

        assert_eq!(
            render(&old, &new),
            "col_key(declared: [\"id\"])\ncol_drop(\"line\\n\\\"quoted\\\"\")"
        );
    }
}
