---
title: Key reconsideration
---

# Todo

- [x] **Extract the single pass.** Everything in `diff_tables` below key resolution — row matching, schema reconciliation, rename and swap inference, ordering, cells, edit-hint validation, summarization — becomes one function over a given key and starting map, returning an intermediate result the orchestrator can publish or discard. Hint resolution stays above it and runs once, being key-independent.
- [x] **Define implausibility.** A `ChangeMass { changed, total }` measured from a pass's row matches and cells, and a predicate `changed * 100 > total * MAX_PLAUSIBLE_CHANGE_PERCENT` with the constant at 50, exact integer arithmetic like `within_fanout_limit`, strict at the limit. The measure takes the basis: under the fallback, added rows are read as appended at the end and contribute nothing to `changed`, while dropped rows always count — the asymmetry argued below.
- [x] **Teach `guess_key` exclusions.** A list of excluded candidate pairs it must pass over, empty on the first pass. Confirm with a test that the existing identity-map path already surfaces a cross-name candidate when the map holds an inferred identity, since reconsideration depends on that rather than adding a discovery mechanism.
- [x] **Orchestrate the two passes in `src/lib.rs`.** Run pass one, evaluate the two triggers, and run pass two at most once, straight-line code with no loop. Pass two starts from the hint map plus the selected key pair claimed with its pass-one basis — both halves of the exchange when that basis is `swapped` — and re-derives everything else. Carry the declared rejection and any retraction onto the final `KeyDiff` whichever pass wins.
- [x] **Add the model for what happened.** `KeyRetraction` with the retracted components and its `ChangeMass`, carried as `KeyDiff::retraction`; `Diff::regeneration` as `Option<ChangeMass>`. Both hold their counts so the lines don't have to say them.
- [x] **Render `key_retracted()`.** A problem line, `key_retracted([amount], reason: excessive_change)`, placed after `key_invalid()` and before the hint issues in the problems block. Supersession renders nothing.
- [x] **Render `table_regenerate()`.** When `regeneration` is set, the row-level findings — `row_add()`, `row_drop()`, `row_edit()`, `row_fanout()`, `row_order()`, value-only `col_edit()`, and the `changes:` field of a type-and-value `col_edit()` — collapse into the single line. `col_key()`, `col_rename()`, `col_add()`, `col_drop()`, `col_order()`, and type-only `col_edit()` stay.
- [x] **Update `design.md`.** The vocabulary gains `table_regenerate()`. Key resolution gains a reconsideration section: the two triggers, the once-only rule, the exemption for declared keys, and which result wins. The fallback section's "should that prove too weak in practice" paragraph is updated now that the regenerate event exists, and it records the appended-tail assumption and why it is not symmetric.
- [x] **Update `README.md`.** The output section gains `key_retracted()`, `table_regenerate()`, and a sentence on reconsideration.
- [x] **Refresh the demo.** The "Without the pair" section of `demo/README.md` flips from demonstrating the failure to demonstrating reconsideration, and its prose ("too late to be any use") is rewritten by hand after `UPDATE_README=1 cargo test --test readme`. A new fixture pair with no usable key and wholesale different values gets a regenerate section, written by `examples/generate_demo.rs`.
- [x] **Cover it.** Unit tests for the predicate, the exclusion, and trigger evaluation; integration coverage in `tests/diff.rs` for each reconsideration path and for once-only; CLI snapshots in `tests/cli.rs`; determinism checks that repeated runs are byte-identical.
- [x] **Complete the acceptance pass.** `cargo build --workspace --all-targets`, `cargo test --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`, and byte-identical repeated runs.

# Goal

Key guessing pairs candidates by name, so a renamed key column is invisible to it, and the demo shows the cost. Rename inference *does* find the identity — but `resolve_key` has already run, and the evidence arrives too late to match rows with:

```console
$ data-diff demo/key-rename-old.parquet demo/key-rename-new.parquet
col_key([amount], basis: guessed, overlap: 0.67)
col_rename(customer_id -> id, basis: exact)
row_drop(2)
row_add(2)
```

This step feeds that evidence back: after the first pass, the key is reconsidered **once**, and a key the first pass's inference identified can win:

```console
$ data-diff demo/key-rename-old.parquet demo/key-rename-new.parquet
col_key([customer_id -> id], basis: guessed, overlap: 1.00)
col_rename(customer_id -> id, basis: exact)
row_edit(2, changes: 1)
```

The same mechanism handles the other way a guess goes wrong. A guessed key that validates can still be a coincidence, and the tell is a diff in which more changed than stayed the same. Such a key is retracted — reported as a problem, excluded, and the chain rerun — and when even the last resort produces a diff past that threshold, the row-level findings are not worth enumerating and the output says what the evidence actually supports:

```console
$ data-diff demo/regenerate-old.parquet demo/regenerate-new.parquet
col_key([#row], basis: fallback)
table_regenerate()
```

In every case the key is reconsidered at most once. That is the invariant that keeps the process deterministic and bounded, and it is structural — two passes in straight-line code — rather than a counter.

# Scope

## What changes

* `src/lib.rs` splits `diff_tables` into key resolution, a single-pass pipeline function, and an orchestrator that runs the pipeline at most twice.
* `src/key.rs` gives `guess_key` an exclusion list. Candidate discovery through the identity map already exists and is not extended.
* `src/model.rs` gains `ChangeMass`, `KeyRetraction`, `KeyDiff::retraction`, and `Diff::regeneration`.
* `src/human.rs` renders `key_retracted()` in the problems block and `table_regenerate()` in place of the row-level findings.
* `design.md`, `README.md`, `demo/README.md`, `examples/generate_demo.rs`, and the test suites listed above.

## What stays and why

* `match_rows`, `reconcile_schema`, `rename::infer`, `swap::infer`, `order`, `cells`, `hint::validate_edits`, and `summary` are all untouched. Reconsideration reruns them over a different key; it does not change what any of them does.
* Hint resolution runs once. Hints are claims about columns, not about the key, and resolving them again with the same inputs would produce the same result.
* Declared keys are exempt from all of this. A validated declaration is the user's assertion about the data, and second-guessing it would put the tool's judgement above an instruction; `--key '#row'` is a declaration like any other. The existing rejection path for declarations the data cannot support is unchanged.
* The fanout limit and guessing's ranking rule are unchanged. Reconsideration re-enters the existing chain with more knowledge; it does not alter what makes a key acceptable.

## Explicitly deferred

* Tuning `MAX_PLAUSIBLE_CHANGE_PERCENT`. Fifty is defensible — past it, change outweighs sameness and the description has stopped compressing — but the number is a tunable constant in the style of `MAX_FANOUT_PERCENT`, and benchmarking-driven tuning is the budgets step's business.
* Disabling rename and swap inference under an implausible fallback. `design.md` already names this as a possible later step; `table_regenerate()` reduces the harm by withholding the row story those identities would misdescribe, and the identities themselves still print with their basis visible.
* Reporting magnitude on the `table_regenerate()` line. The counts live in `Diff::regeneration`, following `invalid_value`, whose row and side the line also does not say. If the owner wants a visible magnitude, `changes:` exists in the field vocabulary, at the cost of a line whose first argument is a field.
* Any interactive override of a reconsidered key. The UI item owns that.

# Design

## Two ways out, weighed

The queue item offered two designs: resolve the key again once inference has identified a candidate, or infer key renames before row matching by pairing rows positionally for that inference alone. This plan takes the first, because it subsumes the second. When nothing identifies a row, pass one *is* positional — the fallback basis — and its rename inference *is* rename inference over positionally paired rows; reconsideration then feeds any identity it found back into key guessing. The second design's open question — how far positional evidence may be trusted when the files differ in row count or order — answers itself here: the candidate is not trusted at all. It is only admitted to key guessing, where it must validate as a key on its own values — present, unique in `old`, within the fanout limit — and win the existing shared-count ranking against every other candidate. A coincidental identity that cannot actually identify rows fails that gauntlet on the data, not on a trust heuristic.

The item's other questions dissolve the same way. Whether a second pass stays deterministic: it is the same deterministic pipeline over a deterministically chosen key. Whether it can loop: it cannot, structurally — the orchestrator runs pass one, at most one pass two, and stops. Which result wins when the passes disagree: pass two, unconditionally, because it was computed with strictly more information and because ranking the two passes would need a quality metric that would itself need defending. The backstop for a second pass that is still noise is `table_regenerate()`, not a third pass.

## The triggers

After pass one, reconsideration is evaluated only when the basis is `guessed` or `fallback` — a validated declaration is never reconsidered. Two triggers, either sufficient:

**A better key exists.** Rerun `guess_key` with pass one's final identity map, which now contains what rename and swap inference established. If the winner differs from pass one's key, run pass two with it. Evaluating the trigger *is* running the pass-two guess, so a candidate that does not actually qualify or does not outrank the incumbent changes nothing and no second pass runs. From a fallback, any winner at all differs.

**The diff is implausible.** If pass one's key was guessed and its diff is implausible (below), the guess is retracted: recorded as a `KeyRetraction`, added to the exclusion list, and the guess rerun without it — landing on the next candidate or the fallback. A fallback cannot be retracted; there is nothing below it, so an implausible fallback goes directly to regeneration reporting with no second pass.

Both can fire together — a junk guess and a better candidate — and combine naturally: one rerun of `guess_key` with the enriched map and the exclusion. Whatever pass two produces is final; its diff is never used to trigger anything further, which is the once-only rule.

## Implausibility

One predicate serves both the retraction trigger and regeneration reporting, because they are one judgement: this matching explains the data so badly that the diff under it is not credible as a story of edits. The measure is cell mass, symmetric across sides:

$$
changed = \sum_{\text{dropped rows}} c_{old} + \sum_{\text{added rows}} c_{new} + 2 \times |\text{changed cells}|, \qquad total = n_{old} c_{old} + n_{new} c_{new}
$$

where $c$ counts each side's columns and both sums are taken over the rows outside fanout groups: $total$ excludes the cells of each fanout group's old row and new rows, and $changed$ contains no fanout cells. Exclusion has to be two-sided. Fanout's own limit caps *affected keys* at 10% of shared keys, not rows — one affected key may fan out to arbitrarily many new rows — so counting those rows in $total$ but nothing of them in $changed$ would let a large fanout dilute the ratio and hide an implausible matching. Fanout is orthogonal evidence with its own limit, and the mass measures only the one-to-one story: a dropped or added row contributes its whole width once, and a changed matched cell exists in both files and contributes two. The diff is implausible when $changed \times 100 > total \times 50$, in exact integer arithmetic, strict at the limit, so an empty table (total zero) is never implausible. Cell mass rather than row counts because rows are too coarse a unit: one changed cell in a wide row is a normal edit, while a fallback over reordered rows changes nearly every cell, and it is the second that the predicate exists to catch.

The measure is basis-dependent in one respect. Under the fallback, added rows contribute nothing to $changed$ (while staying in $total$): positional matching puts every addition at the tail, and a longer new file is read, until the evidence says otherwise, as rows appended at the end — appending preserves the position of every pre-existing row, so it is exactly the operation positional matching is right about, and a cleanly matching prefix corroborates it. The evidence that says otherwise is the prefix itself: widespread cell changes there still make the diff implausible on their own mass. The assumption is deliberately not symmetric. Dropped rows keep their full width in $changed$, because a shorter new file gives no licence to assume truncation — rows are usually deleted by filtering, from anywhere, and a positional matching that shifts every later row is precisely the misreading the predicate exists to catch. Under a guessed key both additions and drops count in full: there the key identifies rows by value, an addition is a claim the key vouches for, and the appended-tail reading has no special standing.

The threshold is deliberately conservative in one direction: a *declared* key showing 90% change is respected, because the user vouched for the matching and the edits are then real. Only judgement bases answer to it.

## What pass two inherits

Pass two starts from the hint map — the same starting point as pass one — plus the identity pair of the key it will use, claimed with the basis pass one's inference established (`exact`, `approximate`, or `swapped`). A `swapped` basis carries one thing more: the exchange's companion identity, claimed alongside it with the same basis. A swap is one atomic fact — the model creates `swapped` identities only by exchanging two at once, and every consumer may assume they occur in pairs — so adopting half of it would leave a final `swapped` identity with no exchange behind it, and adopting half adopts it entire. Everything else pass one inferred is discarded and re-derived over the new matching. This is the line "no inferred event without underlying evidence" draws: pass-one identities were derived from a matching the orchestrator has just repudiated, so they may not survive as facts, but the one the key adopts is re-validated on its own values by key resolution itself, which is fresh evidence independent of any matching. Its basis prints as inference established it — `col_rename(customer_id -> id, basis: exact)` beside `col_key([customer_id -> id], basis: guessed)` — which reads as what happened: inference found the identity, and guessing used it.

The declared rejection from before pass one, if any, is carried onto the final key whichever pass wins, as it is today. A retraction is carried the same way, so a run can honestly show the whole chain: `key_invalid()` for the declaration, `key_retracted()` for the guess, and a final fallback key.

## Reporting a retraction, and not a supersession

A retraction is a problem: the tool chose a key, the key produced garbage, and a reader who can see that `amount` looks like an obvious key needs telling why it was not used. It renders in the problems block, after any `key_invalid()` and before the hint issues, subject bracketed like the other whole-key judgements:

```
key_retracted([amount], reason: excessive_change)
```

A supersession — trigger A replacing a guess or a fallback with a better-informed guess — is not a problem: nothing went wrong, the process worked, and the final `col_key()` line with its rename beside it already tells the story. It renders nothing. This asymmetry is the problems block's own rule — it holds what went wrong, not what happened.

`KeyRetraction` carries the retracted components by name (a guessed key's names are resolvable at retraction time) and its `ChangeMass`, so the model holds the detail the line omits, as `RejectionReason::InvalidValue` already does with its row and side.

## Regeneration

When the *final* diff is implausible and its basis is `guessed` or `fallback`, `Diff::regeneration` is set and the human format replaces the row story with one line. The claim `table_regenerate()` makes is that the new file is not usefully described as an edit of the old — it was regenerated — and every line it suppresses is one whose content is conditional on a row matching the tool has just declared untrustworthy: the add/drop/edit/fanout/order row events, value-only column edits, and the `changes:` count on a column edit that also changed type. What stays is everything derived from schemas and identities rather than from matched rows: the key line, renames with their bases, column adds and drops, column order, and type-only column edits. The model is unchanged underneath — the complete cell-level diff, the summary, and every event are still computed and retained, preserving the design invariant that the evidence stays accessible; only the rendering collapses.

`table_regenerate()` takes no arguments. There is no name to put in the subject position — the subject is the table — and the magnitude lives in the model, per the deferral above.

A guessed basis can only reach regeneration on pass two, since an implausible pass-one guess always triggers retraction instead; a fallback can reach it on either pass. A declared basis never does.

## Determinism and cost

Everything here is deterministic: the triggers are pure functions of a deterministic pass, the exclusion list and enriched map are derived from it, and pass two is the same pipeline over different inputs. Repeated runs stay byte-identical. Worst-case cost is two full pipeline runs, a fixed factor that the budgets step will measure rather than something unbounded; no third pass exists to pay for.

# Verification

* Unit tests for the implausibility predicate at, below, and above the limit, including the empty-table and integer-exactness edges, and a case where a large fanout group is excluded from both masses so it neither triggers implausibility nor dilutes it, in the module that owns it. The fallback asymmetry gets its own pair: a file that doubled by appended rows stays plausible under the fallback, and a file truncated by the same amount does not.
* Unit tests in `src/key.rs` that `guess_key` passes over an excluded candidate, and that an identity present in the map surfaces a cross-name candidate — the existing mechanism reconsideration relies on.
* Integration coverage in `tests/diff.rs` for each path: a renamed key recovered with basis `guessed` and its identity basis `exact`; a retracted guess landing on a second candidate; a retracted guess landing on the fallback; an implausible fallback regenerating with no second pass; a declared key with 90% change left alone; and lineage — declared rejection plus retraction both present on the final key.
* A once-only test: a construction whose second pass is itself implausible, asserting exactly one retraction, no third pass, and regeneration under a guessed basis.
* A test that pass-one inferred identities other than the adopted key pair do not survive into pass two's result unless re-derived there, and a test that adopting a `swapped` key pair carries its companion, so the final map never holds a `swapped` identity without its exchange.
* CLI snapshots in `tests/cli.rs` for the retraction line's placement in the problems block and for `table_regenerate()`'s suppression rule, including a type-and-value `col_edit()` printing type-only.
* The refreshed demo sections held to real output by `tests/readme.rs`, with the fixture rules observed: the new regenerate fixture pair is written by `examples/generate_demo.rs` and read by its section, and no orphaned fixtures remain.
* Determinism: repeated runs byte-identical on every new path.

# Definition of done

This step is complete when:

* the demo's renamed-key pair produces a correct diff with no `--key`, the key recovered on reconsideration and the rename's basis preserved;
* a guessed key whose diff is implausible is retracted, reported as `key_retracted([...], reason: excessive_change)` in the problems block, and replaced by the next candidate or the fallback;
* the key is reconsidered at most once, structurally, and repeated runs are byte-identical;
* a declared key is never reconsidered and never regenerated over;
* an implausible final diff under a judgement basis renders `table_regenerate()` in place of the row story, with schema- and identity-level lines retained and the full model unchanged underneath;
* `KeyDiff` carries rejection and retraction together where both occurred, and `Diff::regeneration` carries the mass its line omits;
* `design.md` records the vocabulary addition, the triggers, the once-only rule, and the declared-key exemption; `README.md` and `demo/README.md` show the new behaviour and `tests/readme.rs` holds the demo to it; and
* the full test suite, strict Clippy, formatting, and diff checks pass across the workspace.
