use arrow_array::RecordBatch;

use crate::value::{Value, extract};
use crate::{DiffError, Side};

/// Lazy lookup of cell and row values from the two input tables.
///
/// A `Diff` deliberately retains no values — `Diff::cells` holds only
/// coordinates and added or dropped rows are only positions — so a consumer
/// that renders values fetches them here, on demand and page-sized. The
/// lookup borrows the batches the caller already read and knows positions
/// and arrays, nothing about the diff: translating `CellCoordinate`s, row
/// events, and fanout events into positions is the caller's work, done
/// through the model's accessors. That keeps reconciliation the single
/// source of truth and this surface trivially deterministic.
///
/// All positions are one-based, matching the model's convention.
pub struct Lookup<'a> {
    old: &'a RecordBatch,
    new: &'a RecordBatch,
}

impl<'a> Lookup<'a> {
    pub fn new(old: &'a RecordBatch, new: &'a RecordBatch) -> Self {
        Self { old, new }
    }

    fn table(&self, side: Side) -> &RecordBatch {
        match side {
            Side::Old => self.old,
            Side::New => self.new,
        }
    }

    /// The value at one one-based `(row, column)` position of a side.
    pub fn value(&self, side: Side, row: u32, column: u32) -> Result<Value, DiffError> {
        let table = self.table(side);
        self.check_row(table, side, row)?;
        if !(1..=table.num_columns() as u32).contains(&column) {
            return Err(DiffError::ColumnOutOfRange {
                side,
                column,
                columns: table.num_columns(),
            });
        }
        Ok(extract(
            table.column(column as usize - 1).as_ref(),
            row as usize - 1,
        ))
    }

    /// The values at a set of one-based positions, in request order.
    ///
    /// The paginated form of [`Self::value`]: the caller passes a page of
    /// coordinates and the page size stays the caller's, this being the
    /// batch fetch rather than a pagination machinery of its own.
    pub fn values(&self, side: Side, coords: &[(u32, u32)]) -> Result<Vec<Value>, DiffError> {
        coords
            .iter()
            .map(|&(row, column)| self.value(side, row, column))
            .collect()
    }

    /// Every value of one one-based row of a side, in schema order.
    pub fn row(&self, side: Side, row: u32) -> Result<Vec<Value>, DiffError> {
        let table = self.table(side);
        self.check_row(table, side, row)?;
        Ok(table
            .columns()
            .iter()
            .map(|column| extract(column.as_ref(), row as usize - 1))
            .collect())
    }

    fn check_row(&self, table: &RecordBatch, side: Side, row: u32) -> Result<(), DiffError> {
        if !(1..=table.num_rows() as u32).contains(&row) {
            return Err(DiffError::RowOutOfRange {
                side,
                row,
                rows: table.num_rows(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use test_support::table;

    use super::Lookup;
    use crate::{DiffError, Side, Value};

    fn tables() -> (arrow_array::RecordBatch, arrow_array::RecordBatch) {
        let old = table! {
            "id" => [1, 2],
            "price" => [Some(9), None],
        };
        let new = table! {
            "id" => [1, 2, 3],
            "price" => [Some(9), Some(12), Some(1)],
        };
        (old, new)
    }

    #[test]
    fn positions_are_one_based() {
        let (old, new) = tables();
        let lookup = Lookup::new(&old, &new);

        assert_eq!(lookup.value(Side::Old, 1, 1), Ok(Value::Int64(1)));
        assert_eq!(lookup.value(Side::Old, 2, 2), Ok(Value::Null));
        assert_eq!(lookup.value(Side::New, 3, 2), Ok(Value::Int64(1)));
    }

    #[test]
    fn values_answer_in_request_order_with_duplicates_preserved() {
        let (old, new) = tables();
        let lookup = Lookup::new(&old, &new);

        let values = lookup.values(Side::New, &[(3, 1), (1, 2), (3, 1)]).unwrap();
        assert_eq!(values, [Value::Int64(3), Value::Int64(9), Value::Int64(3)]);
    }

    #[test]
    fn a_row_reads_every_column_in_schema_order() {
        let (old, new) = tables();
        let lookup = Lookup::new(&old, &new);

        assert_eq!(
            lookup.row(Side::Old, 2),
            Ok(vec![Value::Int64(2), Value::Null])
        );
    }

    #[test]
    fn out_of_range_positions_are_checked_errors_naming_the_side() {
        let (old, new) = tables();
        let lookup = Lookup::new(&old, &new);

        assert_eq!(
            lookup.value(Side::Old, 3, 1),
            Err(DiffError::RowOutOfRange {
                side: Side::Old,
                row: 3,
                rows: 2,
            })
        );
        assert_eq!(
            lookup.value(Side::New, 1, 0),
            Err(DiffError::ColumnOutOfRange {
                side: Side::New,
                column: 0,
                columns: 2,
            })
        );
        assert_eq!(
            lookup.row(Side::New, 4),
            Err(DiffError::RowOutOfRange {
                side: Side::New,
                row: 4,
                rows: 3,
            })
        );
        // The boundary itself is in range.
        assert!(lookup.value(Side::Old, 2, 2).is_ok());
    }

    #[test]
    fn repeated_lookups_are_identical() {
        let (old, new) = tables();
        let lookup = Lookup::new(&old, &new);

        let first = lookup.values(Side::New, &[(1, 1), (2, 2), (3, 2)]).unwrap();
        let second = lookup.values(Side::New, &[(1, 1), (2, 2), (3, 2)]).unwrap();
        assert_eq!(first, second);
    }
}
