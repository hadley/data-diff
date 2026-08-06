---
title: Row-denominated budgets tuned against the lean floor
---

# Todo

- [x] **Re-denominate the pair budgets in rows.** `Budgets.rename_pairs` and `swap_pairs` become `rename_rows` and `swap_rows` of a new public `RowBudget`: `PerCell(usize)`, this many row examinations per cell of the compared table, and `Rows(usize)`, exactly this many. `agreement_rows` and `summary_cells` keep their absolute counts and their existing rationales — the sample size is a statistical floor and the summary cap tracks reader interest, neither of which scales with the input. The defaults become `PerCell` constants — starting points `PerCell(4)` for renames and `PerCell(2)` for swaps — so the budget and the acceptance rule's bar scale together by construction, at every size, on every machine. Field docs state the unit, the derivation, and what exhaustion strands.
- [x] **Resolve the budget from the input once per pass.** `RowBudget::resolve(cells) -> usize` computes the row allowance, saturating; `cells` is the matched rows times the wider side's column count — the size of the table the unavoidable linear pass reads, which is exactly the quantity the acceptance rule prices everything against. `run_pass` resolves both budgets afresh each pass, so reconsideration's second pass keeps its fresh counters and a bounded first pass still cannot starve it.
- [x] **Meter charges what an examination costs.** `Meter::charge` takes the examination's row count: a full-row verification or informativeness measurement costs the matched rows, a sampled measurement costs the sample's size, and a memoized answer still costs nothing — the memo check precedes the charge today and keeps doing so. A charge the remainder cannot fund kills the meter for good, so exhaustion stays one point in the deterministic examination order and strands exactly the tail, as the design's partial-result arguments assume; a zero-cost charge always succeeds, which is what makes empty tables incapable of exhausting anything. Charge sites: `Aligned::verify`, `measure_full`, and `measure_sampled` know their own costs, so callers stop counting units and the budget arithmetic lives beside the work it prices.
- [x] **Tune the per-cell constants by benchmark.** The rule, restated: by construction each bounded stage examines at most `c` rows per cell, and the grid confirms the two halves that construction cannot — no non-adversarial scenario reports an incomplete stage at any grid point, and each adversarial scenario's wall clock stays within the multiplier of the same-sized `identical` run that the tuning records, cell-assembly overage excused as already documented. `c_rename` must clear the bulk-rename case with headroom: the positional pre-pass verifies and measures each diagonal pair over full rows, ~2 row-touches per renamed cell, which is why 4 is the starting point. `c_swap` starts at 2; non-adversarial scenarios enumerate no crossings at all, so its binding evidence is the `swapped` scenario's behavior against today's, which the 2048-pair budget already cut short at the same grid points. Raise or lower each constant by the stated rule, never by feel, and record the tuning in `benches/README.md`.
- [x] **Re-run the grid and rewrite the acceptance rule's text.** `benches/README.md`: the rule section describes the row-denominated scheme and retires the third reading — the 2026-08-05 tension this step exists to resolve — keeping the first two readings that still apply; a fresh dated baseline table lands above the two kept priors, and the recorded wall-clock multipliers become the rule's enforceable half.
- [x] **Update `design.md`'s budgets section.** The unit is a row examined, not a pair; the budget derives from the input's own size so bounded work is a constant multiple of the linear pass, which is the design's own standard for "predictably bounded"; sticky exhaustion is recorded with its reasoning (one exhaustion point keeps stranded-tail arguments valid); the fixed budgets that stay fixed keep their recorded rationales; and the queue's recorded tension between pair-denominated budgets and the row-scaled bar is resolved and dated.
- [x] **Touch `README.md`'s one budget sentence.** "Fixed computation budgets" becomes budgets proportional to the input's own size, in the same breath that already promises a budget never makes anything up; the `incomplete_*()` documentation is otherwise unchanged, no output kind moving.
- [x] **Convert the injected test budgets.** Every test that pins stranding behavior through a tiny `rename_pairs`/`swap_pairs` count converts to `RowBudget::Rows` with a value scaled to its fixture's rows, chosen so each test strands exactly the pairs it strands today; new unit tests cover `Meter`'s variable charging, sticky exhaustion, and free zero-cost charges, and `RowBudget::resolve`'s arithmetic including saturation. Integration tests drive a derived `PerCell` budget through the public API on a fixture sized to bind it, proving exhaustion reports through `Diff::incomplete` exactly as before.
- [x] **Verify the demo and determinism.** Demo fixtures sit far below any derived budget, so transcripts stay byte-identical, confirmed by `tests/readme.rs` rather than assumed; repeated runs stay byte-identical on exhausted and non-exhausted paths, checked on both sides of each budget's boundary.
- [x] **Complete the acceptance pass.** `cargo build --workspace --all-targets`, `cargo test --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`, byte-identical repeated runs, and `cargo bench` compiling and running.

# Goal

The constant-factor step cut the `identical` floor 9–14× while the adversarial scenarios fell 2–4×, leaving the acceptance rule's arithmetic — bounded stage cost within twice the same-sized identical run — failing at several grid points even though every point got absolutely faster. The profiled causes are structural, not incidental: a rename examination is priced in pairs but costs rows, so `renamed_constant` at 100k×10 runs ~100 verifications, a twentieth of its 2048-pair budget, and still exceeds a bar that shrank to about twenty column passes. A pair-denominated budget and a row-denominated bar can never track each other across table shapes — the affordable examination count scales with columns while the budget stands still — and the same mismatch would reopen at the next speedup.

This step makes the budget and the bar the same currency. The two search budgets count rows examined and default to a small constant per cell of the compared table, so "each bounded stage does at most `c` times the work of reading the table" holds by construction, machine-independently, at every size and shape; the benchmarks stop defining the bound and start confirming its two unconstructible halves — that ordinary inputs never bind, and that the wall-clock corollary holds with the recorded multiplier. Exhaustion semantics do not change: the same conservative partial results, the same `Diff::incomplete` reporting, the same fresh counters per reconsideration pass. What changes is which inputs exhaust — degenerate and adversarial shapes now exhaust in proportion to what examining them actually costs — and that is the owner-approved semantic change this step exists to make (2026-08-05).

# Scope

## What changes

- `src/model.rs`: `RowBudget`, the renamed `rename_rows`/`swap_rows` fields, `PerCell` defaults, docs.
- `src/agreement.rs`: `Meter::charge(rows)` with sticky exhaustion; charges move inside `verify`/`measure_full`/`measure_sampled`.
- `src/lib.rs`: resolving budgets from the pass's matched rows and column count.
- `src/rename.rs`, `src/swap.rs`: stop passing unit counts; stage logic otherwise untouched.
- `benches/README.md`: the restated rule, the tuned constants, a fresh baseline table.
- `design.md`: the budgets section; `README.md`: one sentence.
- Test conversions and the new unit and integration coverage.

## What stays and why

- **Every inference rule and threshold.** Agreement, kappa, informativeness, mutual uniqueness, the swap bars, examination order, endpoint-group acceptance, swap all-or-nothing, the summary fallback — budgets change how much examination is funded, never what examined evidence means.
- **`agreement_rows` and `summary_cells`, absolute.** The sample size is a statistical floor recorded with its error bound; the summary cap tracks the reader's interest in cell-level minimality. Neither claims to scale with the input, and neither failed the rule.
- **Exhaustion semantics and reporting.** Valid conservative partial results, `Diff::incomplete` in fixed order, `incomplete_*()` lines, fresh counters per pass.
- **Determinism.** A resolved budget is a pure function of the input and options; charges are counts of deterministic quantities in a deterministic order; sticky exhaustion keeps the stranded set a pure function too.
- **The complete cell-level diff and its cost.** Still never sampled, budgeted, or shrunk; the all-cells-change scenarios' wall-clock overage remains excused as the retained invariant it is.

## Explicitly deferred

- **CLI and UI exposure of budgets.** `RowBudget` is library surface; flags stay deferred with the other UI decisions.
- **Budgeting the summary in rows or re-deriving `summary_cells`.** The König solve is already capped by an absolute cell count with its own rationale; nothing measured argues for touching it.
- **The performance leads and the same-type fast path.** Queued separately; nothing here depends on them.

# Design

## The budget is the bar's own currency

The acceptance rule prices every stage against the linear pass, which reads matched rows times columns — cells. A budget that holds by construction must be denominated in the same units: rows examined, allowed in proportion to cells. `PerCell(c)` says a stage may do at most `c` times the work of reading the table, which is the design's standing definition of predictably bounded computation, and it holds on every machine because both sides of the inequality are counts. `Rows(n)` remains for tests and embedders who want an absolute ceiling; the default is proportional because a fixed count is exactly the mistake being repaired.

## Sticky exhaustion keeps the stranded tail an interval

With unit charges, a failed charge implied an empty meter, so everything after the failure failed too and the stranded candidates formed one tail of the examination order — which is what the design's partial-result arguments lean on. Variable charges would break that for free: a large verification could fail while a later small measurement still fits, scattering the stranded set through the order. Killing the meter at the first unfundable charge restores the invariant at the cost of a little unspent budget, keeps "the stage records itself incomplete" meaning one thing, and is the conservative direction — work is declined, never over-admitted. A zero-cost charge succeeds regardless: zero rows examined is zero work, and empty tables must not exhaust.

## The costs are the ones already recorded

A full-row examination — exact verification or informativeness measurement — costs the matched rows; a sampled measurement costs the sample. These are precisely the weights the previous step's plan recorded when it distinguished "an exact unit weighs its full rows where an approximate unit weighs its sample"; the pair budget flattened that weight to one, and this step stops flattening it. Digest lookups, projection construction, and the rewritten filter keep their recorded costs and stay outside the budgets.

# Verification

- Unit tests: `Meter` funds an examination it can afford, kills itself at the first it cannot, answers memoized questions free forever, and accepts zero-cost charges in any state; `RowBudget::resolve` multiplies and saturates; converted stranding tests strand the same pairs as today, asserted against the same expectations.
- Integration tests: a `PerCell` budget on a fixture sized to bind reports `Renames`/`Swaps` in `Diff::incomplete` with valid conservative events; default budgets on ordinary fixtures report nothing; reconsideration's second pass runs under freshly resolved budgets; byte-identical repeated runs on both sides of each boundary.
- CLI snapshots: unchanged — the `incomplete_*()` lines and their placement do not move.
- `tests/readme.rs`: demo transcripts byte-identical under the derived defaults.
- Benchmarks: the grid re-run under the tuned constants satisfies the restated rule's both halves, recorded in `benches/README.md`.

# Definition of done

This step is complete when:

- the rename and swap budgets are row-denominated with `PerCell` defaults, resolved per pass from the input, and every charge site prices its examination in the rows it actually reads;
- exhaustion is sticky, strands one tail of the deterministic order, and reports exactly as today;
- the tuned constants pass the restated rule on the full grid — nothing incomplete on non-adversarial scenarios, recorded wall-clock multipliers on adversarial ones — and `benches/README.md`'s third reading is retired as resolved;
- `design.md` records the units, the derivation, and the sticky rule; `README.md`'s budget sentence matches;
- demo transcripts are byte-identical and repeated runs are byte-identical on every path; and
- the full test suite, strict Clippy, formatting, and diff checks pass across the workspace.
