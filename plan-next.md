---
title: data-diff next steps
---

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`. Implement only that plan, leave the result uncommitted for owner review, and do not start the following item until the owner has reviewed and committed the current work.

Complete maintenance work and add reconciliation features in dependency order, giving each isolated fixtures, integration coverage, and determinism checks:

1. Promote the common opaque types to semantic cross-type comparison, replacing the identical-source-type-only regime the opaque-columns step gave them. Each pair needs its own decided rule in the comparison matrix: timestamps across units and across the with/without-timezone divide, date ↔ timestamp (midnight or refusal), `Date32` ↔ `Date64`, decimal ↔ integer and decimal ↔ double when the value is exact, and decimals across precision and scale. Whether strings parse against any temporal type belongs here too, if anywhere. Exactness discipline as ever: unit conversion must be overflow-checked, and a pair with no defensible rule stays incomparable rather than approximately equal.
1. Benchmark the complete pipeline and introduce deterministic sampling, computation budgets, valid partial results, and incomplete-stage reporting. Approximate rename inference and swap inference are the stages that most need it, being quadratic in candidates and linear in matched rows with no bound on either. This is also when edit summarization gains a bounded valid-cover fallback and may emit `optimal: false`.

Defer decisions about hint syntax, UI presentation, thresholds, and concrete budgets until the prerequisite behavior exists and can be benchmarked. Preserve the central invariants: deterministic reconciliation, no inferred event without underlying evidence, and a result model that retains the complete cell-level diff.
