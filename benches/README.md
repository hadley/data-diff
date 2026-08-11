# Pipeline benchmarks

`pipeline.rs` benchmarks `diff_tables` end to end over synthetic table pairs, and exists for one recurring job: choosing and defending the default `Budgets` constants. Run it with `cargo bench --bench pipeline`; a single point or scenario can be filtered by its criterion id, for example `cargo bench --bench pipeline -- 'rename_and_modify/1000x100$'`, and `cargo bench --bench pipeline -- --test` smoke-runs every point once without measuring.

## The grid and the scenarios

Every scenario runs over rows {1 000, 100 000, 1 000 000} by columns {10, 100, 1 000}, skipping combinations above 10⁷ cells. The generators live in `test-support/src/generate.rs`; every value is a pure function of its coordinates, so two runs generate byte-identical tables and the benchmark measures the code rather than the fixture. Most pairs carry an `id` column holding the row index, which the harness declares as the key so reconciliation is measured rather than key guessing. The two `guessed_*`/`keyless_*` scenarios omit it and run with no declared key, because they exist to measure the key search itself.

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
| `guessed_compound` | no declared key; a hidden (g1, g2) pair behind near-perfect payload columns | the honest keyless case: the compound search must find the pair and outrank every one-row-short single column |
| `keyless_duplicates` | no declared key; low-cardinality columns with no key at any width | the key-search adversary: the lattice must exhaust into the positional fallback under its budgets |

`identical`, `identical_strings`, `renamed_distinct`, `renamed_strings`, and `guessed_compound` are the non-adversarial scenarios; the others are the adversaries the budgets exist to cut. `keyless_duplicates` is special among the adversaries: exhausting is its expected result, so the acceptance check on it is that `Diff::incomplete` holds exactly the key guess, and that the fallback diff over its identical sides stays empty.

## The acceptance rule for the default budgets

The search budgets are row-denominated and proportional: `rename_rows` and `swap_rows` default to a fixed number of row examinations per cell of the compared table, and `key_rows` to a fixed number per cell of the raw input with a documented floor, so "each bounded stage does at most that multiple of the work of reading the table" holds by construction, at every size and shape, on every machine. Key guessing adds two absolute caps, `key_candidates` and `key_width`, whose rationale lives in `design.md`'s computation-budgets and guessed-key sections. What the grid confirms is the two halves construction cannot:

- with the default budgets, at every grid point, the non-adversarial scenarios report nothing in `Diff::incomplete` (and `keyless_duplicates` reports exactly its expected key-guess exhaustion); and
- no scenario's time exceeds its multiplier of the same-sized `identical` run recorded in the baseline table below.

When a constant changes, re-run the grid, re-verify both halves, and re-record the table — under one further criterion: no grid point may lose a completion the previous defaults funded, verified by running both builds over the grid and comparing `Diff::incomplete` point by point. The current multiples, their analytic floors, and the reasoning behind them are recorded in `design.md`'s computation-budgets section, which is where a re-tuning argument belongs.

## Reading a ratio

Three principles keep a multiplier honest. First, multipliers are floor-relative: an optimization that shrinks the `identical` floor inflates every other scenario's ratio while their absolute times fall or hold, so cross-build comparisons must be made in absolute times, never by comparing multipliers across baselines. Second, the all-cells-change scenarios' overage is largely not the bounded search: the capped summary fallback is a trivial linear pass, and the bulk is assembling the complete cell-level diff, a retained design invariant no budget may cut. Third, the `renamed_*` scenarios' rise past 100k rows is the exact stage's full-column work — the unbudgeted linear pass the design accepts — and their `Diff::incomplete` staying empty is the claim to check, not their ratio.

The keyless scenarios read the same way. Their floor-relative ratios are the highest in the table because a run with no declared key pays for what every declared run skips: one canonical projection, digest, and duplicate index per column per side before the search starts, and then the budgeted search itself. That preparation is the unbudgeted linear pass the design records; the search above it burns at most its counted budgets, which is the bound being demonstrated, so the claim to check is `guessed_compound` completing with the right key and `keyless_duplicates` exhausting into exactly the key-guess report.

## Baseline (2026-08-07, Apple Silicon)

Ratios of scenario time to `identical` at the same size; `identical` absolute times on the first row. These are the recorded multipliers the acceptance rule enforces. Prior baselines live in this file's git history; they are context, not a comparison method — see the next section for how a change is actually verified.

| | 1k×10 | 1k×100 | 1k×1000 | 100k×10 | 100k×100 | 1M×10 |
|---|---|---|---|---|---|---|
| `identical` | 0.33 ms | 2.6 ms | 25 ms | 12 ms | 50 ms | 184 ms |
| `identical_strings` | 2.02× | 2.43× | 2.59× | 1.44× | 2.13× | 1.11× |
| `renamed_distinct` | 1.58× | 1.79× | 1.93× | 4.19× | 8.96× | 3.85× |
| `renamed_strings` | 3.01× | 3.68× | 3.92× | 7.93× | 18.30× | 8.55× |
| `renamed_constant` | 2.65× | 2.56× | 2.66× | 7.00× | 13.27× | 5.09× |
| `rename_and_modify` | 4.14× | 7.05× | 8.20× | 3.06× | 10.81× | 2.33× |
| `swapped` | 2.93× | 4.11× | 4.67× | 1.10× | 3.89× | 1.10× |
| `full_rewrite` | 4.22× | 4.45× | 5.00× | 7.00× | 16.24× | 5.88× |
| `guessed_compound` | 2.69× | 2.87× | 3.10× | 6.81× | 13.77× | 6.63× |
| `keyless_duplicates` | 2.81× | 2.78× | 5.32× | 6.03× | 13.69× | 4.77× |

The 2026-08-07 re-record accompanies the compound-key step: the declared-key scenarios' absolute times are within noise of the 2026-08-06 baseline and their outputs were verified byte-identical against it, so their ratio drift is measurement noise, not change. The keyless rows are new, and they sit inside the range the declared adversaries already occupy: an earlier draft of the step measured `guessed_compound` at up to 29× before profiling moved its bucketing onto `KeyIndex`'s flat layout and reconsideration stopped re-running provably redundant guesses. Their verification is behavioral as well as temporal: at every grid point, `guessed_compound` completes with the hidden two-column key and an empty `Diff::incomplete`, and `keyless_duplicates` reports exactly `incomplete_key_guess()` over an otherwise empty diff.

## Verifying a change against the committed baseline

Performance work on this pipeline gates on output identity, not on the numbers above: unless a step deliberately changes semantics with the owner's sign-off, its output must be byte-identical to the committed baseline's. The procedure that has carried every step so far: build the baseline commit in a `git worktree` (copying in any new generators), write a throwaway example that runs one scenario and prints an xxh3 digest of the complete `Diff`'s debug form, and compare the two builds' digests over every scenario at sizes that exercise both sampling (more than 4096 matched rows) and budget exhaustion. Any divergence is a bug in the change, not a tolerance to record.

## Profiling a point

When a number needs explaining rather than comparing, sample a looped run. Write a throwaway example that repeats the interesting `diff_tables` call, then:

```console
cargo build --release --example <name>
./target/release/examples/<name> & sample $! 10 -file profile.txt
```

The "Sort by top of stack" section at the end of `profile.txt` is usually enough, and the call-graph section attributes what top-of-stack cannot. Re-profile before optimizing: the profile reshapes every time the totals shrink, and more than one queued lead has been overturned by the re-measurement that was supposed to confirm it.
