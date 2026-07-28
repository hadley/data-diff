---
title: data-diff next steps
---

# Minor todo

Small things to fix in additional commits before the next big thing. These don't need a PR.

# Next steps

Each item below becomes its own detailed plan and dedicated branch from `main`. Implement only that plan, leave the result uncommitted for owner review, and do not start the following item until the owner has reviewed and committed the current work.

Complete maintenance work and add reconciliation features in dependency order, giving each isolated fixtures, integration coverage, and determinism checks:

1. Add validated `col_rename` hints, carrying the hint machinery every kind shares: parsing, normalization, deduplication, whole-group rejection of contradictory claims, and the issue channel with `hint_missing_target` and `contradictory_hints`.
1. Add validated `col_add`, `col_drop`, and `col_edit` hints, extending conflict detection across all four kinds and adding the `hint_no_change` and `hint_unresolved_identity` issues.
1. Flag the source of each column identity in the human format, so an inferred rename reads as the judgement it is rather than as a certainty, in the way a guessed key already does. The sources are the declared key pair, a rename hint, exact inference, and approximate inference; decide whether a swap is labelled as approximate inference or earns its own. Carry the source on the identity through to `Diff` rather than reconstructing it in the renderer, and settle how a swap renders, including whether the vocabulary's combined `col_rename([a, b], [b, a])` form should replace the two separate lines it produces today. Agreement statistics are deliberately not rendered: the thresholds put every accepted rename above 0.9, so the source carries the information and the number would only dress it up.
1. Design and implement row-number fallback, explicitly deciding which reconciliation stages remain valid, must be skipped, or must report incomplete results without a semantic row key. This is also where a rejected declared key must stop discarding the identities its paired components asserted: separate component parsing and endpoint resolution from key validation, so those identities survive into the diff the fallback produces.
1. Decide what an incompatible same-name column pair means. `reconcile_schema` currently treats one as fatal, so changing a column from boolean to integer — or exchanging two columns of those types — fails the whole comparison rather than reporting a drop and an addition, or an edit whose values all changed. Settle which reading is right and where the pair should be resolved, given that swap inference could account for some of them if the columns survived that far, which today they do not.
1. Benchmark the complete pipeline and introduce deterministic sampling, computation budgets, valid partial results, and incomplete-stage reporting. Approximate rename inference and swap inference are the stages that most need it, being quadratic in candidates and linear in matched rows with no bound on either. This is also when edit summarization gains a bounded valid-cover fallback and may emit `optimal: false`.
1. Consider one-sided diffs, i.e. provide a brief summary if a parquet file is added (col_add(), row_add()) or removed (col_drop(), row_drop())

Defer decisions about hint syntax, UI presentation, thresholds, and concrete budgets until the prerequisite behavior exists and can be benchmarked. Preserve the central invariants: deterministic reconciliation, no inferred event without underlying evidence, and a result model that retains the complete cell-level diff.
