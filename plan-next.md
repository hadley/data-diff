---
title: data-diff next steps
---

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`. Implement only that plan, leave the result uncommitted for owner review, and do not start the following item until the owner has reviewed and committed the current work.

Complete maintenance work and add reconciliation features in dependency order, giving each isolated fixtures, integration coverage, and determinism checks:

1. Decide what an incompatible same-name column pair means. `reconcile_schema` currently treats one as fatal, so changing a column from boolean to integer — or exchanging two columns of those types — fails the whole comparison rather than reporting a drop and an addition, or an edit whose values all changed. Settle which reading is right and where the pair should be resolved, given that swap inference could account for some of them if the columns survived that far, which today they do not.
1. Benchmark the complete pipeline and introduce deterministic sampling, computation budgets, valid partial results, and incomplete-stage reporting. Approximate rename inference and swap inference are the stages that most need it, being quadratic in candidates and linear in matched rows with no bound on either. This is also when edit summarization gains a bounded valid-cover fallback and may emit `optimal: false`.
1. Consider one-sided diffs, i.e. provide a brief summary if a parquet file is added (col_add(), row_add()) or removed (col_drop(), row_drop())

Defer decisions about hint syntax, UI presentation, thresholds, and concrete budgets until the prerequisite behavior exists and can be benchmarked. Preserve the central invariants: deterministic reconciliation, no inferred event without underlying evidence, and a result model that retains the complete cell-level diff.
