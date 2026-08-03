use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use arrow_array::{Array, RecordBatch, UInt64Array};
use arrow_schema::{DataType, SchemaRef};
use arrow_select::concat::concat_batches;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

use crate::{ColumnSchema, DiffError, DuplicateColumnName, NormalizedType, Schemas, Side};

/// The reserved path that names an absent side of a comparison.
///
/// Reserved rather than looked up, like the key's `#row`, and only as the
/// exact bare argument: a real file with this name is still reachable as
/// `./#missing`, since the token never contains a separator.
pub const MISSING_FILE: &str = "#missing";

/// Read a Parquet file into one logical in-memory table.
pub fn read_parquet(path: &Path) -> Result<RecordBatch, DiffError> {
    let file = File::open(path).map_err(|error| read_error(path, error))?;
    let builder =
        ParquetRecordBatchReaderBuilder::try_new(file).map_err(|error| read_error(path, error))?;
    let schema = builder.schema().clone();
    let reader = builder.build().map_err(|error| read_error(path, error))?;
    let batches = reader
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| read_error(path, error))?;

    concatenate(schema, &batches).map_err(|error| read_error(path, error))
}

fn read_error(path: &Path, error: impl std::fmt::Display) -> DiffError {
    DiffError::ReadParquet {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

fn concatenate(
    schema: SchemaRef,
    batches: &[RecordBatch],
) -> Result<RecordBatch, arrow_schema::ArrowError> {
    if batches.is_empty() {
        Ok(RecordBatch::new_empty(schema))
    } else {
        concat_batches(&schema, batches)
    }
}

/// Validate both in-memory inputs and return their inspectable schemas.
pub fn validate_tables(old: &RecordBatch, new: &RecordBatch) -> Result<Schemas, DiffError> {
    Ok(Schemas {
        old: validate_table(old, Side::Old)?,
        new: validate_table(new, Side::New)?,
    })
}

pub(crate) fn validate_table(
    table: &RecordBatch,
    side: Side,
) -> Result<Vec<ColumnSchema>, DiffError> {
    validate_names(table, side)?;
    table
        .schema()
        .fields()
        .iter()
        .enumerate()
        .map(|(column_index, field)| {
            let data_type = field.data_type();
            let normalized_type =
                normalized_type(data_type).ok_or_else(|| DiffError::UnsupportedColumn {
                    side,
                    column: field.name().clone(),
                    source_type: source_type(data_type),
                })?;
            validate_values(
                table.column(column_index).as_ref(),
                side,
                field.name(),
                data_type,
            )?;

            Ok(ColumnSchema {
                name: field.name().clone(),
                source_type: source_type(data_type),
                normalized_type,
            })
        })
        .collect()
}

fn validate_names(table: &RecordBatch, side: Side) -> Result<(), DiffError> {
    let mut positions = BTreeMap::<String, Vec<usize>>::new();
    for (index, field) in table.schema().fields().iter().enumerate() {
        positions
            .entry(field.name().clone())
            .or_default()
            .push(index + 1);
    }
    let duplicates = positions
        .into_iter()
        .filter(|(_, positions)| positions.len() > 1)
        .map(|(name, positions)| DuplicateColumnName { name, positions })
        .collect::<Vec<_>>();

    if duplicates.is_empty() {
        Ok(())
    } else {
        Err(DiffError::DuplicateColumnNames { side, duplicates })
    }
}

fn normalized_type(data_type: &DataType) -> Option<NormalizedType> {
    match data_type {
        DataType::Boolean => Some(NormalizedType::Boolean),
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => Some(NormalizedType::Int64),
        DataType::Float32 | DataType::Float64 => Some(NormalizedType::Double),
        DataType::Utf8 | DataType::LargeUtf8 => Some(NormalizedType::String),
        DataType::Dictionary(_, value)
            if matches!(value.as_ref(), DataType::Utf8 | DataType::LargeUtf8) =>
        {
            Some(NormalizedType::String)
        }
        _ => None,
    }
}

fn validate_values(
    values: &dyn Array,
    side: Side,
    column: &str,
    data_type: &DataType,
) -> Result<(), DiffError> {
    if !matches!(data_type, DataType::UInt64) {
        return Ok(());
    }
    let values = values
        .as_any()
        .downcast_ref::<UInt64Array>()
        .expect("a UInt64 field has a UInt64 array");
    if let Some((row, _)) = values
        .iter()
        .enumerate()
        .find(|(_, value)| value.is_some_and(|value| value > i64::MAX as u64))
    {
        Err(DiffError::IntegerOutOfRange {
            side,
            column: column.to_owned(),
            source_type: source_type(data_type),
            row: row + 1,
        })
    } else {
        Ok(())
    }
}

fn source_type(data_type: &DataType) -> String {
    format!("{data_type:?}")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_schema::{DataType, Field, Fields, TimeUnit};
    use test_support::table;

    use super::{concatenate, normalized_type, validate_tables};
    use crate::{DiffError, DuplicateColumnName, NormalizedType, Side};

    #[test]
    fn normalizes_every_supported_arrow_representation() {
        let old = table! {
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
            "utf8" => str["a"],
            "large_utf8" => large_str["a"],
            "dictionary" => dict["a"],
        };

        let schemas = validate_tables(&old, &table! {}).unwrap();
        let normalized = schemas
            .old
            .iter()
            .map(|column| column.normalized_type)
            .collect::<Vec<_>>();
        assert_eq!(
            normalized,
            [
                NormalizedType::Boolean,
                NormalizedType::Int64,
                NormalizedType::Int64,
                NormalizedType::Int64,
                NormalizedType::Int64,
                NormalizedType::Int64,
                NormalizedType::Int64,
                NormalizedType::Int64,
                NormalizedType::Int64,
                NormalizedType::Double,
                NormalizedType::Double,
                NormalizedType::String,
                NormalizedType::String,
                NormalizedType::String,
            ]
        );
    }

    #[test]
    fn concatenates_batches_in_input_order() {
        let first = table! { "id" => [1, 2] };
        let second = table! { "id" => [3] };

        let combined = concatenate(first.schema(), &[first, second]).unwrap();

        assert_eq!(combined.column(0), table! { "id" => [1, 2, 3] }.column(0));
    }

    #[test]
    fn reports_all_duplicate_names_and_positions() {
        let old = table! {
            "a" => [1],
            "b" => [1],
            "a" => [1],
            "b" => [1],
        };

        assert_eq!(
            validate_tables(&old, &table! {}),
            Err(DiffError::DuplicateColumnNames {
                side: Side::Old,
                duplicates: vec![
                    DuplicateColumnName {
                        name: "a".into(),
                        positions: vec![1, 3],
                    },
                    DuplicateColumnName {
                        name: "b".into(),
                        positions: vec![2, 4],
                    },
                ],
            })
        );
    }

    #[test]
    fn rejects_unsupported_types() {
        let old = table! { "bytes" => binary["a"] };

        assert_eq!(
            validate_tables(&old, &table! {}),
            Err(DiffError::UnsupportedColumn {
                side: Side::Old,
                column: "bytes".into(),
                source_type: "Binary".into(),
            })
        );
    }

    #[test]
    fn rejects_every_unsupported_type_family() {
        let item = Arc::new(Field::new("item", DataType::Int64, true));
        let unsupported = [
            DataType::Binary,
            DataType::FixedSizeBinary(4),
            DataType::Date32,
            DataType::Timestamp(TimeUnit::Microsecond, None),
            DataType::Duration(TimeUnit::Microsecond),
            DataType::Decimal128(10, 2),
            DataType::List(item.clone()),
            DataType::Struct(Fields::from(vec![item])),
            DataType::Dictionary(Box::new(DataType::Int8), Box::new(DataType::Int64)),
        ];

        for data_type in unsupported {
            assert_eq!(
                normalized_type(&data_type),
                None,
                "{data_type:?} should be unsupported"
            );
        }
    }

    #[test]
    fn rejects_first_out_of_range_unsigned_integer() {
        let old = table! {
            "id" => u64[Some(1), None, Some(i64::MAX as u64 + 1), Some(u64::MAX)]
        };

        assert_eq!(
            validate_tables(&old, &table! {}),
            Err(DiffError::IntegerOutOfRange {
                side: Side::Old,
                column: "id".into(),
                source_type: "UInt64".into(),
                row: 3,
            })
        );
    }
}
