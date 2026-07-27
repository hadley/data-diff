use std::path::PathBuf;

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
    /// No key was supplied and no eligible key could be guessed.
    MissingKey,
    /// A comma-separated key contained an empty component.
    EmptyKeyComponent,
    /// Paired old/new key names are deferred until hint support.
    PairedKeyUnsupported { component: String },
    /// A compound key repeated a component.
    DuplicateKeyComponent { component: String },
    /// A key component was absent on one side.
    MissingKeyColumn { side: Side, component: String },
    /// A corresponding key-column pair cannot be compared.
    IncompatibleKeyTypes {
        component: String,
        old_type: String,
        new_type: String,
    },
    /// A key contains null or `NaN`.
    InvalidKeyValue {
        side: Side,
        component: String,
        row: usize,
    },
    /// The declared key is not unique in the old input.
    NonUniqueOldKey { first_row: usize, row: usize },
    /// New-side duplication requires fanout, which the MVP defers.
    UnsupportedFanout { first_row: usize, row: usize },
    /// Same-name non-key columns cannot be compared.
    IncompatibleColumns {
        column: String,
        old_type: String,
        new_type: String,
    },
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
            DiffError::MissingKey => f.write_str(
                "no key was supplied and no eligible key could be guessed; supply --key",
            ),
            DiffError::EmptyKeyComponent => f.write_str("the key contains an empty component"),
            DiffError::PairedKeyUnsupported { component } => {
                write!(f, "paired key component {component:?} is not supported yet")
            }
            DiffError::DuplicateKeyComponent { component } => {
                write!(f, "key component {component:?} is repeated")
            }
            DiffError::MissingKeyColumn { side, component } => {
                write!(f, "{side} is missing key column {component:?}")
            }
            DiffError::IncompatibleKeyTypes {
                component,
                old_type,
                new_type,
            } => write!(
                f,
                "key column {component:?} has incompatible types {old_type} and {new_type}"
            ),
            DiffError::InvalidKeyValue {
                side,
                component,
                row,
            } => write!(
                f,
                "{side} key column {component:?} has null or NaN at row {row}"
            ),
            DiffError::NonUniqueOldKey { first_row, row } => write!(
                f,
                "old key is non-unique at rows {first_row} and {row} (non_unique_old)"
            ),
            DiffError::UnsupportedFanout { first_row, row } => write!(
                f,
                "new key repeats at rows {first_row} and {row}; fanout is not supported yet"
            ),
            DiffError::IncompatibleColumns {
                column,
                old_type,
                new_type,
            } => write!(
                f,
                "column {column:?} has incompatible types {old_type} and {new_type}"
            ),
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

#[derive(Clone, Debug, PartialEq, Eq)]
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

    pub(crate) fn positions(&self) -> (usize, usize) {
        match self.0 {
            CoordinateRepr::Same(position) => (position, position),
            CoordinateRepr::Moved([old, new]) => (old, new),
        }
    }
}

/// A one-based old/new cell coordinate, collapsed when both positions agree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellCoordinate(CellCoordinateRepr);

#[derive(Clone, Debug, PartialEq, Eq)]
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

/// A type in the MVP comparison domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NormalizedType {
    Boolean,
    Int64,
    Double,
    String,
}

/// One column in an input schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnSchema {
    pub name: String,
    pub source_type: String,
    pub normalized_type: NormalizedType,
}

/// The original and normalized input schemas.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schemas {
    pub old: Vec<ColumnSchema>,
    pub new: Vec<ColumnSchema>,
}

/// Evidence that an identified column changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnEdit {
    pub column: Coordinate,
    pub type_changed: bool,
    pub values_changed: bool,
}

/// Resolved column identities and schema events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnsDiff {
    pub identities: Vec<Coordinate>,
    pub added: Vec<usize>,
    pub dropped: Vec<usize>,
    pub edited: Vec<ColumnEdit>,
}

/// How the row key was selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyBasis {
    Declared,
    Guessed,
}

/// Exact shared-value evidence behind a guessed key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyOverlap {
    pub shared: usize,
    pub possible: usize,
}

impl KeyOverlap {
    /// The normalized `shared / possible` ratio.
    ///
    /// Exact counts are what the model stores; the ratio is derived only where
    /// it is reported, so the model itself stays comparable with `Eq`.
    pub fn ratio(&self) -> f64 {
        self.shared as f64 / self.possible as f64
    }
}

/// The resolved row key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyDiff {
    pub basis: KeyBasis,
    pub columns: Vec<Coordinate>,
    pub overlap: Option<KeyOverlap>,
}

/// Row matching events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowsDiff {
    pub added: Vec<usize>,
    pub dropped: Vec<usize>,
    pub matched: Vec<Coordinate>,
}

/// Minimal relative-order changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrderDiff {
    pub columns: Vec<Coordinate>,
    pub rows: Vec<Coordinate>,
}

/// A minimum semantic summary of row and column edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditSummary {
    pub optimal: bool,
    pub columns: Vec<ColumnEdit>,
    pub rows: Vec<Coordinate>,
}

impl Default for EditSummary {
    fn default() -> Self {
        Self {
            optimal: true,
            columns: Vec::new(),
            rows: Vec::new(),
        }
    }
}

/// An inspectable, coordinate-only table diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diff {
    pub schemas: Schemas,
    pub columns: ColumnsDiff,
    pub key: KeyDiff,
    pub rows: RowsDiff,
    pub order: OrderDiff,
    pub cells: Vec<CellCoordinate>,
    pub summary: EditSummary,
}

#[cfg(test)]
mod tests {
    use super::{Coordinate, EditSummary, KeyOverlap};

    #[test]
    fn coordinate_collapses_equal_positions() {
        assert_eq!(Coordinate::from_zero_based(1, 1).positions(), (2, 2));
    }

    #[test]
    fn coordinate_retains_moved_positions() {
        assert_eq!(Coordinate::from_zero_based(2, 0).positions(), (3, 1));
    }

    #[test]
    fn overlap_normalizes_shared_by_possible() {
        let overlap = KeyOverlap {
            shared: 2,
            possible: 3,
        };
        assert_eq!(overlap.ratio(), 2.0 / 3.0);
    }

    #[test]
    fn empty_summary_is_still_optimal() {
        assert!(EditSummary::default().optimal);
    }
}
