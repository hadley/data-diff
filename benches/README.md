# Pipeline benchmarks

`pipeline.rs` benchmarks `diff_tables` end to end over synthetic table pairs, and exists for one recurring job: choosing and defending the default `Budgets` constants. Run it with `cargo bench --bench pipeline`; a single point or scenario can be filtered by its criterion id, for example `cargo bench --bench pipeline -- 'rename_and_modify/1000x100$'`, and `cargo bench --bench pipeline -- --test` smoke-runs every point once without measuring.

## The grid and the scenarios

Every scenario runs over rows {1 000, 100 000, 1 000 000} by columns {10, 100, 1 000}, skipping combinations above 10⁷ cells. The generators live in `test-support/src/generate.rs`; every value is a pure function of its coordinates, so two runs generate byte-identical tables and the benchmark measures the code rather than the fixture. Each pair carries an `id` column holding the row index, which the harness declares as the key so reconciliation is measured rather than key guessing.

| Scenario | Shape | What it stresses |
|---|---|---|
| `identical` | the same table twice | the floor: one linear pass with nothing to infer |
| `renamed_distinct` | every column renamed in place, distinct values | the positional pre-pass's O(columns) case |
| `renamed_constant` | every column renamed in place, one shared constant | the digest-collision adversary: every candidate digests alike, only budgeted verification separates them |
| `rename_and_modify` | k drops against k adds, each pair ~5% edited | the quadratic approximate stage: no pair exact, every pair measured |
| `swapped` | adjacent same-named column pairs exchanged | the swap adversary: every identity rewritten, every crossing measured |
| `full_rewrite` | every non-key value changed | the summarization adversary and the cost of the complete cell diff |
| `identical_strings` | the same all-string table twice | the string floor: values that clone per value wherever the pipeline materializes |
| `renamed_strings` | every string column renamed in place, distinct values | the digest join and rename verification over string columns |

`identical`, `identical_strings`, `renamed_distinct`, and `renamed_strings` are the non-adversarial scenarios; the other four are the adversaries the budgets exist to cut.

## The acceptance rule for the default budgets

The search budgets are row-denominated and proportional: `rename_rows` and `swap_rows` default to a fixed number of row examinations per cell of the compared table, so "each bounded stage does at most that multiple of the work of reading the table" holds by construction, at every size and shape, on every machine. What the grid confirms is the two halves construction cannot:

- with the default budgets, at every grid point, the non-adversarial scenarios report nothing in `Diff::incomplete`; and
- no scenario's time exceeds its multiplier of the same-sized `identical` run recorded in the baseline table below.

When a constant changes, re-run the grid, re-verify both halves, and re-record the table — under one further criterion: no grid point may lose a completion the previous defaults funded, verified by running both builds over the grid and comparing `Diff::incomplete` point by point. The current multiples, their analytic floors, and the reasoning behind them are recorded in `design.md`'s computation-budgets section, which is where a re-tuning argument belongs.

## Reading a ratio

Three principles keep a multiplier honest. First, multipliers are floor-relative: an optimization that shrinks the `identical` floor inflates every other scenario's ratio while their absolute times fall or hold, so cross-build comparisons must be made in absolute times, never by comparing multipliers across baselines. Second, the all-cells-change scenarios' overage is largely not the bounded search: the capped summary fallback is a trivial linear pass, and the bulk is assembling the complete cell-level diff, a retained design invariant no budget may cut. Third, the `renamed_*` scenarios' rise past 100k rows is the exact stage's full-column work — the unbudgeted linear pass the design accepts — and their `Diff::incomplete` staying empty is the claim to check, not their ratio.

## Baseline (2026-08-06, Apple Silicon)

Ratios of scenario time to `identical` at the same size; `identical` absolute times on the first row. These are the recorded multipliers the acceptance rule enforces. Prior baselines live in this file's git history; they are context, not a comparison method — see the next section for how a change is actually verified.

| | 1k×10 | 1k×100 | 1k×1000 | 100k×10 | 100k×100 | 1M×10 |
|---|---|---|---|---|---|---|
| `identical` | 0.32 ms | 2.6 ms | 25 ms | 14 ms | 52 ms | 209 ms |
| `identical_strings` | 2.13× | 2.53× | 2.64× | 1.38× | 2.09× | 1.15× |
| `renamed_distinct` | 1.67× | 2.02× | 1.97× | 3.95× | 9.05× | 3.58× |
| `renamed_strings` | 3.13× | 3.73× | 3.90× | 7.32× | 17.58× | 7.43× |
| `renamed_constant` | 2.83× | 2.63× | 2.70× | 6.37× | 12.46× | 4.55× |
| `rename_and_modify` | 4.33× | 7.24× | 8.23× | 2.86× | 10.42× | 2.11× |
| `swapped` | 3.10× | 4.25× | 4.70× | 1.04× | 3.51× | 0.97× |
| `full_rewrite` | 4.17× | 4.31× | 4.73× | 5.89× | 14.88× | 4.95× |

## Verifying a change against the committed baseline

Performance work on this pipeline gates on output identity, not on the numbers above: unless a step deliberately changes semantics with the owner's sign-off, its output must be byte-identical to the committed baseline's. The procedure that has carried every step so far: build the baseline commit in a `git worktree` (copying in any new generators), write a throwaway example that runs one scenario and prints an xxh3 digest of the complete `Diff`'s debug form, and compare the two builds' digests over every scenario at sizes that exercise both sampling (more than 4096 matched rows) and budget exhaustion. Any divergence is a bug in the change, not a tolerance to record.

## Profiling a point

When a number needs explaining rather than comparing, sample a looped run. Write a throwaway example that repeats the interesting `diff_tables` call, then:

```console
cargo build --release --example <name>
./target/release/examples/<name> & sample $! 10 -file profile.txt
```

The "Sort by top of stack" section at the end of `profile.txt` is usually enough, and the call-graph section attributes what top-of-stack cannot. Re-profile before optimizing: the profile reshapes every time the totals shrink, and more than one queued lead has been overturned by the re-measurement that was supposed to confirm it.
