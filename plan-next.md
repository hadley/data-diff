---
title: data-diff next steps
---

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`. Implement only that plan, leave the result uncommitted for owner review, and do not start the following item until the owner has reviewed and committed the current work.

Complete maintenance work and add reconciliation features in dependency order, giving each isolated fixtures, integration coverage, and determinism checks:

1. Benchmark the complete pipeline and introduce deterministic sampling, computation budgets, valid partial results, and incomplete-stage reporting. Approximate rename inference and swap inference are the stages that most need it, being quadratic in candidates and linear in matched rows with no bound on either. This is also when edit summarization gains a bounded valid-cover fallback and may emit `optimal: false`.
1. Extend comparison beyond the four MVP types to the parquet types datasets most commonly carry: dates (`Date32`/`Date64`), timestamps in every unit with and without a timezone, times, decimals (`Decimal128`/`Decimal256`), and binary (`Binary`/`LargeBinary`/`FixedSizeBinary`), with nested lists and structs behind them. For each addition, decide which cross-type pairs compare and by what rule (e.g. timestamp unit conversions, date ↔ timestamp at midnight, decimal ↔ integer/double when exact). Be explicit about the fallback for a type the tool does not know: today `validate_tables` makes an unsupported column fatal, and this step should replace that with the degradation `design.md` sketches — values comparable only between identical source types, excluded from cross-type rename inference, hintable into identity — so an unknown type costs that column's detail rather than the whole comparison. Incomparable pairs become constructible again here, so this step also implements the reading recorded in `design.md` (a same-named incomparable pair is a drop and an addition, declined where it would be claimed) and revives the `incompatible_types` rejection and hint vocabulary retired when booleans joined the numeric domain.

Defer decisions about hint syntax, UI presentation, thresholds, and concrete budgets until the prerequisite behavior exists and can be benchmarked. Preserve the central invariants: deterministic reconciliation, no inferred event without underlying evidence, and a result model that retains the complete cell-level diff.
