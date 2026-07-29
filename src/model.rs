use std::path::PathBuf;

use crate::key::MAX_FANOUT_PERCENT;

/// Options that influence reconciliation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffOptions {
    /// Components of the declared compound key.
    ///
    /// A component is one column name used on both sides, or an `old/new` pair
    /// naming a column that differs between them.
    pub key: Vec<String>,
    /// Hints, each written in the human format's own line grammar.
    ///
    /// Raw spellings rather than parsed hints, so that parsing, and the errors
    /// it can raise, belong to the library rather than to each caller.
    pub hints: Vec<String>,
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
    /// A key component named more than one column per side.
    MalformedKeyComponent { component: String },
    /// More than one key component claimed the same column.
    DuplicateKeyColumn { side: Side, column: String },
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
    /// New-side duplication is too broad to be read as fanout.
    ExcessiveFanout { affected: usize, shared: usize },
    /// A hint could not be read as a line of the format's grammar.
    MalformedHint { hint: String },
    /// A hint named an operation that cannot be asserted.
    UnknownHintKind { hint: String, kind: String },
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
            DiffError::MalformedKeyComponent { component } => write!(
                f,
                "key component {component:?} must be one name or an old/new pair"
            ),
            DiffError::DuplicateKeyColumn { side, column } => write!(
                f,
                "{side} column {column:?} is claimed by more than one key component"
            ),
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
            DiffError::ExcessiveFanout { affected, shared } => write!(
                f,
                "declared key fans out for {affected} of {shared} shared key values, \
                 above the {MAX_FANOUT_PERCENT}% limit; supply a different --key"
            ),
            DiffError::MalformedHint { hint } => write!(
                f,
                "hint {hint:?} is not a line of the form col_drop(old), col_add(new), \
                 col_edit(column), or col_rename(old -> new)"
            ),
            DiffError::UnknownHintKind { hint, kind } => {
                write!(f, "hint {hint:?} names {kind:?}, which is not an operation")
            }
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
    pub fanout: Vec<FanoutEvent>,
}

/// One old row that corresponds to several new rows sharing its key.
///
/// The row positions are plain and one-based rather than `Coordinate`s: a
/// `Coordinate` pairs one old with one new position and collapses when they
/// agree, and a fanout has no such pair. The cells do pair, so they keep their
/// coordinate type; they are held here rather than in `Diff::cells` because a
/// one-to-many comparison is not evidence of an edit to a matched row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FanoutEvent {
    pub old: usize,
    pub new: Vec<usize>,
    pub cells: Vec<CellCoordinate>,
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

/// What a hint claimed, by name rather than by position.
///
/// Names rather than coordinates because a hint may name a column that does not
/// exist, which is one of the things an issue has to be able to report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HintClaim {
    pub kind: HintKind,
    pub names: HintNames,
}

/// The names a hint was written with.
///
/// As written rather than as resolved, so that reporting a hint back to its
/// author shows them what they typed. `col_drop(a)` has one name and
/// `col_rename(a -> b)` has two, and `col_edit` takes either form, so the shape
/// is the hint's own rather than something its kind determines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HintNames {
    /// One name, whose side the kind settles.
    Single(String),
    /// An old-to-new pair.
    Pair(String, String),
}

/// The kind of claim a hint makes against column identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintKind {
    /// Both endpoints are one column.
    Rename,
    /// The new endpoint has no partner.
    Add,
    /// The old endpoint has no partner.
    Drop,
    /// An identity changed, claiming no endpoint of its own.
    Edit,
}

impl HintKind {
    /// The operation name this kind is written and printed as.
    pub fn name(&self) -> &'static str {
        match self {
            HintKind::Rename => "col_rename",
            HintKind::Add => "col_add",
            HintKind::Drop => "col_drop",
            HintKind::Edit => "col_edit",
        }
    }
}

/// Something reconciliation declined to do, and why.
///
/// An issue is not a failure. It reports an instruction that could not be
/// followed, or an ambiguity left for the user, while the comparison itself
/// completed; the exit status is unaffected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub kind: IssueKind,
    /// The hints the issue concerns, in the order they were supplied.
    pub hints: Vec<HintClaim>,
}

/// A stable identifier for what went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IssueKind {
    /// A hint named a column that is absent from the side it named it on.
    HintMissingTarget { side: Side, column: String },
    /// Hints made claims that cannot all hold, so none of them was applied.
    ContradictoryHints,
    /// A hint claimed two columns are one, but their values cannot be compared.
    HintIncompatibleTypes { old_type: String, new_type: String },
    /// An edit hint named an identity that reconciliation did not establish.
    HintUnresolvedIdentity,
    /// An edit hint named an identity that changed in neither type nor value.
    HintNoChange,
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
    /// Instructions declined and ambiguities left unresolved.
    pub issues: Vec<Issue>,
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
