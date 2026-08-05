use std::path::PathBuf;

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
    /// Bounds on the stages whose worst case is not linear in the input.
    pub budgets: Budgets,
}

/// Counted bounds on the superlinear reconciliation stages.
///
/// Every budget is a count of deterministic work units — rows sampled, pairs
/// examined, changed cells admitted to the exact cover — never elapsed time,
/// which would make the output depend on the machine. Exhausting one changes
/// when the answer arrives, not whether it is valid: each bounded stage returns
/// a conservative partial result and reports itself in [`Diff::incomplete`].
///
/// The defaults are benchmark-tuned so that ordinary inputs never bind them;
/// they are exposed so tests and embedders can tighten or relax the bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budgets {
    /// The matched rows agreement is measured over.
    ///
    /// Above this many matched rows, approximate rename inference and swap
    /// inference measure a deterministic key-hash sample of exactly this size;
    /// at or below it, they measure every matched row. Exact rename inference
    /// always uses every matched row, its basis being a universal claim.
    /// Completing inference over the sample is not budget exhaustion.
    pub agreement_rows: usize,
    /// The candidate pairs rename inference may examine, across both stages.
    ///
    /// One unit is one value-level pair examination: a full-row verification
    /// of a digest-equal exact candidate, a full-row informativeness
    /// measurement, or a first-time sampled agreement measurement.
    pub rename_pairs: usize,
    /// The crossings swap inference may measure.
    ///
    /// One unit is one first-time sampled measurement of a crossed column
    /// pair. On exhaustion the stage accepts no swap at all, the cancellation
    /// rule being a judgement over the whole candidate set.
    pub swap_pairs: usize,
    /// The changed cells up to which the edit summary is exactly minimal.
    ///
    /// Above this many residual changed cells, the minimum-cover solve is
    /// skipped and each connected component is covered by its smaller affected
    /// side, columns when tied, with [`EditSummary::optimal`] set false.
    pub summary_cells: usize,
}

/// The defaults, tuned against `benches/pipeline.rs` (2026-08-04): on every
/// benchmark grid point of every adversarial scenario, each bounded stage
/// completes within twice the same-sized identical run — the pipeline's
/// unavoidable linear pass — and no non-adversarial scenario reports an
/// incomplete stage. The pair budgets sit at the small end of that rule's
/// range because a unit touches up to a sample's worth of rows, so at the
/// grid's smallest tables a budget an order larger buys the adversaries more
/// time than the whole linear pass it is measured against.
impl Default for Budgets {
    fn default() -> Self {
        Self {
            agreement_rows: 4096,
            rename_pairs: 2048,
            swap_pairs: 2048,
            summary_cells: 10_000,
        }
    }
}

/// A reconciliation stage its budget cut short.
///
/// Each variant's partial result is valid but conservative: unexamined rename
/// candidates stay drops and additions, an unfinished swap enumeration accepts
/// nothing, and an uncapped summary covers every cell with possibly more events
/// than the minimum. The complete cell-level diff is never affected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncompleteStage {
    Renames,
    Swaps,
    Summary,
}

impl IncompleteStage {
    /// The operation word the human format writes this stage as.
    pub fn name(&self) -> &'static str {
        match self {
            IncompleteStage::Renames => "incomplete_renames",
            IncompleteStage::Swaps => "incomplete_swaps",
            IncompleteStage::Summary => "incomplete_summary",
        }
    }
}

/// An error that prevents a complete diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffError {
    ReadParquet {
        path: PathBuf,
        message: String,
    },
    DuplicateColumnNames {
        side: Side,
        duplicates: Vec<DuplicateColumnName>,
    },
    UnsupportedColumn {
        side: Side,
        column: String,
        source_type: String,
    },
    IntegerOutOfRange {
        side: Side,
        column: String,
        source_type: String,
        row: usize,
    },
    EmptyKeyComponent,
    MalformedKeyComponent {
        component: String,
    },
    DuplicateKeyColumn {
        side: Side,
        column: String,
    },
    CompoundPositionalKey,
    MalformedHint {
        hint: String,
    },
    UnknownHintKind {
        hint: String,
        kind: String,
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
            DiffError::EmptyKeyComponent => f.write_str("the key contains an empty component"),
            DiffError::MalformedKeyComponent { component } => write!(
                f,
                "key component {component:?} must be one name or an old/new pair"
            ),
            DiffError::DuplicateKeyColumn { side, column } => write!(
                f,
                "{side} column {column:?} is claimed by more than one key component"
            ),
            DiffError::CompoundPositionalKey => f.write_str(
                "key component \"#row\" matches rows by position and cannot be combined \
                 with a column",
            ),
            DiffError::MalformedHint { hint } => write!(
                f,
                "hint {hint:?} is not a line of the form col_drop(old), col_add(new), \
                 col_edit(column), or col_rename(old -> new)"
            ),
            DiffError::UnknownHintKind { hint, kind } => {
                write!(f, "hint {hint:?} names {kind:?}, which is not an operation")
            }
        }
    }
}

impl std::error::Error for DiffError {}

/// Which input produced a validation issue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// A type in the comparison domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NormalizedType {
    Boolean,
    Int64,
    Double,
    String,
    /// A timestamp, aware or naive: comparable within its awareness across
    /// units and timezones, and against ISO 8601 strings.
    Timestamp,
    /// A calendar date: comparable across `Date32` and `Date64`, against
    /// naive timestamps at midnight, and against ISO 8601 date strings.
    Date,
    /// A decimal: comparable across precision, scale, and width, against
    /// integers and doubles when the value is exact, and against exact
    /// numeric strings.
    Decimal,
    /// Admitted but outside the matrix: comparable only against the identical
    /// source type, by canonical row-format bytes.
    Opaque,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnSchema {
    pub name: String,
    pub source_type: String,
    pub normalized_type: NormalizedType,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schemas {
    pub old: Vec<ColumnSchema>,
    pub new: Vec<ColumnSchema>,
}

/// Evidence that an identified column changed.
///
/// `changes` counts every changed cell in the column, over the one-to-one
/// matched rows. It is positive exactly when the values changed, which is why
/// it carries what a `values_changed` flag used to: the flag was this number
/// with its magnitude thrown away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnEdit {
    pub column: Coordinate,
    pub type_changed: bool,
    pub changes: usize,
}

/// Evidence that a matched row changed.
///
/// `changes` counts every changed cell in the row, over the identified columns.
/// A row edit has no aspect but its values, so unlike a column edit this count
/// is never zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowEdit {
    pub row: Coordinate,
    pub changes: usize,
}

/// Resolved column identities and schema events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnsDiff {
    pub identities: Vec<ColumnIdentity>,
    pub added: Vec<usize>,
    pub dropped: Vec<usize>,
    pub edited: Vec<ColumnEdit>,
}

/// One column identified across the two files, and how it was identified.
///
/// The coordinate is the whole of the identity; the basis is how reconciliation
/// arrived at it, which a reader needs because some of the ways are certainties
/// and some are judgements. Key-ness is deliberately absent: `KeyDiff::columns`
/// already says which identities the key is made of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnIdentity {
    pub column: Coordinate,
    pub basis: IdentityBasis,
}

/// What established a column identity.
///
/// Every pair in the bijection carries one, not only the pairs whose two names
/// differ: a stage that wants to know whether an identity is a provisional
/// same-name one should ask rather than compare names again, and a consumer of
/// `Diff` should find every identity described the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityBasis {
    /// A component of the declared key, whether it named one column or a pair.
    Declared,
    /// An accepted `col_rename` hint.
    Hinted,
    /// Both files calling the column the same thing.
    Name,
    /// Inference, from values that agree in every matched row.
    Exact,
    /// Inference, from values that agree closely and by more than chance.
    Approximate,
    /// Swap inference, exchanging this identity's new end with another's.
    Swapped,
}

impl IdentityBasis {
    /// The word the human format writes this basis as.
    ///
    /// `Name` has one for completeness rather than for use: a same-named
    /// identity is not a rename, so it reaches no line of the output.
    pub fn name(&self) -> &'static str {
        match self {
            IdentityBasis::Declared => "declared",
            IdentityBasis::Hinted => "hinted",
            IdentityBasis::Name => "name",
            IdentityBasis::Exact => "exact",
            IdentityBasis::Approximate => "approximate",
            IdentityBasis::Swapped => "swapped",
        }
    }
}

/// How the row key was selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyBasis {
    Declared,
    Guessed,
    /// Row position, reached because nothing else could identify a row.
    ///
    /// Declaring `#row` reaches the same key under `Declared`, so this basis
    /// says the tool ran out of alternatives rather than that rows are matched
    /// positionally, which the empty column list says on its own.
    Fallback,
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
///
/// `columns` is empty exactly for the positional key, whether that key was
/// declared as `#row` or fallen back to, since a declared key of no components
/// is refused before it gets here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyDiff {
    pub basis: KeyBasis,
    pub columns: Vec<Coordinate>,
    pub overlap: Option<KeyOverlap>,
    /// A declared key this data could not support, which is why the basis is
    /// not `Declared`.
    pub rejection: Option<KeyRejection>,
    /// A guessed key the tool itself withdrew, which this key replaced.
    pub retraction: Option<KeyRetraction>,
}

/// A guessed key the tool withdrew after seeing the diff it produced.
///
/// Not a [`KeyRejection`]: a rejected key is a declaration the data could not
/// support, refused before any row was matched, while a retracted key validated
/// and was then judged by its own diff. The mass holds the measurement the
/// `key_retracted()` line omits, as a rejection's variants hold their detail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRetraction {
    pub columns: Vec<KeyComponent>,
    pub mass: ChangeMass,
}

/// How much of the two files a diff accounts as changed.
///
/// Cell-denominated and symmetric: a dropped or added row contributes its whole
/// width once, and a changed matched cell exists in both files and contributes
/// two. Fanout groups are outside both counts, and under the fallback basis
/// added rows are read as appended and leave `changed` while staying in
/// `total`. Exact counts rather than a ratio, so the model stays `Eq` and the
/// threshold is applied where it is defined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeMass {
    pub changed: usize,
    pub total: usize,
}

/// A declared key that could not be used, and what about it failed.
///
/// This is not an [`Issue`], which is an instruction declined and names the
/// hints it concerns. A rejected key concerns none, so it is recorded here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRejection {
    pub subject: KeySubject,
    pub reason: RejectionReason,
}

/// What a rejection is about.
///
/// Resolving a component can fail on its own account, while uniqueness and
/// fanout are properties of the whole tuple and blame no one component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeySubject {
    /// One component of the declared key.
    Component(KeyComponent),
    /// The declared key entire.
    Key(Vec<KeyComponent>),
}

/// One declared key component, named on each side.
///
/// The two names come from parsing rather than from resolution, so a component
/// can be named in a rejection even when the column it names was never found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyComponent {
    pub old: String,
    pub new: String,
}

/// Why a declared key was rejected.
///
/// Each variant carries what the fatal error it replaces used to report, so
/// making these recoverable moves the detail into the diff rather than losing
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RejectionReason {
    MissingColumn { side: Side },
    IncompatibleTypes { old_type: String, new_type: String },
    DuplicateColumn { side: Side },
    InvalidValue { side: Side, row: usize },
    NonUniqueOld { first_row: usize, row: usize },
    ExcessiveFanout { affected: usize, shared: usize },
}

impl RejectionReason {
    pub fn name(&self) -> &'static str {
        match self {
            RejectionReason::MissingColumn { .. } => "missing_column",
            RejectionReason::IncompatibleTypes { .. } => "incompatible_types",
            RejectionReason::DuplicateColumn { .. } => "duplicate_column",
            RejectionReason::InvalidValue { .. } => "invalid_value",
            RejectionReason::NonUniqueOld { .. } => "non_unique_old",
            RejectionReason::ExcessiveFanout { .. } => "excessive_fanout",
        }
    }
}

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrderDiff {
    pub columns: Vec<Coordinate>,
    pub rows: Vec<Coordinate>,
}

/// A minimum semantic summary of row and column edits.
///
/// Each event carries a count of every changed cell incident to it, so the
/// counts of a row edit and a column edit that cross both include the cell they
/// share. They therefore do not sum to the number of changed cells, and are not
/// meant to: a count is a fact about its own row or column rather than a share
/// of a partition, and the events themselves already overlap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditSummary {
    pub optimal: bool,
    pub columns: Vec<ColumnEdit>,
    pub rows: Vec<RowEdit>,
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
    Single(String),
    Pair(String, String),
}

/// The kind of claim a hint makes against column identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintKind {
    Rename,
    Add,
    Drop,
    Edit,
}

impl HintKind {
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
    pub hints: Vec<HintClaim>,
}

/// A stable identifier for what went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IssueKind {
    /// A hint named a column that is absent from the side it named it on.
    HintMissingTarget { side: Side, column: String },
    /// Hints made claims that cannot all hold, so none of them was applied.
    ContradictoryHints,
    /// An edit hint named an identity that reconciliation did not establish.
    HintUnresolvedIdentity,
    /// An edit hint named an identity that changed in neither type nor value.
    HintNoChange,
}

/// A brief description of a file with nothing to compare it against.
///
/// `side` is the side that exists: `New` for an added file, `Old` for a
/// removed one. Nothing else is held because nothing else is known — no
/// reconciliation ran, so a key, an identity map, or a cell set here would
/// claim one had. The rows are a count rather than positions: a wholly
/// one-sided file's positions are `1..=n` by construction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OneSidedDiff {
    pub side: Side,
    pub columns: Vec<ColumnSchema>,
    pub rows: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diff {
    pub schemas: Schemas,
    pub columns: ColumnsDiff,
    pub key: KeyDiff,
    pub rows: RowsDiff,
    pub order: OrderDiff,
    pub cells: Vec<CellCoordinate>,
    pub summary: EditSummary,
    /// Stages the budgets cut short, in the fixed order renames, swaps,
    /// summary, describing the pass the diff kept.
    pub incomplete: Vec<IncompleteStage>,
    /// Instructions declined and ambiguities left unresolved.
    pub issues: Vec<Issue>,
    /// Set when the diff is not credible as a story of edits: change outweighs
    /// sameness under a key the tool chose rather than the user. The human
    /// format then writes `table_regenerate()` in place of the row-level
    /// findings; everything underneath — cells, summary, row events — is still
    /// computed and held here.
    pub regeneration: Option<ChangeMass>,
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
