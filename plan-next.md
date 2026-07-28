---
title: data-diff next steps
---

# Minor todo

Small things to fix in additional commits before the next big thing. These don't need a PR.

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`. Implement only that plan, leave the result uncommitted for owner review, and do not start the following item until the owner has reviewed and committed the current work.

Complete maintenance work and add reconciliation features in dependency order, giving each isolated fixtures, integration coverage, and determinism checks:

1. Add approximate rename inference and then swap detection, initially examining all matched rows. This is also where exact inference should gain an information-content requirement, so that two all-null columns are no longer paired as a rename.
1. Add validated `col_rename` hints, carrying the hint machinery every kind shares: parsing, normalization, deduplication, whole-group rejection of contradictory claims, and the issue channel with `hint_missing_target` and `contradictory_hints`.
1. Add validated `col_add`, `col_drop`, and `col_edit` hints, extending conflict detection across all four kinds and adding the `hint_no_change` and `hint_unresolved_identity` issues.
1. Design and implement row-number fallback, explicitly deciding which reconciliation stages remain valid, must be skipped, or must report incomplete results without a semantic row key. This is also where a rejected declared key must stop discarding the identities its paired components asserted: separate component parsing and endpoint resolution from key validation, so those identities survive into the diff the fallback produces.
1. Benchmark the complete pipeline and introduce deterministic sampling, computation budgets, valid partial results, and incomplete-stage reporting. This is also when edit summarization gains a bounded valid-cover fallback and may emit `optimal: false`.

Defer decisions about hint syntax, UI presentation, thresholds, and concrete budgets until the prerequisite behavior exists and can be benchmarked. Preserve the central invariants: deterministic reconciliation, no inferred event without underlying evidence, and a result model that retains the complete cell-level diff.
