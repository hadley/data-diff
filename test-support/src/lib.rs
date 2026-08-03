//! Fixture construction shared by the `data-diff` test suite.
//!
//! Tests build tables from literal values and let the Arrow type follow from
//! them, so a fixture reads as the data it is rather than as the machinery
//! that assembles it:
//!
//! ```
//! use test_support::table;
//!
//! let old = table! {
//!     "id" => [1, 2, 3],
//!     "label" => ["a", "b", "c"],
//! };
//! assert_eq!(old.num_rows(), 3);
//! ```
//!
//! A column carries an annotation when its Arrow type is part of what the test
//! says, and an annotation is the only way to type a column with no values:
//!
//! ```
//! use test_support::table;
//!
//! let narrow = table! { "id" => i32[1, 2] };
//! let blank = table! { "id" => i64[] };
//! ```

use std::sync::Arc;

use arrow_array::types::Int8Type;
use arrow_array::{
    ArrayRef, BinaryArray, BooleanArray, Date32Array, DictionaryArray, Float32Array, Float64Array,
    Int8Array, Int16Array, Int32Array, Int64Array, LargeStringArray, RecordBatch,
    RecordBatchOptions, StringArray, TimestampMillisecondArray, UInt8Array, UInt16Array,
    UInt32Array, UInt64Array,
};
use arrow_schema::{DataType, Field, Schema, TimeUnit};

/// The Arrow type family a Rust value type belongs to.
///
/// A kind is what the values themselves determine; an annotation chooses a
/// representation within one kind and cannot cross between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Int,
    Double,
    Text,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Bool(bool),
    Int(i128),
    Double(f64),
    Text(String),
}

/// A Rust type that can be written as a fixture value.
///
/// Every element of a column is one Rust array literal, so the whole column
/// shares one implementation and therefore one [`Kind`], even where the values
/// are all null.
pub trait CellValue: Sized {
    const KIND: Kind;

    fn cell(self) -> Cell;
}

impl CellValue for bool {
    const KIND: Kind = Kind::Bool;

    fn cell(self) -> Cell {
        Cell::Bool(self)
    }
}

impl CellValue for i32 {
    const KIND: Kind = Kind::Int;

    fn cell(self) -> Cell {
        Cell::Int(self.into())
    }
}

impl CellValue for i64 {
    const KIND: Kind = Kind::Int;

    fn cell(self) -> Cell {
        Cell::Int(self.into())
    }
}

impl CellValue for u64 {
    const KIND: Kind = Kind::Int;

    fn cell(self) -> Cell {
        Cell::Int(self.into())
    }
}

impl CellValue for f64 {
    const KIND: Kind = Kind::Double;

    fn cell(self) -> Cell {
        Cell::Double(self)
    }
}

impl CellValue for &str {
    const KIND: Kind = Kind::Text;

    fn cell(self) -> Cell {
        Cell::Text(self.to_owned())
    }
}

impl<T: CellValue> CellValue for Option<T> {
    const KIND: Kind = T::KIND;

    fn cell(self) -> Cell {
        match self {
            Some(value) => value.cell(),
            None => Cell::Null,
        }
    }
}

pub fn column<T: CellValue, const N: usize>(
    values: [T; N],
    annotation: &str,
    context: Option<&str>,
) -> ArrayRef {
    let cells = values.into_iter().map(CellValue::cell).collect();
    let data_type = resolve(annotation, Some(T::KIND), context);
    build(cells, &data_type, annotation, context)
}

pub fn empty_column(annotation: &str, context: Option<&str>) -> ArrayRef {
    let data_type = resolve(annotation, None, context);
    build(Vec::new(), &data_type, annotation, context)
}

pub fn table_from_columns(columns: Vec<(&str, ArrayRef)>) -> RecordBatch {
    let fields = columns
        .iter()
        .map(|(name, values)| Field::new(*name, values.data_type().clone(), true))
        .collect::<Vec<_>>();
    let arrays = columns.into_iter().map(|(_, values)| values).collect();
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).expect("fixture columns agree")
}

pub fn empty_table() -> RecordBatch {
    RecordBatch::new_empty(Arc::new(Schema::empty()))
}

pub fn rows_without_columns(rows: usize) -> RecordBatch {
    RecordBatch::try_new_with_options(
        Arc::new(Schema::empty()),
        vec![],
        &RecordBatchOptions::new().with_row_count(Some(rows)),
    )
    .expect("a row count alone is a valid batch")
}

/// Resolve the Arrow type of a column from its annotation and its values.
fn resolve(annotation: &str, kind: Option<Kind>, context: Option<&str>) -> DataType {
    if annotation.is_empty() {
        return match kind.expect("an unannotated column has values to infer from") {
            Kind::Bool => DataType::Boolean,
            Kind::Int => DataType::Int64,
            Kind::Double => DataType::Float64,
            Kind::Text => DataType::Utf8,
        };
    }

    let (required, data_type) = match annotation {
        "bool" => (Kind::Bool, DataType::Boolean),
        "i8" => (Kind::Int, DataType::Int8),
        "i16" => (Kind::Int, DataType::Int16),
        "i32" => (Kind::Int, DataType::Int32),
        "i64" => (Kind::Int, DataType::Int64),
        "u8" => (Kind::Int, DataType::UInt8),
        "u16" => (Kind::Int, DataType::UInt16),
        "u32" => (Kind::Int, DataType::UInt32),
        "u64" => (Kind::Int, DataType::UInt64),
        "f32" => (Kind::Double, DataType::Float32),
        "f64" => (Kind::Double, DataType::Float64),
        "str" => (Kind::Text, DataType::Utf8),
        "large_str" => (Kind::Text, DataType::LargeUtf8),
        "dict" => (
            Kind::Text,
            DataType::Dictionary(Box::new(DataType::Int8), Box::new(DataType::Utf8)),
        ),
        "binary" => (Kind::Text, DataType::Binary),
        "date32" => (Kind::Int, DataType::Date32),
        "ts_ms" => (
            Kind::Int,
            DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into())),
        ),
        "ts_ms_naive" => (Kind::Int, DataType::Timestamp(TimeUnit::Millisecond, None)),
        unknown => fail(context, format!("unknown type annotation `{unknown}`")),
    };

    if let Some(kind) = kind
        && kind != required
    {
        fail(
            context,
            format!(
                "annotation `{annotation}` cannot hold {} values",
                describe(kind)
            ),
        );
    }
    data_type
}

fn build(
    cells: Vec<Cell>,
    data_type: &DataType,
    annotation: &str,
    context: Option<&str>,
) -> ArrayRef {
    let label = if annotation.is_empty() {
        default_label(data_type)
    } else {
        annotation
    };

    match data_type {
        DataType::Boolean => Arc::new(BooleanArray::from(booleans(cells))),
        DataType::Int8 => Arc::new(Int8Array::from(narrow(cells, label, context))),
        DataType::Int16 => Arc::new(Int16Array::from(narrow(cells, label, context))),
        DataType::Int32 => Arc::new(Int32Array::from(narrow(cells, label, context))),
        DataType::Int64 => Arc::new(Int64Array::from(narrow(cells, label, context))),
        DataType::UInt8 => Arc::new(UInt8Array::from(narrow(cells, label, context))),
        DataType::UInt16 => Arc::new(UInt16Array::from(narrow(cells, label, context))),
        DataType::UInt32 => Arc::new(UInt32Array::from(narrow(cells, label, context))),
        DataType::UInt64 => Arc::new(UInt64Array::from(narrow(cells, label, context))),
        DataType::Date32 => Arc::new(Date32Array::from(narrow(cells, label, context))),
        DataType::Timestamp(TimeUnit::Millisecond, timezone) => Arc::new(
            TimestampMillisecondArray::from(narrow::<i64>(cells, label, context))
                .with_timezone_opt(timezone.clone()),
        ),
        DataType::Float32 => Arc::new(Float32Array::from(
            doubles(cells)
                .into_iter()
                .map(|value| value.map(|value| value as f32))
                .collect::<Vec<_>>(),
        )),
        DataType::Float64 => Arc::new(Float64Array::from(doubles(cells))),
        DataType::Utf8 => Arc::new(texts(cells).into_iter().collect::<StringArray>()),
        DataType::LargeUtf8 => Arc::new(texts(cells).into_iter().collect::<LargeStringArray>()),
        DataType::Binary => Arc::new(texts(cells).into_iter().collect::<BinaryArray>()),
        DataType::Dictionary(..) => dictionary(texts(cells)),
        other => unreachable!("no annotation resolves to {other:?}"),
    }
}

fn dictionary(values: Vec<Option<String>>) -> ArrayRef {
    let mut distinct: Vec<String> = Vec::new();
    let mut keys: Vec<Option<i8>> = Vec::new();
    for value in values {
        let Some(text) = value else {
            keys.push(None);
            continue;
        };
        let index = distinct
            .iter()
            .position(|existing| *existing == text)
            .unwrap_or_else(|| {
                distinct.push(text);
                distinct.len() - 1
            });
        keys.push(Some(
            i8::try_from(index).expect("fixture dictionaries stay small"),
        ));
    }

    Arc::new(
        DictionaryArray::<Int8Type>::try_new(
            Int8Array::from(keys),
            Arc::new(distinct.into_iter().map(Some).collect::<StringArray>()),
        )
        .expect("keys index the values they were built from"),
    )
}

fn booleans(cells: Vec<Cell>) -> Vec<Option<bool>> {
    cells
        .into_iter()
        .map(|cell| match cell {
            Cell::Null => None,
            Cell::Bool(value) => Some(value),
            other => unreachable!("{other:?} in a boolean column"),
        })
        .collect()
}

fn doubles(cells: Vec<Cell>) -> Vec<Option<f64>> {
    cells
        .into_iter()
        .map(|cell| match cell {
            Cell::Null => None,
            Cell::Double(value) => Some(value),
            other => unreachable!("{other:?} in a double column"),
        })
        .collect()
}

fn texts(cells: Vec<Cell>) -> Vec<Option<String>> {
    cells
        .into_iter()
        .map(|cell| match cell {
            Cell::Null => None,
            Cell::Text(value) => Some(value),
            other => unreachable!("{other:?} in a text column"),
        })
        .collect()
}

fn narrow<T: TryFrom<i128>>(
    cells: Vec<Cell>,
    label: &str,
    context: Option<&str>,
) -> Vec<Option<T>> {
    cells
        .into_iter()
        .map(|cell| match cell {
            Cell::Null => None,
            Cell::Int(value) => Some(
                T::try_from(value)
                    .unwrap_or_else(|_| fail(context, format!("{value} does not fit {label}"))),
            ),
            other => unreachable!("{other:?} in an integer column"),
        })
        .collect()
}

fn describe(kind: Kind) -> &'static str {
    match kind {
        Kind::Bool => "boolean",
        Kind::Int => "integer",
        Kind::Double => "double",
        Kind::Text => "text",
    }
}

fn default_label(data_type: &DataType) -> &'static str {
    match data_type {
        DataType::Boolean => "bool",
        DataType::Int64 => "i64",
        DataType::Float64 => "f64",
        _ => "str",
    }
}

fn fail(context: Option<&str>, message: String) -> ! {
    match context {
        Some(name) => panic!("column {name:?}: {message}"),
        None => panic!("{message}"),
    }
}

/// Build one column, for the tests that compare arrays rather than tables.
///
/// ```
/// use test_support::column;
///
/// let values = column!(["1", "1.0"]);
/// let narrow = column!(i32[10, 20]);
/// ```
#[macro_export]
macro_rules! column {
    ($($tokens:tt)*) => {
        $crate::__column!(::core::option::Option::None, $($tokens)*)
    };
}

/// Build a table from named columns.
///
/// ```
/// use test_support::table;
///
/// let old = table! {
///     "id" => [1, 2],
///     "score" => [Some(1.5), None],
/// };
/// let schemaless = table! {};
/// ```
#[macro_export]
macro_rules! table {
    () => {
        $crate::empty_table()
    };
    ($($name:literal => $($annotation:ident)? [$($value:expr),* $(,)?]),+ $(,)?) => {
        $crate::table_from_columns(vec![$((
            $name,
            $crate::__column!(
                ::core::option::Option::Some($name),
                $($annotation)? [$($value),*]
            ),
        )),+])
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __column {
    ($context:expr, []) => {
        compile_error!("a column with no values needs a type annotation, as `i64[]`")
    };
    ($context:expr, $annotation:ident []) => {
        $crate::empty_column(stringify!($annotation), $context)
    };
    ($context:expr, $($annotation:ident)? [$($value:expr),+ $(,)?]) => {
        $crate::column([$($value),+], stringify!($($annotation)?), $context)
    };
}

#[cfg(test)]
mod tests {
    use arrow_array::cast::AsArray;
    use arrow_array::{Array, Int8Array, StringArray};
    use arrow_schema::DataType;

    fn column_type(table: &arrow_array::RecordBatch, index: usize) -> &DataType {
        table.schema_ref().field(index).data_type()
    }

    #[test]
    fn values_alone_decide_the_arrow_type() {
        let table = table! {
            "bool" => [true],
            "int" => [1],
            "wide_int" => [1_i64],
            "double" => [1.5],
            "text" => ["a"],
        };

        assert_eq!(column_type(&table, 0), &DataType::Boolean);
        assert_eq!(column_type(&table, 1), &DataType::Int64);
        assert_eq!(column_type(&table, 2), &DataType::Int64);
        assert_eq!(column_type(&table, 3), &DataType::Float64);
        assert_eq!(column_type(&table, 4), &DataType::Utf8);
    }

    #[test]
    fn every_annotation_names_one_representation() {
        let table = table! {
            "bool" => bool[true],
            "i8" => i8[1],
            "i16" => i16[1],
            "i32" => i32[1],
            "i64" => i64[1],
            "u8" => u8[1],
            "u16" => u16[1],
            "u32" => u32[1],
            "u64" => u64[1],
            "f32" => f32[1.0],
            "f64" => f64[1.0],
            "str" => str["a"],
            "large_str" => large_str["a"],
            "dict" => dict["a"],
            "binary" => binary["a"],
        };

        let types = table
            .schema_ref()
            .fields()
            .iter()
            .map(|field| field.data_type().clone())
            .collect::<Vec<_>>();
        assert_eq!(
            types,
            [
                DataType::Boolean,
                DataType::Int8,
                DataType::Int16,
                DataType::Int32,
                DataType::Int64,
                DataType::UInt8,
                DataType::UInt16,
                DataType::UInt32,
                DataType::UInt64,
                DataType::Float32,
                DataType::Float64,
                DataType::Utf8,
                DataType::LargeUtf8,
                DataType::Dictionary(Box::new(DataType::Int8), Box::new(DataType::Utf8)),
                DataType::Binary,
            ]
        );
    }

    #[test]
    fn missing_values_keep_their_place_and_their_type() {
        let table = table! {
            "id" => [Some(1), None, Some(3)],
            "label" => [None::<&str>, None, None],
        };

        let ids = table
            .column(0)
            .as_primitive::<arrow_array::types::Int64Type>();
        assert_eq!(ids.len(), 3);
        assert!(!ids.is_null(0));
        assert!(ids.is_null(1));
        assert_eq!(ids.value(2), 3);

        assert_eq!(column_type(&table, 1), &DataType::Utf8);
        assert!(table.column(1).is_null(0));
    }

    #[test]
    fn integers_reach_the_whole_unsigned_range() {
        let table = table! { "id" => u64[Some(1), None, Some(u64::MAX)] };

        let ids = table
            .column(0)
            .as_primitive::<arrow_array::types::UInt64Type>();
        assert_eq!(ids.value(0), 1);
        assert!(ids.is_null(1));
        assert_eq!(ids.value(2), u64::MAX);
    }

    #[test]
    fn dictionaries_number_values_as_they_first_appear() {
        let encoded = column!(dict["b", "a", "b"]);
        let encoded = encoded.as_dictionary::<arrow_array::types::Int8Type>();

        assert_eq!(encoded.keys(), &Int8Array::from(vec![0, 1, 0]));
        assert_eq!(
            encoded.values().as_ref(),
            &StringArray::from(vec!["b", "a"]) as &dyn Array
        );
    }

    #[test]
    fn binary_columns_hold_the_bytes_of_their_values() {
        let bytes = column!(binary["a"]);

        assert_eq!(bytes.as_binary::<i32>().value(0), b"a");
    }

    #[test]
    fn an_empty_column_is_typed_by_its_annotation() {
        let table = table! { "id" => i32[], "label" => str[] };

        assert_eq!(table.num_rows(), 0);
        assert_eq!(column_type(&table, 0), &DataType::Int32);
        assert_eq!(column_type(&table, 1), &DataType::Utf8);
    }

    #[test]
    fn tables_without_columns_and_rows_without_columns_are_distinct() {
        let schemaless = table! {};
        assert_eq!(schemaless.num_columns(), 0);
        assert_eq!(schemaless.num_rows(), 0);

        let rows = super::rows_without_columns(2);
        assert_eq!(rows.num_columns(), 0);
        assert_eq!(rows.num_rows(), 2);
    }

    #[test]
    fn every_column_is_nullable() {
        let table = table! { "id" => [1] };

        assert!(table.schema_ref().field(0).is_nullable());
    }

    #[test]
    #[should_panic(expected = "column \"id\": unknown type annotation `banana`")]
    fn an_unknown_annotation_names_the_column() {
        let _ = table! { "id" => banana[1] };
    }

    #[test]
    #[should_panic(expected = "annotation `dict` cannot hold integer values")]
    fn an_annotation_cannot_cross_between_kinds() {
        let _ = column!(dict[1, 2]);
    }

    #[test]
    #[should_panic(expected = "column \"id\": 300 does not fit u8")]
    fn a_value_too_wide_for_its_annotation_names_the_column() {
        let _ = table! { "id" => u8[300] };
    }

    #[test]
    #[should_panic(expected = "does not fit u8")]
    fn the_same_failure_through_a_column_has_no_column_to_name() {
        let _ = column!(u8[300]);
    }
}
