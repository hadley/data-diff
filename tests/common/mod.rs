#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{Field, Schema};
use parquet::arrow::ArrowWriter;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A schema-preserving table with no rows or columns.
pub fn empty_batch() -> RecordBatch {
    RecordBatch::new_empty(Arc::new(Schema::empty()))
}

/// Build a small table while keeping each test's columns visible inline.
pub fn batch<const N: usize>(columns: [(&str, ArrayRef); N]) -> RecordBatch {
    let fields = columns
        .iter()
        .map(|(name, values)| Field::new(*name, values.data_type().clone(), true))
        .collect::<Vec<_>>();
    let arrays = columns.into_iter().map(|(_, values)| values).collect();
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).unwrap()
}

/// A unique temporary directory removed when the test finishes.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "data-diff-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

/// Write one batch as a Parquet fixture.
pub fn write_parquet(path: &Path, batch: &RecordBatch) {
    let file = std::fs::File::create(path).unwrap();
    let mut writer = ArrowWriter::try_new(file, batch.schema(), None).unwrap();
    writer.write(batch).unwrap();
    writer.close().unwrap();
}
