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

1. Move inline unit-test modules into adjacent dedicated files, preserving
   private-module access. This should make code and test changes immediately
   distinguishable when reviewing a diff.
2. Guess eligible single-column keys and allow users to override the guess.
3. Add paired key components and validated rename/add/drop/edit hints.
4. Infer exact renames from aligned matched rows.
5. Support bounded declared-key fanout while keeping fanout cells separate.
6. Add approximate rename inference and then swap detection, initially
   examining all matched rows.
7. Benchmark the complete pipeline and introduce deterministic sampling,
   computation budgets, valid partial results, and incomplete-stage reporting.
   This is also when edit summarization gains a bounded valid-cover fallback and
   may emit `optimal: false`.
8. Expand scalar type support, then design a bounded large-data execution model
   and interactive UI.

Defer decisions about hint syntax, UI presentation, thresholds, and concrete
budgets until the prerequisite behavior exists and can be benchmarked. Preserve
the central invariants: deterministic reconciliation, no inferred event without
underlying evidence, and continued access to the complete cell-level diff.
