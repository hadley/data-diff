---
title: data-diff next steps
---

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`. Implement only that plan, leave the result uncommitted for owner review, and do not start the following item until the owner has reviewed and committed the current work.

Complete maintenance work and add reconciliation features in dependency order, giving each isolated fixtures, integration coverage, and determinism checks:

1. Extend bounded fanout to guessed keys, amending `design.md`, admitting a candidate that is unique in `old` and within the fanout bound in `new`, ranking every strictly unique candidate ahead of every fanout-bearing one so no comparison that resolves today changes, and scoring candidates by distinct shared key values rather than matching new rows so duplication cannot inflate a candidate. This must land before row-number fallback, which should be designed against the final guessing rules.
1. Add paired key components and validated rename/add/drop/edit hints.
1. Infer exact renames from aligned matched rows.
1. Add approximate rename inference and then swap detection, initially examining all matched rows.
1. Design and implement row-number fallback, explicitly deciding which reconciliation stages remain valid, must be skipped, or must report incomplete results without a semantic row key.
1. Benchmark the complete pipeline and introduce deterministic sampling, computation budgets, valid partial results, and incomplete-stage reporting. This is also when edit summarization gains a bounded valid-cover fallback and may emit `optimal: false`.

Defer decisions about hint syntax, UI presentation, thresholds, and concrete budgets until the prerequisite behavior exists and can be benchmarked. Preserve the central invariants: deterministic reconciliation, no inferred event without underlying evidence, and a result model that retains the complete cell-level diff.
