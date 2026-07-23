use std::path::PathBuf;

use serde::Serialize;

/// Options that influence reconciliation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffOptions {
    /// Same-name columns that form the declared compound key.
    pub key: Vec<String>,
}

/// An error that prevents a complete diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffError {
    /// A Parquet input could not be opened or decoded.
    ReadParquet { path: PathBuf, message: String },
    /// Top-level column names are not unique.
    DuplicateColumnNames {
        side: Side,
        duplicates: Vec<DuplicateColumnName>,
    },
    /// A column is outside the MVP type set.
    UnsupportedColumn {
        side: Side,
        column: String,
        source_type: String,
    },
    /// An unsigned value cannot be represented as an `int64`.
    IntegerOutOfRange {
        side: Side,
        column: String,
        source_type: String,
        row: usize,
    },
    /// Reconciliation has not been implemented yet.
    NotImplemented,
}

impl std::fmt::Display for DiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiffError::ReadParquet { path, message } => {
                write!(f, "cannot read {}: {message}", path.display())
            }
            DiffError::DuplicateColumnNames { side, duplicates } => {
                write!(f, "{side} has duplicate column names: ")?;
                for (index, duplicate) in duplicates.iter().enumerate() {
                    if index > 0 {
                        f.write_str("; ")?;
                    }
                    write!(f, "{} at {:?}", duplicate.name, duplicate.positions)?;
                }
                Ok(())
            }
            DiffError::UnsupportedColumn {
                side,
                column,
                source_type,
            } => write!(
                f,
                "{side} column {column:?} has unsupported type {source_type}"
            ),
            DiffError::IntegerOutOfRange {
                side,
                column,
                source_type,
                row,
            } => write!(
                f,
                "{side} column {column:?} ({source_type}) exceeds int64 at row {row}"
            ),
            DiffError::NotImplemented => f.write_str("reconciliation is not implemented yet"),
        }
    }
}

impl std::error::Error for DiffError {}

/// Which input produced a validation issue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Old,
    New,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Side::Old => f.write_str("old"),
            Side::New => f.write_str("new"),
        }
    }
}

/// Every occurrence of one duplicated top-level name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateColumnName {
    pub name: String,
    pub positions: Vec<usize>,
}

/// A one-based old/new position, collapsed when the positions agree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coordinate(CoordinateRepr);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
enum CoordinateRepr {
    Same(usize),
    Moved([usize; 2]),
}

impl Coordinate {
    /// Construct a coordinate from zero-based positions in the input tables.
    pub fn from_zero_based(old: usize, new: usize) -> Self {
        let old = old + 1;
        let new = new + 1;
        if old == new {
            Self(CoordinateRepr::Same(old))
        } else {
            Self(CoordinateRepr::Moved([old, new]))
        }
    }
}

impl Serialize for Coordinate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

/// A one-based old/new cell coordinate, collapsed when both positions agree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellCoordinate(CellCoordinateRepr);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
enum CellCoordinateRepr {
    Same([usize; 2]),
    Moved([[usize; 2]; 2]),
}

impl CellCoordinate {
    /// Construct a cell coordinate from zero-based row and column positions.
    pub fn from_zero_based(
        old_row: usize,
        old_column: usize,
        new_row: usize,
        new_column: usize,
    ) -> Self {
        let old = [old_row + 1, old_column + 1];
        let new = [new_row + 1, new_column + 1];
        if old == new {
            Self(CellCoordinateRepr::Same(old))
        } else {
            Self(CellCoordinateRepr::Moved([old, new]))
        }
    }
}

impl Serialize for CellCoordinate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

/// A type in the MVP comparison domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizedType {
    Boolean,
    Int64,
    Double,
    String,
}

/// One column in an input schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ColumnSchema {
    pub name: String,
    pub source_type: String,
    pub normalized_type: NormalizedType,
}

/// The original and normalized input schemas.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Schemas {
    pub old: Vec<ColumnSchema>,
    pub new: Vec<ColumnSchema>,
}

/// Evidence that an identified column changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ColumnEdit {
    pub column: Coordinate,
    pub type_changed: bool,
    pub values_changed: bool,
}

/// Resolved column identities and schema events.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ColumnsDiff {
    pub identities: Vec<Coordinate>,
    pub added: Vec<usize>,
    pub dropped: Vec<usize>,
    pub edited: Vec<ColumnEdit>,
}

/// How the row key was selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyBasis {
    Declared,
}

/// The resolved row key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct KeyDiff {
    pub basis: KeyBasis,
    pub columns: Vec<Coordinate>,
}

/// Row matching events.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RowsDiff {
    pub added: Vec<usize>,
    pub dropped: Vec<usize>,
    pub matched: Vec<Coordinate>,
}

/// Minimal relative-order changes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct OrderDiff {
    pub columns: Vec<Coordinate>,
    pub rows: Vec<Coordinate>,
}

/// An inspectable, coordinate-only table diff.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diff {
    pub schemas: Schemas,
    pub columns: ColumnsDiff,
    pub key: KeyDiff,
    pub rows: RowsDiff,
    pub order: OrderDiff,
    pub cells: Vec<CellCoordinate>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CellCoordinate, ColumnEdit, ColumnSchema, ColumnsDiff, Coordinate, Diff, KeyBasis, KeyDiff,
        NormalizedType, OrderDiff, RowsDiff, Schemas,
    };

    #[test]
    fn coordinate_collapses_equal_positions() {
        let coordinate = Coordinate::from_zero_based(1, 1);
        assert_eq!(serde_json::to_value(coordinate).unwrap(), json!(2));
    }

    #[test]
    fn coordinate_retains_moved_positions() {
        let coordinate = Coordinate::from_zero_based(2, 0);
        assert_eq!(serde_json::to_value(coordinate).unwrap(), json!([3, 1]));
    }

    #[test]
    fn cell_collapses_when_both_positions_agree() {
        let cell = CellCoordinate::from_zero_based(1, 2, 1, 2);
        assert_eq!(serde_json::to_value(cell).unwrap(), json!([2, 3]));
    }

    #[test]
    fn cell_retains_both_positions_when_either_moves() {
        let cell = CellCoordinate::from_zero_based(0, 2, 3, 1);
        assert_eq!(serde_json::to_value(cell).unwrap(), json!([[1, 3], [4, 2]]));
    }

    #[test]
    fn diff_serializes_in_stable_field_order() {
        let diff = Diff {
            schemas: Schemas {
                old: vec![ColumnSchema {
                    name: "id".into(),
                    source_type: "INT64".into(),
                    normalized_type: NormalizedType::Int64,
                }],
                new: vec![ColumnSchema {
                    name: "id".into(),
                    source_type: "INT64".into(),
                    normalized_type: NormalizedType::Int64,
                }],
            },
            columns: ColumnsDiff {
                identities: vec![Coordinate::from_zero_based(0, 0)],
                edited: vec![ColumnEdit {
                    column: Coordinate::from_zero_based(0, 0),
                    type_changed: false,
                    values_changed: true,
                }],
                ..ColumnsDiff::default()
            },
            key: KeyDiff {
                basis: KeyBasis::Declared,
                columns: vec![Coordinate::from_zero_based(0, 0)],
            },
            rows: RowsDiff {
                matched: vec![Coordinate::from_zero_based(0, 1)],
                ..RowsDiff::default()
            },
            order: OrderDiff::default(),
            cells: vec![CellCoordinate::from_zero_based(0, 0, 1, 0)],
        };

        insta::assert_json_snapshot!(diff, @r#"
        {
          "schemas": {
            "old": [
              {
                "name": "id",
                "source_type": "INT64",
                "normalized_type": "int64"
              }
            ],
            "new": [
              {
                "name": "id",
                "source_type": "INT64",
                "normalized_type": "int64"
              }
            ]
          },
          "columns": {
            "identities": [
              1
            ],
            "added": [],
            "dropped": [],
            "edited": [
              {
                "column": 1,
                "type_changed": false,
                "values_changed": true
              }
            ]
          },
          "key": {
            "basis": "declared",
            "columns": [
              1
            ]
          },
          "rows": {
            "added": [],
            "dropped": [],
            "matched": [
              [
                1,
                2
              ]
            ]
          },
          "order": {
            "columns": [],
            "rows": []
          },
          "cells": [
            [
              [
                1,
                1
              ],
              [
                2,
                1
              ]
            ]
          ]
        }
        "#);
    }
}
