---
title: Bounded fanout for guessed keys
---

# Todo

- [x] **Amend `design.md`.** Rewrite the "Guessed key" section so a candidate must be unique in `old` and within the fanout bound in `new` rather than unique on both sides, state that selection is by shared key values with freedom from fanout as a tie-break, and redefine the reported overlap denominator over distinct key values. Remove "guessed keys never infer fanout" and the corresponding sentence in "Row matching".
- [x] **Score candidates by distinct shared keys.** In `src/key.rs`, change `candidate_overlap` to return the shared and affected key counts rather than a count of matching new rows, and to reject a candidate only for an invalid value or old-side duplication. Counting rows would give a fanned-out candidate one point per duplicate and bias selection towards the columns the bound is meant to tolerate.
- [x] **Rank by shared keys.** Admit a candidate when it shares at least one key and satisfies the same bound as a declared key, and select by shared keys descending, breaking ties in favour of a candidate that does not fan out and then by old column order.
- [x] **Report overlap over distinct keys.** Compute `KeyOverlap::possible` from the distinct key values on each side rather than the row counts, which is the same number whenever neither side duplicates and is the meaningful one when `new` does.
- [x] **Cover the new selection.** Unit tests in `src/key.rs` for the ranking, the counts themselves, the row-count bias the new metric removes, the overlap denominator, the bound applying to guesses, and old-side duplication still disqualifying a candidate; integration coverage in `tests/diff.rs` for a guessed key that fans out; and a CLI snapshot.
- [x] **Refresh the demo datasets and documentation.** Add a `demo/guessed-fanout-*.parquet` pair whose only eligible candidate fans out, and update `demo/README.md` and `README.md` to describe guessing as fanout-tolerant.
- [x] **Complete the acceptance pass.** Run `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`, and confirm repeated runs still produce byte-identical output.

# Goal

A declared key may now identify one old row and several new rows. A guessed key still may not, because `candidate_overlap` rejects any column that repeats a value on either side. The consequence is backwards: fanout usually arrives from a join that duplicated rows in the identity column, so the column that fanned out is exactly the one guessing discards, and the tool is least able to guess precisely when it has the most to explain. Once row-number fallback exists, that case will silently degrade to positional matching rather than failing, which is worse still.

This step lets a guess fan out under the same bound as a declaration, and ranks candidates by the evidence they carry — the number of key values the two files share — rather than by freedom from fanout. A true key that duplicated one row is then chosen over a coincidentally unique column that identifies far fewer rows.

That changes what some comparisons guess today. The alternative, ranking every unique candidate ahead of every fanned-out one, would preserve current behavior exactly but would keep choosing the weaker key in precisely the case this step exists to fix, so the churn is the point rather than a side effect. It should be rare on real inputs, where a true key usually shares many more values than a column that is unique by coincidence.

# Scope

## What changes

* `design.md`: the "Guessed key" section, the overlap denominator, and the two sentences that say a guess never fans out.
* `src/key.rs`: candidate scoring, eligibility, ranking, and the reported overlap.
* `tests/diff.rs` and `tests/cli.rs`: coverage for a guessed key that fans out.
* `examples/generate_demo.rs`, `demo/README.md`, and `README.md`.

## What stays and why

Everything downstream of key resolution is untouched. `match_rows`, `compare_cells`, the summary, ordering, and the human format all work from a `ResolvedKey` and never ask how it was chosen, so a guessed key that fans out produces the same events a declared one does. That is worth an integration test rather than an edit: the test asserts a `KeyBasis::Guessed` key with a populated `rows.fanout`.

The bound itself is unchanged and shared. `MAX_FANOUT_PERCENT` and the rule `affected * 100 <= shared * MAX_FANOUT_PERCENT` are lifted into one predicate used by both the declared and the guessed path, so the two can never drift. What differs between them is only the consequence: a declared key that exceeds the bound is an error, because the user asserted it; a candidate that exceeds it is silently ineligible, like every other candidate that fails a test.

Old-side duplication still disqualifies a candidate outright. Fanout is one-directional, and a column that repeats a value in `old` cannot identify rows.

A guess still requires no nulls or `NaN`, and still requires at least one shared value.

## Explicitly deferred

* **Compound guessed keys.** Guessing remains single-column.
* **Reporting the fanout rate.** The `Diff` records the resolved key and its overlap, not the evidence behind admitting it; that belongs with the issue channel.
* **Changing what a declared key does.** The declared path keeps its counts and its error.

# Design

## Why the score has to change first

`candidate_overlap` currently counts *new rows* whose value occurs in `old`. That equals the number of shared key values only because duplicates are impossible today. Relax uniqueness without touching it and a column that fans out scores one extra point per duplicated row — so the metric would actively prefer the columns the bound exists to tolerate, and a badly duplicated column could outscore a clean one. The count must therefore become distinct shared keys before any candidate is allowed to fan out. This is a change with no observable effect today and is what makes the rest safe.

The function's contract becomes:

```rust
struct Overlap {
    /// Distinct old keys that also occur in `new`.
    shared: usize,
    /// Those that occur more than once there.
    affected: usize,
}

fn candidate_overlap(old, new, hash) -> Option<Overlap>;
```

`None` still means the column cannot identify rows at all: an invalid value on either side, or a duplicate in `old`. New-side duplication is no longer disqualifying but is now measured, which is the whole change.

## Ranking

A candidate is eligible when `shared > 0` and it satisfies the bound. Candidates are then ordered by:

1. shared keys, descending;
2. whether they fan out — `affected == 0` before `affected > 0`;
3. old column order.

Evidence comes first: the candidate that identifies the most rows across the two files wins, and freedom from fanout only settles a tie. A true key that duplicated one row therefore beats a coincidentally unique column that shares far less, which is the case this step exists to fix.

This changes guesses on inputs that resolve today, deliberately. Two kinds of comparison move: one where a fanned-out candidate now outscores the clean candidate that currently wins, and one where a candidate is eligible at all because its only duplicates are of keys absent from `old`. The demo fixtures are small enough to show the effect readily; real inputs, where the true key usually shares far more values than an accidentally unique column, should see it rarely. Ranking uniqueness first would avoid the churn but would keep choosing the weaker key, which is the behavior being fixed.

The tie-break has two states rather than three. A candidate whose duplicates are all of keys absent from `old` has `affected == 0`, because those rows are additions: they cannot make a matched row ambiguous and so do not degrade the key, even though today's uniqueness test rejects them. Only fanout — a duplicate of a key that `old` also has — costs anything, so only fanout is discriminated against. Separating "globally unique" from "duplicates only new-only keys" would rank one above the other on a difference that does not affect the diff.

Note that `affected == 0` is therefore not the same as today's eligible set, and no ordering built on it could preserve today's behavior anyway.

## Overlap over distinct keys

`KeyOverlap::possible` is `min(old.num_rows(), new.num_rows())` today, and `shared` is about to become a count of distinct keys. Mixing the two would make the ratio incoherent as soon as `new` duplicates, so `possible` becomes the smaller of the two sides' distinct key counts. `old` is unique by eligibility, so its distinct count is its row count, and neither number moves unless `new` duplicates — the reported overlap changes only where a duplicating candidate is chosen, which is new behavior anyway.

# Verification

* `src/key.rs` unit tests cover: a fanned-out candidate chosen when it is the only eligible one; a fanned-out candidate that shares more keys chosen over a clean candidate that shares fewer, which is the ranking change stated as its purpose; a clean candidate winning a tie on shared keys against a fanned-out one; a candidate rejected for exceeding the bound while a weaker clean candidate is still chosen; a candidate whose duplicates are all of new-only keys being eligible with `affected == 0`; and old-side duplication still disqualifying.
* Two tests pin the scoring change, because a ranking test alone can be passed by the wrong metric. The first asserts `Overlap { shared, affected }` directly for a fanned-out column, which is the counts themselves rather than their consequence. The second makes the two metrics disagree on the winner: candidate `a` shares ten keys with one of them appearing three times in `new`, for twelve matching new rows, and candidate `b` shares eleven keys with no duplicates, for eleven. Distinct-key scoring ranks `b` above `a`; the row count this step removes ranks `a` above `b`, whatever the column order.
* `tests/diff.rs` asserts a guessed key with `basis: Guessed`, a populated `rows.fanout`, and fanout cells absent from `cells` and `summary`, plus a repeated run that is structurally and byte-identical.
* `tests/cli.rs` snapshots a guessed fanout, whose first line is `col_key(guessed: [...], overlap: ...)` followed by a `row_fanout()` line.
* One test pins the overlap denominator, which the other overlap cases cannot distinguish because their row and distinct-key counts coincide. Twenty old keys against eleven new rows holding ten distinct keys, all shared, must report `KeyOverlap { shared: 10, possible: 10 }` for a ratio of 1.00; the row-count denominator would give `possible: 11` and 0.91.
* The existing guessing tests still pass unchanged, which is a result to verify rather than a property of the design: `guesses_the_single_eligible_column`, `guessing_prefers_the_largest_exact_intersection`, `guessing_breaks_ties_by_old_column_order`, `overlap_is_normalized_by_the_smaller_side`, and `guessing_skips_every_ineligible_candidate` all keep their winners under the new ranking. The `dup_new` column in the last of them stays ineligible, but now because one of its one shared key is duplicated and 100% exceeds the bound, rather than because `new` repeats a value. `guessing_never_admits_fanout` keeps passing for that same reason and so becomes misleading; it is rewritten to say that the candidate exceeded the bound rather than that guessing forbids fanout.
* The demo pair `demo/guessed-fanout-*.parquet` has one non-unique second column, so the fanned-out key is the only eligible candidate and the demo shows a guess that fans out.

# Definition of done

This step is complete when:

* `design.md` describes guessing as admitting bounded fanout, selecting by shared key values, and using freedom from fanout only as a tie-break;
* a guessed key may identify one old row and several new rows under the same bound and the same shared predicate as a declared key, while old-side duplication, nulls, and `NaN` still disqualify a candidate;
* candidates are scored by distinct shared key values, so duplication cannot inflate a candidate's rank;
* candidates are ranked by shared keys, with fanout breaking ties and old column order breaking those, and every existing guessing test still passes unchanged;
* the reported overlap is normalized by distinct key values on each side, which changes the ratio only where a duplicating candidate is chosen;
* the demo datasets and both READMEs describe guessing as fanout-tolerant; and
* the full test suite, strict Clippy, formatting, and diff checks pass across the workspace, and repeated runs still produce byte-identical output.
