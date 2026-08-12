//! Synthetic table pairs for the pipeline benchmarks.
//!
//! Each generator returns an `(old, new)` pair shaped like one scenario the
//! budgets are tuned against, parameterized by rows and columns. Every value
//! is a pure function of its coordinates — no random state anywhere — so a
//! benchmark measures the code and not the fixture, and two runs generate
//! byte-identical tables.
//!
//! Most pairs carry an `id` column holding the row index, so a benchmark can
//! declare the key and measure reconciliation rather than key guessing. The
//! `guessed_*` and `keyless_*` pairs omit it — they exist to measure the key
//! search itself, and an obvious unique column would end it at width one.

use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{Field, Schema};

/// An identical pair: the floor every bound is compared against, the run
/// being one linear pass of cell comparison with nothing to infer.
pub fn identical(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let build = || {
        table(
            names("c", columns),
            (0..columns).map(|column| int_column(rows, |row| distinct(column, row))),
        )
    };
    (build(), build())
}

/// Every column renamed in place with distinct values: the positional
/// pre-pass's O(columns) case.
pub fn renamed_distinct(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let values = |prefix| {
        table(
            names(prefix, columns),
            (0..columns).map(|column| int_column(rows, move |row| distinct(column, row))),
        )
    };
    (values("old"), values("new"))
}

/// Every column renamed in place with one shared constant value: the
/// digest-collision adversary, where every candidate digests like every other
/// and only budgeted verification separates them.
pub fn renamed_constant(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let values = |prefix| {
        table(
            names(prefix, columns),
            (0..columns).map(|_| int_column(rows, |_| 0)),
        )
    };
    (values("old"), values("new"))
}

/// Dropped columns against added ones related by rename-and-modify: the
/// quadratic approximate case, no pair exact and every pair measured.
pub fn rename_and_modify(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let old = table(
        names("old", columns),
        (0..columns).map(|column| int_column(rows, move |row| distinct(column, row))),
    );
    // One row in twenty edited, which keeps each true pair above the 90%
    // agreement bar while denying the exact stage every candidate.
    let new = table(
        names("new", columns),
        (0..columns).map(|column| {
            int_column(rows, move |row| {
                let value = distinct(column, row);
                if row % 20 == 0 { value + 1 } else { value }
            })
        }),
    );
    (old, new)
}

/// Same-named column pairs whose contents were exchanged: the swap adversary,
/// every identity rewritten under its own name and every crossing measured.
pub fn swapped(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let old = table(
        names("c", columns),
        (0..columns).map(|column| int_column(rows, move |row| distinct(column, row))),
    );
    // Exchange within each adjacent pair; an odd last column keeps its values.
    let new = table(
        names("c", columns),
        (0..columns).map(|column| {
            let partner = if column % 2 == 0 {
                (column + 1).min(columns - 1)
            } else {
                column - 1
            };
            int_column(rows, move |row| distinct(partner, row))
        }),
    );
    (old, new)
}

/// Every non-key value changed: the summarization adversary, one edge per
/// cell in the minimum-cover graph.
pub fn full_rewrite(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let old = table(
        names("c", columns),
        (0..columns).map(|column| int_column(rows, move |row| distinct(column, row))),
    );
    let new = table(
        names("c", columns),
        (0..columns).map(|column| int_column(rows, move |row| distinct(column, row) + 1)),
    );
    (old, new)
}

/// An identical all-string pair: the floor for string tables, where every
/// value clones its bytes during canonicalization and the same linear pass
/// costs what the integer floor hides.
pub fn identical_strings(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let build = || {
        table(
            names("c", columns),
            (0..columns).map(|column| string_column(rows, column)),
        )
    };
    (build(), build())
}

/// Every string column renamed in place with distinct values: the digest
/// join's string case, resolved by the positional pre-pass like its integer
/// counterpart.
pub fn renamed_strings(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let values = |prefix| {
        table(
            names(prefix, columns),
            (0..columns).map(|column| string_column(rows, column)),
        )
    };
    (values("old"), values("new"))
}

/// A hidden compound key: the honest keyless case the search exists for.
///
/// Neither group column is unique alone and the (g1, g2) tuple identifies
/// every row on both sides. Each payload column is unique too, but one row of
/// it was edited, so it shares one tuple fewer than the compound and the
/// ranking must prefer the evidence over the parsimony tie-break. Every
/// eligible candidate is terminal, so the lattice's one live path is the
/// hidden pair, and downstream sees an ordinary small edit.
pub fn guessed_compound(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let block = integer_sqrt(rows);
    let side = |edited: i64| {
        let mut named = vec![
            (
                "g1".to_owned(),
                int_column(rows, move |row| (row / block) as i64),
            ),
            (
                "g2".to_owned(),
                int_column(rows, move |row| (row % block) as i64),
            ),
        ];
        named.extend((0..columns).map(|column| {
            (
                format!("c{column}"),
                int_column(rows, move |row| {
                    let value = distinct(column, row);
                    if row == 0 { value + edited } else { value }
                }),
            )
        }));
        keyless_table(named)
    };
    (side(0), side(1))
}

/// Low-cardinality columns with no key at any width: the key-search adversary.
///
/// Every column holds a handful of values, every combination up to the width
/// cap stays duplicated, and the two sides are identical, so the whole cost is
/// the lattice the budgets exist to bound. The search must exhaust into the
/// positional fallback — which is also the correct answer, the files being
/// identical.
pub fn keyless_duplicates(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let build = || {
        keyless_table(
            (0..columns)
                .map(|column| {
                    (
                        format!("c{column}"),
                        int_column(rows, move |row| ((row >> (column % 16)) & 1) as i64),
                    )
                })
                .collect(),
        )
    };
    (build(), build())
}

/// One changed cell per row and column, on the diagonal: the issue #41
/// wide-table case. Every identity agrees in all but one row, so the swap
/// stage's rewritten filter measures each of the many columns and admits
/// none of them — the filter's per-column constant is what this stresses.
pub fn wide_diagonal(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let old = table(
        names("c", columns),
        (0..columns).map(|column| int_column(rows, move |row| distinct(column, row))),
    );
    let new = table(
        names("c", columns),
        (0..columns).map(|column| {
            int_column(rows, move |row| {
                let value = distinct(column, row);
                if row == column { value + 1 } else { value }
            })
        }),
    );
    (old, new)
}

/// Changed cells forming a path through the grid — column `c` changes in
/// rows `c` and `c + 1`: the second issue #41 shape, same stress as the
/// diagonal with two changes per column instead of one.
pub fn wide_path(rows: usize, columns: usize) -> (RecordBatch, RecordBatch) {
    let old = table(
        names("c", columns),
        (0..columns).map(|column| int_column(rows, move |row| distinct(column, row))),
    );
    let new = table(
        names("c", columns),
        (0..columns).map(|column| {
            int_column(rows, move |row| {
                let value = distinct(column, row);
                if row == column || row == column + 1 {
                    value + 1
                } else {
                    value
                }
            })
        }),
    );
    (old, new)
}

/// The largest `block` with `block * block <= rows`, without floating point.
fn integer_sqrt(rows: usize) -> usize {
    let mut block = 1;
    while (block + 1) * (block + 1) <= rows {
        block += 1;
    }
    block
}

/// A value distinct across both coordinates, so unrelated columns never agree
/// and a column's values never repeat: informative everywhere, colliding
/// nowhere. Deterministic by construction.
fn distinct(column: usize, row: usize) -> i64 {
    (row as i64) * 1_000_003 + (column as i64)
}

/// The string spelling of [`distinct`], with a prefix so the values exercise
/// real byte comparison rather than parsing back into integers.
fn string_column(rows: usize, column: usize) -> ArrayRef {
    Arc::new(StringArray::from_iter_values(
        (0..rows).map(|row| format!("value-{}", distinct(column, row))),
    ))
}

fn names(prefix: &str, columns: usize) -> Vec<String> {
    std::iter::once("id".to_owned())
        .chain((0..columns).map(|column| format!("{prefix}{column}")))
        .collect()
}

fn int_column(rows: usize, value: impl Fn(usize) -> i64) -> ArrayRef {
    Arc::new(Int64Array::from_iter_values((0..rows).map(value)))
}

/// Assemble the columns behind an `id` key column holding the row index.
fn table(names: Vec<String>, columns: impl Iterator<Item = ArrayRef>) -> RecordBatch {
    let rows_then_columns = columns.collect::<Vec<_>>();
    let rows = rows_then_columns
        .first()
        .map(|column| column.len())
        .unwrap_or(0);
    let mut arrays = vec![int_column(rows, |row| row as i64)];
    arrays.extend(rows_then_columns);
    assemble(names, arrays)
}

/// Assemble named columns as given, with no key column injected.
fn keyless_table(named: Vec<(String, ArrayRef)>) -> RecordBatch {
    let (names, arrays): (Vec<String>, Vec<ArrayRef>) = named.into_iter().unzip();
    assemble(names, arrays)
}

fn assemble(names: Vec<String>, arrays: Vec<ArrayRef>) -> RecordBatch {
    let fields = names
        .iter()
        .zip(&arrays)
        .map(|(name, array)| Field::new(name, array.data_type().clone(), true))
        .collect::<Vec<_>>();
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
        .expect("generated columns share one row count")
}
