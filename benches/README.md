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

`identical` and `renamed_distinct` are the non-adversarial scenarios; the other four are the adversaries the budgets exist to cut.

## The acceptance rule for the default budgets

The rule is measured rather than felt, and it is written in ratios against the same machine's own runs so it does not depend on the hardware: with the default budgets, at every grid point, each bounded stage must complete within twice the same-sized `identical` run — read the stage's cost as the scenario's time minus `identical`'s at that size — and the non-adversarial scenarios must report nothing in `Diff::incomplete`. When a constant changes, re-run the grid (or at least the previously binding points) and check both halves.

Two readings to keep straight when a ratio looks bad. First, a fixed budget binds hardest at the grid's small end, where a pair examination — up to a sample's worth of rows — can rival the whole linear pass; that is why the pair budgets sit at 2048, one halving below the 4096-row sample, after the 20 000 starting points failed the rule at 1000×100 (2026-08-04). Second, `full_rewrite`'s overage is not the bounded stage: the capped summary fallback is a trivial linear pass, and the extra time is assembling the complete cell-level diff, which is a retained design invariant rather than a search a budget could cut.

## Baseline (2026-08-04, Apple Silicon, tuned defaults)

Ratios of scenario time to `identical` at the same size, from the tuning run for the bounded-reconciliation step's default budgets; `identical` absolute times on the second row. Points re-measured after the pair budgets moved to 2048 are marked †; the untouched points are from the 20 000-unit run and only overstate today's ratios.

| | 1k×10 | 1k×100 | 1k×1000 | 100k×10 | 100k×100 | 1M×10 |
|---|---|---|---|---|---|---|
| `identical` | 2.5 ms | 20 ms | 195 ms | 258 ms | 1.95 s | 3.59 s |
| `renamed_distinct` | 1.03× | 1.01× | 1.01× | 1.06× | 1.15× | 1.12× |
| `renamed_constant` | 0.81× | 0.89×† | 0.74× | 0.94× | 2.11× | 0.76× |
| `rename_and_modify` | 1.48× | 2.2׆ | 2.26× | 1.14× | 3.53× | 1.04× |
| `swapped` | 1.25× | 2.6׆ | 2.93× | 1.07× | 2.28× | 1.01× |
| `full_rewrite` | 1.42× | 4.50× | 2.86× | 1.40× | 2.66× | 1.31× |

## Profiling a point

When a number needs explaining rather than comparing, sample a looped run. Write a throwaway example that repeats the interesting `diff_tables` call, then:

```console
cargo build --release --example <name>
./target/release/examples/<name> & sample $! 10 -file profile.txt
```

The "Sort by top of stack" section at the end of `profile.txt` is usually enough. The 2026-08-04 profile of `identical/1000000x10` is recorded in `plan-next.md`'s constant-factor item: roughly 40% allocator traffic and 30% eager projection work, with the necessary linear passes a small minority.
