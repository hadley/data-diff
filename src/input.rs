use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use arrow_array::{Array, RecordBatch, UInt64Array};
use arrow_schema::{DataType, SchemaRef};
use arrow_select::concat::concat_batches;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

use crate::{ColumnSchema, DiffError, DuplicateColumnName, NormalizedType, Schemas, Side};

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

fn validate_table(table: &RecordBatch, side: Side) -> Result<Vec<ColumnSchema>, DiffError> {
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

    use arrow_array::types::Int8Type;
    use arrow_array::{
        ArrayRef, BinaryArray, BooleanArray, DictionaryArray, Float32Array, Float64Array,
        Int8Array, Int16Array, Int32Array, Int64Array, LargeStringArray, StringArray, UInt8Array,
        UInt16Array, UInt32Array, UInt64Array,
    };
    use arrow_schema::{DataType, Field, Fields, Schema, TimeUnit};

    use super::{concatenate, normalized_type, validate_tables};
    use crate::{DiffError, DuplicateColumnName, NormalizedType, Side};

    fn table(columns: Vec<(&str, ArrayRef)>) -> arrow_array::RecordBatch {
        let fields = columns
            .iter()
            .map(|(name, values)| Field::new(*name, values.data_type().clone(), true))
            .collect::<Vec<_>>();
        let arrays = columns.into_iter().map(|(_, values)| values).collect();
        arrow_array::RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).unwrap()
    }

    fn empty() -> arrow_array::RecordBatch {
        arrow_array::RecordBatch::new_empty(Arc::new(Schema::empty()))
    }

    #[test]
    fn normalizes_every_supported_arrow_representation() {
        let dictionary = DictionaryArray::<Int8Type>::try_new(
            Int8Array::from(vec![0]),
            Arc::new(StringArray::from(vec!["a", "b"])),
        )
        .unwrap();
        let old = table(vec![
            ("bool", Arc::new(BooleanArray::from(vec![true]))),
            ("i8", Arc::new(Int8Array::from(vec![1]))),
            ("i16", Arc::new(Int16Array::from(vec![1]))),
            ("i32", Arc::new(Int32Array::from(vec![1]))),
            ("i64", Arc::new(Int64Array::from(vec![1]))),
            ("u8", Arc::new(UInt8Array::from(vec![1]))),
            ("u16", Arc::new(UInt16Array::from(vec![1]))),
            ("u32", Arc::new(UInt32Array::from(vec![1]))),
            ("u64", Arc::new(UInt64Array::from(vec![1]))),
            ("f32", Arc::new(Float32Array::from(vec![1.0]))),
            ("f64", Arc::new(Float64Array::from(vec![1.0]))),
            ("utf8", Arc::new(StringArray::from(vec!["a"]))),
            ("large_utf8", Arc::new(LargeStringArray::from(vec!["a"]))),
            ("dictionary", Arc::new(dictionary)),
        ]);

        let schemas = validate_tables(&old, &empty()).unwrap();
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
        let first = table(vec![("id", Arc::new(Int64Array::from(vec![1, 2])))]);
        let second = table(vec![("id", Arc::new(Int64Array::from(vec![3])))]);

        let combined = concatenate(first.schema(), &[first, second]).unwrap();
        let ids = combined
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();

        assert_eq!(ids.values(), &[1, 2, 3]);
    }

    #[test]
    fn reports_all_duplicate_names_and_positions() {
        let old = table(vec![
            ("a", Arc::new(Int64Array::from(vec![1]))),
            ("b", Arc::new(Int64Array::from(vec![1]))),
            ("a", Arc::new(Int64Array::from(vec![1]))),
            ("b", Arc::new(Int64Array::from(vec![1]))),
        ]);

        assert_eq!(
            validate_tables(&old, &empty()),
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
        let old = table(vec![(
            "bytes",
            Arc::new(BinaryArray::from(vec![b"a".as_slice()])),
        )]);

        assert_eq!(
            validate_tables(&old, &empty()),
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
        let old = table(vec![(
            "id",
            Arc::new(UInt64Array::from(vec![
                Some(1),
                None,
                Some(i64::MAX as u64 + 1),
                Some(u64::MAX),
            ])),
        )]);

        assert_eq!(
            validate_tables(&old, &empty()),
            Err(DiffError::IntegerOutOfRange {
                side: Side::Old,
                column: "id".into(),
                source_type: "UInt64".into(),
                row: 3,
            })
        );
    }
}
