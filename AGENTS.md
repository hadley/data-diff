# Agent instructions for data-diff

## Planning documents

* `plan.md` — the detailed plan for the single step currently in flight, with a
  checklist tracked to completion.
* `plan-next.md` — the ordered queue of future steps. Each item becomes its own
  `plan.md` and its own dedicated branch from `main`.
* `design.md` — the durable design; plans must preserve its central
  invariants: deterministic reconciliation, no inferred event without
  underlying evidence, and continued access to the complete cell-level diff.

## "Next problem" workflow

When the owner says "next problem" (or otherwise asks for the next plan):

1. Confirm the current `plan.md` is complete: every checklist box checked and
   the work committed to `main` by the owner. If not, stop and say what is
   outstanding.
2. Create a dedicated branch from `main` for the new step, named after it.
   Both the plan and its implementation live on this branch.
3. Take the **first** item from `plan-next.md`.
4. Survey the relevant code, then rewrite `plan.md` from scratch as a detailed
   plan for that item alone, keeping the established format: frontmatter
   title, a `# Todo` checklist, a `# Goal`, a `# Scope` (including what is
   explicitly deferred), design or verification sections as needed, and a
   `# Definition of done`. The plan describes the step itself; the execution
   rules below apply to every step and are not restated in `plan.md`.
5. Remove the item from `plan-next.md` and renumber the remaining items.
6. Leave both files uncommitted on the branch for owner review. Do not start
   implementing until the owner has reviewed the plan.

## Execution rules

Development proceeds at a slow, review-first pace:

* All work for a step — plan and implementation — happens on its dedicated
  branch from `main`; never develop directly on `main`.
* Each plan is one separate PR-sized change.
* Present the finished branch for careful review with its changes left
  uncommitted; the owner alone decides when to commit. Do not begin the next
  item until that review is done.
* Every step gets isolated fixtures, integration coverage, and determinism
  checks; repeated runs must produce byte-identical output.
* Before presenting work, run the full test suite, strict Clippy, formatting,
  and diff checks.

## Settled conventions

* Unit tests stay inline in their production module as `#[cfg(test)] mod
  tests` blocks, the dominant Rust convention. Extracting them into separate
  files was considered and rejected (2026-07-25); do not re-propose it.
