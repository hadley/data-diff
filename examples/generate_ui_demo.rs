//! Generate the UI demo pair: `ui/demo-old.parquet` and
//! `ui/demo-new.parquet`, 10,000 rows of inventory with a realistic spread
//! of changes — one edited column, a 50×5 rectangle of edits, an added and
//! a dropped column, 50 added rows, 20 dropped rows. Deterministic: the
//! values come from a fixed-seed xorshift, so regenerating is byte-identical.

use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

use arrow_array::RecordBatch;
use arrow_array::array::{Float64Array, Int64Array, StringArray};
use arrow_schema::{Field, Schema};
use parquet::arrow::ArrowWriter;

const ROWS: usize = 10_000;
/// Dropped from the new file: ids 5001..=5020.
const DROPPED: std::ops::RangeInclusive<i64> = 5001..=5020;
/// Added in the new file: ids 10001..=10050.
const ADDED: std::ops::RangeInclusive<i64> = 10_001..=10_050;
/// The rectangle: rows 2001..=2050, columns r1..=r5.
const RECT_ROWS: std::ops::RangeInclusive<i64> = 2001..=2050;

/// A fixed-seed xorshift, so the demo is byte-identical on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, ceiling: u64) -> u64 {
        self.next() % ceiling
    }
}

fn main() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);

    let mut id = Vec::with_capacity(ROWS);
    let mut name = Vec::with_capacity(ROWS);
    let mut price = Vec::with_capacity(ROWS);
    let mut quantity = Vec::with_capacity(ROWS);
    let mut rating = Vec::with_capacity(ROWS);
    let mut rect: [Vec<i64>; 5] = Default::default();
    let mut legacy = Vec::with_capacity(ROWS);

    for row in 1..=ROWS as i64 {
        id.push(row);
        name.push(format!("item-{row:05}"));
        price.push((rng.below(5000) as f64 + 999.0) / 100.0);
        quantity.push(rng.below(200) as i64);
        rating.push((rng.below(450) as f64 + 50.0) / 100.0);
        for column in &mut rect {
            column.push(rng.below(10_000) as i64);
        }
        legacy.push(format!("L{}", rng.below(90)));
    }

    let old = batch(&[
        ("id", int64(&id)),
        ("name", string(&name)),
        ("price", float64(&price)),
        ("quantity", int64(&quantity)),
        ("rating", float64(&rating)),
        ("r1", int64(&rect[0])),
        ("r2", int64(&rect[1])),
        ("r3", int64(&rect[2])),
        ("r4", int64(&rect[3])),
        ("r5", int64(&rect[4])),
        ("legacy", string(&legacy)),
    ]);

    // The new side: the dropped rows leave, the added rows arrive, `legacy`
    // is dropped and `status` is added, `price` is edited on every twentieth
    // surviving row, and the rectangle's cells are all edited.
    let mut n_id = Vec::new();
    let mut n_name = Vec::new();
    let mut n_price = Vec::new();
    let mut n_quantity = Vec::new();
    let mut n_rating = Vec::new();
    let mut n_rect: [Vec<i64>; 5] = Default::default();
    let mut n_status = Vec::new();

    let mut push = |row: usize, price_scale: f64, in_rectangle: bool| {
        n_id.push(id[row]);
        n_name.push(name[row].clone());
        n_price.push(price[row] * price_scale);
        n_quantity.push(quantity[row]);
        n_rating.push(rating[row]);
        for (column, target) in rect.iter().zip(n_rect.iter_mut()) {
            target.push(column[row] + if in_rectangle { 100_000 } else { 0 });
        }
        n_status.push(if row.is_multiple_of(7) {
            "backordered".to_owned()
        } else {
            "in stock".to_owned()
        });
    };

    for (index, &row) in id.iter().enumerate() {
        if DROPPED.contains(&row) {
            continue;
        }
        push(
            index,
            if row % 20 == 0 { 1.05 } else { 1.0 },
            RECT_ROWS.contains(&row),
        );
    }
    for row in ADDED {
        let index = (row as usize - 1) % ROWS;
        n_id.push(row);
        n_name.push(format!("item-{row:05}"));
        n_price.push(price[index]);
        n_quantity.push(quantity[index]);
        n_rating.push(rating[index]);
        for (column, target) in rect.iter().zip(n_rect.iter_mut()) {
            target.push(column[index]);
        }
        n_status.push("new".to_owned());
    }

    let new = batch(&[
        ("id", int64(&n_id)),
        ("name", string(&n_name)),
        ("price", float64(&n_price)),
        ("quantity", int64(&n_quantity)),
        ("rating", float64(&n_rating)),
        ("r1", int64(&n_rect[0])),
        ("r2", int64(&n_rect[1])),
        ("r3", int64(&n_rect[2])),
        ("r4", int64(&n_rect[3])),
        ("r5", int64(&n_rect[4])),
        ("status", string(&n_status)),
    ]);

    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui");
    write(&output.join("demo-old.parquet"), &old);
    write(&output.join("demo-new.parquet"), &new);
    println!(
        "wrote {} ({} rows) and {} ({} rows)",
        output.join("demo-old.parquet").display(),
        old.num_rows(),
        output.join("demo-new.parquet").display(),
        new.num_rows(),
    );
}

fn int64(values: &[i64]) -> Arc<dyn arrow_array::Array> {
    Arc::new(Int64Array::from(values.to_vec()))
}

fn float64(values: &[f64]) -> Arc<dyn arrow_array::Array> {
    Arc::new(Float64Array::from(values.to_vec()))
}

fn string(values: &[String]) -> Arc<dyn arrow_array::Array> {
    Arc::new(StringArray::from(
        values.iter().map(String::as_str).collect::<Vec<_>>(),
    ))
}

fn batch(columns: &[(&str, Arc<dyn arrow_array::Array>)]) -> RecordBatch {
    let schema = Arc::new(Schema::new(
        columns
            .iter()
            .map(|(name, array)| Field::new(*name, array.data_type().clone(), false))
            .collect::<Vec<_>>(),
    ));
    RecordBatch::try_new(
        schema,
        columns.iter().map(|(_, array)| array.clone()).collect(),
    )
    .unwrap()
}

fn write(path: &PathBuf, batch: &RecordBatch) {
    let mut writer =
        ArrowWriter::try_new(File::create(path).unwrap(), batch.schema(), None).unwrap();
    writer.write(batch).unwrap();
    writer.close().unwrap();
}
