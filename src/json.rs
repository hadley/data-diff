use std::io::{self, Write};

use serde::Serialize;
use serde_json::ser::{Formatter, Serializer};

use crate::Diff;

/// Write readable JSON with arrays kept on one line.
pub fn write_json(writer: impl Write, diff: &Diff) -> serde_json::Result<()> {
    let mut serializer = Serializer::with_formatter(writer, CompactArrayFormatter::new());
    diff.serialize(&mut serializer)
}

struct CompactArrayFormatter {
    current_indent: usize,
    object_has_value: Vec<bool>,
}

impl CompactArrayFormatter {
    fn new() -> Self {
        Self {
            current_indent: 0,
            object_has_value: Vec::new(),
        }
    }

    fn indent(writer: &mut (impl Write + ?Sized), depth: usize) -> io::Result<()> {
        for _ in 0..depth {
            writer.write_all(b"  ")?;
        }
        Ok(())
    }
}

impl Formatter for CompactArrayFormatter {
    fn begin_array<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        writer.write_all(b"[")
    }

    fn end_array<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        writer.write_all(b"]")
    }

    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        self.current_indent += 1;
        self.object_has_value.push(false);
        writer.write_all(b"{")
    }

    fn end_object<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        self.current_indent -= 1;
        let has_value = self.object_has_value.pop().unwrap();
        if has_value {
            writer.write_all(b"\n")?;
            Self::indent(writer, self.current_indent)?;
        }
        writer.write_all(b"}")
    }

    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        writer.write_all(if first { b"\n" } else { b",\n" })?;
        Self::indent(writer, self.current_indent)
    }

    fn begin_object_value<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        writer.write_all(b": ")
    }

    fn end_object_value<W>(&mut self, _writer: &mut W) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        *self.object_has_value.last_mut().unwrap() = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        CellCoordinate, ColumnsDiff, Diff, KeyBasis, KeyDiff, OrderDiff, RowsDiff, Schemas,
    };

    use super::write_json;

    #[test]
    fn keeps_nested_coordinate_arrays_on_one_line() {
        let diff = Diff {
            schemas: Schemas::default(),
            columns: ColumnsDiff::default(),
            key: KeyDiff {
                basis: KeyBasis::Declared,
                columns: vec![],
            },
            rows: RowsDiff::default(),
            order: OrderDiff::default(),
            cells: vec![
                CellCoordinate::from_zero_based(0, 2, 1, 0),
                CellCoordinate::from_zero_based(2, 2, 0, 0),
            ],
        };
        let mut output = Vec::new();

        write_json(&mut output, &diff).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains(r#""cells": [[[1, 3], [2, 1]], [[3, 3], [1, 1]]]"#));
        assert!(output.starts_with("{\n  \"schemas\": {"));
    }
}
