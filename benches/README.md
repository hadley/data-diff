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
| `identical_strings` | the same all-string table twice | the string floor: cell comparison over values that clone per value when materialized |
| `renamed_strings` | every string column renamed in place, distinct values | the digest join and rename verification over string columns |

`identical`, `identical_strings`, `renamed_distinct`, and `renamed_strings` are the non-adversarial scenarios; the other four are the adversaries the budgets exist to cut.

## The acceptance rule for the default budgets

The search budgets are row-denominated and proportional (2026-08-06): `rename_rows` and `swap_rows` default to a fixed number of row examinations per cell of the compared table, so "each bounded stage does at most that multiple of the work of reading the table" holds by construction, at every size and shape, on every machine. What the grid confirms is the two halves construction cannot: with the default budgets, at every grid point, the non-adversarial scenarios must report nothing in `Diff::incomplete`, and no scenario's time may exceed its multiplier of the same-sized `identical` run in the current baseline table — the recorded multipliers, not a universal constant, being the enforceable wall-clock half, with the all-cells-change overage attributed to retained cell assembly as below. When a constant changes, re-run the grid, re-verify both halves, and re-record the table.

The default multiples — 20 for renames, 5 for swaps — were tuned under one further measured criterion: no grid point loses a completion the previous fixed 2048-pair budgets funded, verified by running both builds over the full grid and comparing `Diff::incomplete` point by point. The multiples sit just above the ten-column adversaries' analytic needs (~200 full-row rename examinations across eleven columns is ~18.2 rows per cell; a fully swapped ten-column table's crossing enumeration is ~4.5), and the comparison came out one-sided: every point matches, and 100k×100 completes three stages the pair budgets cut short — `rename_and_modify`'s renames, `swapped`'s enumeration, and `full_rewrite`'s swap check. Funding the swap enumeration there also made the run 30% faster, resolved swaps leaving no changed cells to assemble; funding `rename_and_modify`'s inference costs real time (its 16× below), which is the price of the completed answer rather than overhead.

Two readings to keep straight when a ratio looks bad. First, `full_rewrite`'s overage — and every all-cells-change scenario's — is largely not the bounded stage: the capped summary fallback is a trivial linear pass, and the extra time is assembling the complete cell-level diff, which is a retained design invariant rather than a search a budget could cut. Second, `renamed_distinct`'s rise past 100k rows is the exact stage's full-column digest join — the unbudgeted linear pass the design accepts — visible against a floor that no longer buries it; it reports nothing incomplete at any grid point, as the rule requires of a non-adversarial scenario.

## Baseline (2026-08-06, Apple Silicon, after the same-type fast paths)

Ratios of scenario time to `identical` at the same size; `identical` absolute times on the first row. These are the recorded multipliers the acceptance rule enforces. The fast-path step halved the floor again — native cell comparison plus the `KeyIndex`, `minimal_moves`, and `RowSample` cleanups — so several multipliers rose while every absolute time fell or held; check a suspicious ratio against the prior table's absolute times before reading it as a regression. The step's own wins, output-identical by construction and verified against the prior build: `identical_strings` 1M×10 fell 804 ms → 226 ms and 100k×100 572 ms → 100 ms; `renamed_strings` fell ~15–20% and its remaining cost is rename verification and informativeness materializing string values, which the step deliberately kept and the leads in `plan-next.md` record.

| | 1k×10 | 1k×100 | 1k×1000 | 100k×10 | 100k×100 | 1M×10 |
|---|---|---|---|---|---|---|
| `identical` | 0.64 ms | 6.4 ms | 68 ms | 14 ms | 51 ms | 207 ms |
| `identical_strings` | 3.62× | 3.64× | 3.48× | 1.40× | 1.94× | 1.09× |
| `renamed_distinct` | 1.37× | 1.37× | 1.34× | 6.57× | 19.32× | 6.78× |
| `renamed_strings` | 4.14× | 4.15× | 3.94× | 21.43× | 59.40× | 22.10× |
| `renamed_constant` | 1.41× | 1.31× | 1.29× | 6.70× | 16.22× | 4.65× |
| `rename_and_modify` | 2.28× | 3.37× | 3.35× | 4.00× | 41.09× | 2.36× |
| `swapped` | 1.50× | 2.51× | 2.77× | 1.74× | 18.92× | 1.13× |
| `full_rewrite` | 2.38× | 2.59× | 2.79× | 8.60× | 37.01× | 6.03× |

## Prior baseline (2026-08-06, Apple Silicon, row-denominated defaults, before the fast paths)

The row-budget tuning run, kept as the immediate comparison; older baselines (2026-08-05 fixed pairs, 2026-08-04 original tuning) live in this file's git history.

| | 1k×10 | 1k×100 | 1k×1000 | 100k×10 | 100k×100 | 1M×10 |
|---|---|---|---|---|---|---|
| `identical` | 0.77 ms | 7.0 ms | 74 ms | 29 ms | 138 ms | 407 ms |
| `renamed_distinct` | 1.26× | 1.27× | 1.28× | 3.49× | 7.76× | 3.84× |
| `renamed_constant` | 1.16× | 1.13× | 1.11× | 3.22× | 5.84× | 2.53× |
| `rename_and_modify` | 1.95× | 2.74× | 2.85× | 2.78× | 16.12× | 1.95× |
| `swapped` | 1.33× | 2.29× | 2.43× | 1.25× | 7.12× | 1.02× |
| `full_rewrite` | 1.98× | 2.22× | 2.51× | 4.72× | 13.12× | 3.39× |

## Profiling a point

When a number needs explaining rather than comparing, sample a looped run. Write a throwaway example that repeats the interesting `diff_tables` call, then:

```console
cargo build --release --example <name>
./target/release/examples/<name> & sample $! 10 -file profile.txt
```

The "Sort by top of stack" section at the end of `profile.txt` is usually enough. The 2026-08-05 profiles that drove the constant-factor step are recorded in that step's `plan.md`: before it, roughly 57% of an identical million-row run was eager projection work behind sampled questions and ~25% was SipHash, with the necessary linear passes a small minority; after it, the top of the profile is those linear passes — key indexing, canonicalization, cell comparison, and the ordering LCS.
