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

## Baseline (2026-08-06, Apple Silicon, after the u32 cell coordinates)

Ratios of scenario time to `identical` at the same size; `identical` absolute times on the first row. These are the recorded multipliers the acceptance rule enforces. The u32 step's win is memory rather than time — the changed-cell vector, the largest thing a `Diff` retains, halves from 40 to 20 bytes per cell (400 MB to 200 MB at the 10⁷-cell grid cap) behind an input-validated ceiling of `u32::MAX` rows and columns — and every point here is within run noise of the prior table.

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

## Prior baseline (2026-08-06, Apple Silicon, after the sampled-counts cache and counting placement)

The sampled-counts step's run, kept as the immediate comparison; older baselines live in this file's git history. Its wins over its own prior: sampled frequency maps built once per column instead of once per crossing measurement took `swapped` 100k×100 from 945 ms to 174 ms and `rename_and_modify` from 2.1 s to 0.52 s, and counting placement of the changed-cell list is most of `full_rewrite`'s drop to 0.75 s at the same point.

| | 1k×10 | 1k×100 | 1k×1000 | 100k×10 | 100k×100 | 1M×10 |
|---|---|---|---|---|---|---|
| `identical` | 0.30 ms | 2.4 ms | 26 ms | 13 ms | 51 ms | 198 ms |
| `identical_strings` | 2.23× | 2.68× | 2.55× | 1.41× | 2.06× | 1.09× |
| `renamed_distinct` | 1.80× | 1.98× | 1.88× | 3.92× | 8.59× | 3.70× |
| `renamed_strings` | 3.33× | 4.08× | 3.76× | 8.46× | 17.84× | 8.13× |
| `renamed_constant` | 2.98× | 2.81× | 2.54× | 6.40× | 12.44× | 4.66× |
| `rename_and_modify` | 4.58× | 7.73× | 7.85× | 2.88× | 10.20× | 2.23× |
| `swapped` | 3.25× | 4.51× | 4.48× | 1.06× | 3.40× | 0.98× |
| `full_rewrite` | 4.35× | 4.51× | 4.48× | 6.00× | 14.60× | 5.16× |

## Profiling a point

When a number needs explaining rather than comparing, sample a looped run. Write a throwaway example that repeats the interesting `diff_tables` call, then:

```console
cargo build --release --example <name>
./target/release/examples/<name> & sample $! 10 -file profile.txt
```

The "Sort by top of stack" section at the end of `profile.txt` is usually enough. The 2026-08-05 profiles that drove the constant-factor step are recorded in that step's `plan.md`: before it, roughly 57% of an identical million-row run was eager projection work behind sampled questions and ~25% was SipHash, with the necessary linear passes a small minority; after it, the top of the profile is those linear passes — key indexing, canonicalization, cell comparison, and the ordering LCS.
