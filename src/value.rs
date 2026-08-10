use arrow_array::{
    Array, BooleanArray, Date32Array, Date64Array, Decimal128Array, Decimal256Array, Float32Array,
    Float64Array, Int8Array, Int16Array, Int32Array, Int64Array, LargeStringArray, StringArray,
    TimestampMicrosecondArray, TimestampMillisecondArray, TimestampNanosecondArray,
    TimestampSecondArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};
use arrow_buffer::i256;
use arrow_cast::cast;
use arrow_row::{RowConverter, SortField};
use arrow_schema::{DataType, TimeUnit};

/// A value read from an input table, in its source type.
///
/// This is an extraction type, not a comparison type: a type-changed column
/// reports each side as that side stored it, so `"9.99"` and `9.99` reach the
/// reader honestly rather than normalized into sameness, and `Null`, a `NaN`
/// double, and the empty string stay three distinct values, as the comparison
/// semantics treat them. It derives `PartialEq` but not `Eq`, a raw `f64`
/// being exactly what a display of the input wants.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Boolean(bool),
    /// Every integer width, signed or unsigned, widened as the comparison
    /// widens it: unsigned values wrap, which validation has already bounded
    /// to `i64::MAX` for `u64`.
    Int64(i64),
    Double(f64),
    String(String),
    /// The stored reading with its unit and timezone, the timezone being
    /// presentation metadata an honest rendering keeps.
    Timestamp {
        value: i64,
        unit: TimeUnit,
        timezone: Option<String>,
    },
    /// Days since the epoch.
    Date32(i32),
    /// Milliseconds since the epoch.
    Date64(i64),
    Decimal128 {
        value: i128,
        precision: u8,
        scale: i8,
    },
    Decimal256 {
        value: i256,
        precision: u8,
        scale: i8,
    },
    /// Canonical row-format bytes of a value outside the comparison matrix;
    /// the same encoding `compare` measures, so equal bytes are equal values.
    Opaque(Vec<u8>),
}

/// Read one zero-based row of a column as a [`Value`].
///
/// Nulls are logical rather than physical: a valid dictionary key pointing at
/// a null value is a null of the column, the same rule the comparison
/// applies, so the null mask is consulted before any dispatch.
pub(crate) fn extract(values: &dyn Array, row: usize) -> Value {
    if values
        .logical_nulls()
        .is_some_and(|nulls| nulls.is_null(row))
    {
        return Value::Null;
    }
    match values.data_type() {
        DataType::Boolean => Value::Boolean(read(values, row, |a: &BooleanArray, r| a.value(r))),
        DataType::Int8 => Value::Int64(read(values, row, |a: &Int8Array, r| i64::from(a.value(r)))),
        DataType::Int16 => {
            Value::Int64(read(values, row, |a: &Int16Array, r| i64::from(a.value(r))))
        }
        DataType::Int32 => {
            Value::Int64(read(values, row, |a: &Int32Array, r| i64::from(a.value(r))))
        }
        DataType::Int64 => Value::Int64(read(values, row, |a: &Int64Array, r| a.value(r))),
        DataType::UInt8 => {
            Value::Int64(read(values, row, |a: &UInt8Array, r| i64::from(a.value(r))))
        }
        DataType::UInt16 => Value::Int64(read(values, row, |a: &UInt16Array, r| {
            i64::from(a.value(r))
        })),
        DataType::UInt32 => Value::Int64(read(values, row, |a: &UInt32Array, r| {
            i64::from(a.value(r))
        })),
        DataType::UInt64 => Value::Int64(read(values, row, |a: &UInt64Array, r| a.value(r) as i64)),
        DataType::Float32 => Value::Double(read(values, row, |a: &Float32Array, r| {
            f64::from(a.value(r))
        })),
        DataType::Float64 => Value::Double(read(values, row, |a: &Float64Array, r| a.value(r))),
        DataType::Utf8 => Value::String(read(values, row, |a: &StringArray, r| {
            a.value(r).to_owned()
        })),
        DataType::LargeUtf8 => Value::String(read(values, row, |a: &LargeStringArray, r| {
            a.value(r).to_owned()
        })),
        DataType::Dictionary(_, value_type)
            if matches!(value_type.as_ref(), DataType::Utf8 | DataType::LargeUtf8) =>
        {
            // Hydration through the cast, the same path the comparison's
            // string canonicalization takes.
            let hydrated = cast(values, &DataType::LargeUtf8).expect("validated string dictionary");
            extract(hydrated.as_ref(), row)
        }
        DataType::Timestamp(unit, timezone) => {
            let value = match unit {
                TimeUnit::Second => read(values, row, |a: &TimestampSecondArray, r| a.value(r)),
                TimeUnit::Millisecond => {
                    read(values, row, |a: &TimestampMillisecondArray, r| a.value(r))
                }
                TimeUnit::Microsecond => {
                    read(values, row, |a: &TimestampMicrosecondArray, r| a.value(r))
                }
                TimeUnit::Nanosecond => {
                    read(values, row, |a: &TimestampNanosecondArray, r| a.value(r))
                }
            };
            Value::Timestamp {
                value,
                unit: *unit,
                timezone: timezone.as_ref().map(|zone| zone.to_string()),
            }
        }
        DataType::Date32 => Value::Date32(read(values, row, |a: &Date32Array, r| a.value(r))),
        DataType::Date64 => Value::Date64(read(values, row, |a: &Date64Array, r| a.value(r))),
        DataType::Decimal128(precision, scale) => Value::Decimal128 {
            value: read(values, row, |a: &Decimal128Array, r| a.value(r)),
            precision: *precision,
            scale: *scale,
        },
        DataType::Decimal256(precision, scale) => Value::Decimal256 {
            value: read(values, row, |a: &Decimal256Array, r| a.value(r)),
            precision: *precision,
            scale: *scale,
        },
        _ => opaque(values, row),
    }
}

fn read<A: Array + 'static, T>(
    values: &dyn Array,
    row: usize,
    value: impl Fn(&A, usize) -> T,
) -> T {
    value(
        values
            .as_any()
            .downcast_ref::<A>()
            .expect("dispatched on the data type"),
        row,
    )
}

/// Encode one row of a column outside the matrix as canonical row-format
/// bytes. The slice keeps the conversion to the one row; the encoding is a
/// per-value function, so the bytes match a whole-column conversion's.
fn opaque(values: &dyn Array, row: usize) -> Value {
    let converter = RowConverter::new(vec![SortField::new(values.data_type().clone())])
        .expect("validate_tables admits only encodable types");
    let rows = converter
        .convert_columns(&[values.slice(row, 1)])
        .expect("the converter was built for this column's own type");
    Value::Opaque(rows.row(0).as_ref().to_vec())
}

#[cfg(test)]
mod tests {
    use arrow_array::types::Int8Type;
    use arrow_array::{ArrayRef, DictionaryArray, Int8Array, Int64Array};
    use arrow_schema::TimeUnit;
    use test_support::column;

    use super::{Value, extract};

    fn value_of(array: &ArrayRef, row: usize) -> Value {
        extract(array.as_ref(), row)
    }

    #[test]
    fn null_nan_and_empty_string_are_three_distinct_values() {
        let doubles = column!([Some(0.0), None, Some(f64::NAN)]);
        assert_eq!(value_of(&doubles, 1), Value::Null);
        assert!(matches!(value_of(&doubles, 2), Value::Double(v) if v.is_nan()));
        assert_ne!(value_of(&doubles, 1), value_of(&doubles, 2));

        let strings = column!([Some(""), None]);
        assert_eq!(value_of(&strings, 0), Value::String(String::new()));
        assert_eq!(value_of(&strings, 1), Value::Null);
        assert_ne!(value_of(&strings, 0), value_of(&strings, 1));
    }

    #[test]
    fn extracts_every_domain_in_its_source_type() {
        assert_eq!(value_of(&column!(bool[true]), 0), Value::Boolean(true));
        assert_eq!(value_of(&column!(i8[-1]), 0), Value::Int64(-1));
        assert_eq!(value_of(&column!(u32[7]), 0), Value::Int64(7));
        assert_eq!(value_of(&column!(f32[1.5]), 0), Value::Double(1.5));
        assert_eq!(
            value_of(&column!(large_str["a"]), 0),
            Value::String("a".into())
        );
        assert_eq!(value_of(&column!(dict["a"]), 0), Value::String("a".into()));
        assert_eq!(
            value_of(&column!(ts_ms[1000]), 0),
            Value::Timestamp {
                value: 1000,
                unit: TimeUnit::Millisecond,
                timezone: Some("UTC".into()),
            }
        );
        assert_eq!(
            value_of(&column!(ts_us_naive[1_000_000]), 0),
            Value::Timestamp {
                value: 1_000_000,
                unit: TimeUnit::Microsecond,
                timezone: None,
            }
        );
        assert_eq!(value_of(&column!(date32[1]), 0), Value::Date32(1));
        assert_eq!(
            value_of(&column!(date64[86_400_000]), 0),
            Value::Date64(86_400_000)
        );
        assert_eq!(
            value_of(&column!(dec[150]), 0),
            Value::Decimal128 {
                value: 150,
                precision: 10,
                scale: 2,
            }
        );
        assert!(matches!(
            value_of(&column!(binary["a"]), 0),
            Value::Opaque(_)
        ));
    }

    /// A type-changed column reports each side as that side stored it, which
    /// is the honesty the cell view renders `"9.99"` against `9.99` with.
    #[test]
    fn a_type_changed_column_reads_each_side_in_its_own_type() {
        let old = column!(["9.99"]);
        let new = column!([9.99]);
        assert_eq!(value_of(&old, 0), Value::String("9.99".into()));
        assert_eq!(value_of(&new, 0), Value::Double(9.99));
    }

    /// The null rule is the comparison's: a valid dictionary key pointing at
    /// a null value is a null of the column, not an opaque string.
    #[test]
    fn a_valid_key_pointing_at_a_null_dictionary_value_is_null() {
        let hidden = DictionaryArray::<Int8Type>::try_new(
            Int8Array::from(vec![0, 1]),
            std::sync::Arc::new(Int64Array::from(vec![Some(10), None])),
        )
        .unwrap();
        let array: ArrayRef = std::sync::Arc::new(hidden);
        // An integer dictionary is outside the string fast path, so the
        // opaque arm reads it — and the logical null still wins.
        assert!(matches!(value_of(&array, 0), Value::Opaque(_)));
        assert_eq!(value_of(&array, 1), Value::Null);
    }

    #[test]
    fn opaque_values_encode_equal_values_equally_however_interned() {
        let old = column!(binary["a"]);
        let new = column!(binary["a"]);
        assert_eq!(value_of(&old, 0), value_of(&new, 0));
        let other = column!(binary["b"]);
        assert_ne!(value_of(&old, 0), value_of(&other, 0));
    }
}
