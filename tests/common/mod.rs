#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use arrow_array::RecordBatch;
use parquet::arrow::ArrowWriter;

static COUNTER: AtomicU32 = AtomicU32::new(0);

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

pub fn write_parquet(path: &Path, batch: &RecordBatch) {
    write_parquet_batches(path, std::slice::from_ref(batch));
}

pub fn write_parquet_batches(path: &Path, batches: &[RecordBatch]) {
    assert!(!batches.is_empty());
    let file = std::fs::File::create(path).unwrap();
    let mut writer = ArrowWriter::try_new(file, batches[0].schema(), None).unwrap();
    for batch in batches {
        writer.write(batch).unwrap();
    }
    writer.close().unwrap();
}
