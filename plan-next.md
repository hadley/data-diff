---
title: data-diff next steps
---

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`. Implement only that plan, leave the result uncommitted for owner review, and do not start the following item until the owner has reviewed and committed the current work.

Complete maintenance work and add reconciliation features in dependency order, giving each isolated fixtures, integration coverage, and determinism checks:

1. Make test table construction compact. Today every fixture column is spelled out as `("id", Arc::new(Int64Array::from(vec![1, 2, 3])))`, and the identical `fn table(columns: Vec<(&str, ArrayRef)>)` helper is duplicated in six modules (`human`, `schema`, `key`, `input`, `rows`, `cells`) plus the integration tests. Replace both with one shared construction helper that infers the Arrow array type from the literal values, so a two-column fixture reads as a couple of short lines and the intent of each test is visible without decoding its scaffolding. The helper must still reach every case the current fixtures need: nulls, `NaN`, explicit width and type choices where the test is specifically about them, dictionary-encoded strings, and zero-row tables with a declared schema. It must live somewhere the inline `#[cfg(test)]` modules across `src/` and the integration tests under `tests/` can both use, without moving unit tests out of their production module. This is a pure refactor: every existing assertion keeps its current meaning and the suite stays green throughout.
2. Design and implement row-number fallback, explicitly deciding which reconciliation stages remain valid, must be skipped, or must report incomplete results without a semantic row key.
3. Add paired key components and validated rename/add/drop/edit hints.
4. Infer exact renames from aligned matched rows.
5. Support bounded declared-key fanout while keeping fanout cells separate.
6. Add approximate rename inference and then swap detection, initially examining all matched rows.
7. Benchmark the complete pipeline and introduce deterministic sampling, computation budgets, valid partial results, and incomplete-stage reporting. This is also when edit summarization gains a bounded valid-cover fallback and may emit `optimal: false`.
8. Expand scalar type support, then design a bounded large-data execution model and interactive UI.

Defer decisions about hint syntax, UI presentation, thresholds, and concrete budgets until the prerequisite behavior exists and can be benchmarked. Preserve the central invariants: deterministic reconciliation, no inferred event without underlying evidence, and a result model that retains the complete cell-level diff. Now that JSON output is gone that last invariant constrains the library rather than the CLI, and giving users access to that evidence again is future work.
