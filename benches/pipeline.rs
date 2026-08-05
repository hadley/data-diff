//! Benchmarks of the complete pipeline over the scenarios the budgets are
//! tuned against.
//!
//! The grid is rows {1 000, 100 000, 1 000 000} by columns {10, 100, 1 000},
//! skipping combinations above 10⁷ cells. `identical` is the floor — one
//! linear pass of cell comparison with nothing to infer — and the acceptance
//! rule for the default budgets is measured against it: on every grid point,
//! each adversarial scenario completes within twice the same-sized `identical`
//! run, and no non-adversarial scenario reports an incomplete stage. The
//! multiplier is a ratio against the same run's own linear pass, so the rule
//! does not depend on the machine.

use arrow_array::RecordBatch;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use data_diff::{DiffOptions, diff_tables};
use test_support::generate;

type Scenario = fn(usize, usize) -> (RecordBatch, RecordBatch);

const GRID: [(usize, usize); 6] = [
    (1_000, 10),
    (1_000, 100),
    (1_000, 1_000),
    (100_000, 10),
    (100_000, 100),
    (1_000_000, 10),
];

const SCENARIOS: [(&str, Scenario); 6] = [
    ("identical", generate::identical),
    ("renamed_distinct", generate::renamed_distinct),
    ("renamed_constant", generate::renamed_constant),
    ("rename_and_modify", generate::rename_and_modify),
    ("swapped", generate::swapped),
    ("full_rewrite", generate::full_rewrite),
];

fn pipeline(criterion: &mut Criterion) {
    let options = DiffOptions {
        key: vec!["id".into()],
        ..DiffOptions::default()
    };
    for (name, scenario) in SCENARIOS {
        let mut group = criterion.benchmark_group(name);
        group.sample_size(10);
        for (rows, columns) in GRID {
            let (old, new) = scenario(rows, columns);
            group.bench_with_input(
                BenchmarkId::from_parameter(format!("{rows}x{columns}")),
                &(old, new),
                |bencher, (old, new)| {
                    bencher.iter(|| diff_tables(old, new, &options).unwrap());
                },
            );
        }
        group.finish();
    }
}

criterion_group!(benches, pipeline);
criterion_main!(benches);
