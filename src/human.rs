use std::io::{self, Write};

use crate::{ColumnSchema, Diff};

/// Write a compact, operation-oriented description of a diff.
pub fn write_human(mut writer: impl Write, diff: &Diff) -> io::Result<()> {
    let mut operations = Vec::new();

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

    if operations.is_empty() {
        writer.write_all(b"no_changes()")
    } else {
        writer.write_all(operations.join("\n").as_bytes())
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

    fn render(old: &RecordBatch, new: &RecordBatch) -> String {
        let diff = diff_tables(
            old,
            new,
            &DiffOptions {
                key: vec!["id".into()],
            },
        )
        .unwrap();
        let mut output = Vec::new();
        write_human(&mut output, &diff).unwrap();
        String::from_utf8(output).unwrap()
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

        assert_eq!(render(&old, &new), "row_edit(2)");
    }

    #[test]
    fn writes_an_explicit_operation_when_nothing_changed() {
        let table = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1]))),
            ("value", Arc::new(Int64Array::from(vec![10]))),
        ]);

        assert_eq!(render(&table, &table), "no_changes()");
    }

    #[test]
    fn quotes_unusual_column_names() {
        let old = table(vec![
            ("id", Arc::new(Int64Array::from(vec![1]))),
            ("line\n\"quoted\"", Arc::new(Int64Array::from(vec![10]))),
        ]);
        let new = table(vec![("id", Arc::new(Int64Array::from(vec![1])))]);

        assert_eq!(render(&old, &new), r#"col_drop("line\n\"quoted\"")"#);
    }
}
