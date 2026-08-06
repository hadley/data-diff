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

The search budgets are row-denominated and proportional (2026-08-06): `rename_rows` and `swap_rows` default to a fixed number of row examinations per cell of the compared table, so "each bounded stage does at most that multiple of the work of reading the table" holds by construction, at every size and shape, on every machine. What the grid confirms is the two halves construction cannot: with the default budgets, at every grid point, the non-adversarial scenarios must report nothing in `Diff::incomplete`, and no scenario's time may exceed its multiplier of the same-sized `identical` run in the current baseline table — the recorded multipliers, not a universal constant, being the enforceable wall-clock half, with the all-cells-change overage attributed to retained cell assembly as below. When a constant changes, re-run the grid, re-verify both halves, and re-record the table.

The default multiples — 20 for renames, 5 for swaps — were tuned under one further measured criterion: no grid point loses a completion the previous fixed 2048-pair budgets funded, verified by running both builds over the full grid and comparing `Diff::incomplete` point by point. The multiples sit just above the ten-column adversaries' analytic needs (~200 full-row rename examinations across eleven columns is ~18.2 rows per cell; a fully swapped ten-column table's crossing enumeration is ~4.5), and the comparison came out one-sided: every point matches, and 100k×100 completes three stages the pair budgets cut short — `rename_and_modify`'s renames, `swapped`'s enumeration, and `full_rewrite`'s swap check. Funding the swap enumeration there also made the run 30% faster, resolved swaps leaving no changed cells to assemble; funding `rename_and_modify`'s inference costs real time (its 16× below), which is the price of the completed answer rather than overhead.

Two readings to keep straight when a ratio looks bad. First, `full_rewrite`'s overage — and every all-cells-change scenario's — is largely not the bounded stage: the capped summary fallback is a trivial linear pass, and the extra time is assembling the complete cell-level diff, which is a retained design invariant rather than a search a budget could cut. Second, `renamed_distinct`'s rise past 100k rows is the exact stage's full-column digest join — the unbudgeted linear pass the design accepts — visible against a floor that no longer buries it; it reports nothing incomplete at any grid point, as the rule requires of a non-adversarial scenario.

## Baseline (2026-08-06, Apple Silicon, row-denominated defaults)

Ratios of scenario time to `identical` at the same size; `identical` absolute times on the first row. These are the recorded multipliers the acceptance rule enforces.

| | 1k×10 | 1k×100 | 1k×1000 | 100k×10 | 100k×100 | 1M×10 |
|---|---|---|---|---|---|---|
| `identical` | 0.77 ms | 7.0 ms | 74 ms | 29 ms | 138 ms | 407 ms |
| `renamed_distinct` | 1.26× | 1.27× | 1.28× | 3.49× | 7.76× | 3.84× |
| `renamed_constant` | 1.16× | 1.13× | 1.11× | 3.22× | 5.84× | 2.53× |
| `rename_and_modify` | 1.95× | 2.74× | 2.85× | 2.78× | 16.12× | 1.95× |
| `swapped` | 1.33× | 2.29× | 2.43× | 1.25× | 7.12× | 1.02× |
| `full_rewrite` | 1.98× | 2.22× | 2.51× | 4.72× | 13.12× | 3.39× |

## Prior baseline (2026-08-05, Apple Silicon, fixed pair budgets, after the constant-factor step)

The constant-factor step's run under the 2048-pair budgets, kept for comparison. Its third reading — pair-denominated budgets drifting from the row-scaled bar the leaner floor exposed — is resolved by the row-denominated scheme above.

| | 1k×10 | 1k×100 | 1k×1000 | 100k×10 | 100k×100 | 1M×10 |
|---|---|---|---|---|---|---|
| `identical` | 0.85 ms | 7.6 ms | 76 ms | 29 ms | 138 ms | 437 ms |
| `renamed_distinct` | 1.14× | 1.19× | 1.24× | 3.52× | 7.91× | 3.44× |
| `renamed_constant` | 1.02× | 1.03× | 0.67× | 3.30× | 5.80× | 2.30× |
| `rename_and_modify` | 1.78× | 2.67× | 1.26× | 2.92× | 5.74× | 1.84× |
| `swapped` | 1.23× | 3.41× | 2.16× | 1.33× | 9.69× | 0.97× |
| `full_rewrite` | 1.87× | 3.31× | 2.19× | 4.36× | 9.78× | 3.19× |

## Prior baseline (2026-08-04, Apple Silicon, tuned defaults)

The tuning run for the bounded-reconciliation step's default budgets, kept for comparison; every 2026-08-05 point is absolutely faster. Points re-measured after the pair budgets moved to 2048 are marked †; the untouched points are from the 20 000-unit run and only overstate that day's ratios.

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

The "Sort by top of stack" section at the end of `profile.txt` is usually enough. The 2026-08-05 profiles that drove the constant-factor step are recorded in that step's `plan.md`: before it, roughly 57% of an identical million-row run was eager projection work behind sampled questions and ~25% was SipHash, with the necessary linear passes a small minority; after it, the top of the profile is those linear passes — key indexing, canonicalization, cell comparison, and the ordering LCS.
