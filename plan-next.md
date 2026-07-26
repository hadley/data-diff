---
title: data-diff next steps
---

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`.
Implement only that plan, leave the result uncommitted for owner review, and do
not start the following item until the owner has reviewed and committed the
current work.

Complete maintenance work and add reconciliation features in dependency order,
giving each isolated fixtures, integration coverage, and determinism checks:

1. Add paired key components and validated rename/add/drop/edit hints.
2. Infer exact renames from aligned matched rows.
3. Support bounded declared-key fanout while keeping fanout cells separate.
4. Add approximate rename inference and then swap detection, initially
   examining all matched rows.
5. Benchmark the complete pipeline and introduce deterministic sampling,
   computation budgets, valid partial results, and incomplete-stage reporting.
   This is also when edit summarization gains a bounded valid-cover fallback and
   may emit `optimal: false`.
6. Expand scalar type support, then design a bounded large-data execution model
   and interactive UI.

Defer decisions about hint syntax, UI presentation, thresholds, and concrete
budgets until the prerequisite behavior exists and can be benchmarked. Preserve
the central invariants: deterministic reconciliation, no inferred event without
underlying evidence, and continued access to the complete cell-level diff.
