---
title: The remaining hint kinds
---

# Todo

- [ ] **Give a hint a shape that fits all four kinds.** `HintClaim` carries the names as written — one name, or an old-to-new pair — rather than always two, so `col_drop(a)` reports as `col_drop(a)`. `HintKind` gains `Add`, `Drop`, and `Edit`. `DiffError::UnsupportedHintKind` goes: with the vocabulary complete there is nothing left for it to name.
- [ ] **Teach `ColumnMap` that an endpoint can be reserved unmatched.** A pair is not the only thing a hint can assert. `col_drop` and `col_add` reserve one endpoint as having no partner, which the map must hold and must refuse to pair, exactly as it refuses an endpoint already paired.
- [ ] **Generalise the contest to claims rather than renames.** Each hint asserts something about each endpoint it names — paired with a particular partner, paired with an unknown one, or unmatched. Two hints conflict when they assert different things about one endpoint, which subsumes the rename-versus-rename rule the code has now and adds every cross-kind shape at once.
- [ ] **Resolve `col_edit` to the identity it attaches to.** An edit reserves nothing; it names an identity by one or both of its ends, and the identity may not exist until inference has run. Resolution records the endpoints, and attachment waits.
- [ ] **Keep reserved endpoints out of rename inference.** `rename::infer` draws its candidates from the provisional drops and additions, which is exactly where a reserved endpoint sits. It gains the map and filters both candidate lists, which is the performance argument the design makes for these two kinds as well as the correctness one.
- [ ] **Protect an edited identity from swap inference too.** `swap::eligible` already excludes a pair the map records as hinted, and that exclusion stays exactly as it is. An edit hint protects an identity it does not own and so is not in the map, so eligibility gains a second question beside the first: a pair is ineligible when the map has it hinted *or* a resolved edit attaches to it.
- [ ] **Validate and apply edits after the cells are known.** Attach each edit to its final identity, reporting `hint_unresolved_identity` where there is none and `hint_no_change` where the identity changed in neither type nor value. Surviving edits force their column into the edit set, taking their cells out of the row summary.
- [ ] **Order the issues by the hints they concern.** Issues now arise in two phases either side of the whole comparison. A reader scanning the hints they supplied should find the complaints in that order, not in phase order.
- [ ] **Render the new lines.** `hint_ignored()` gains `unchanged` and `unresolved` as reasons, and a single-name claim renders with one name.
- [ ] **Accept the kinds on the command line.** No new flags: `--hint` and `--hints` already carry whatever the parser accepts, so this is the acceptance test that they do.
- [ ] **Cover the machinery.** Unit tests in `src/hint.rs` for each kind's resolution and for every cross-kind conflict shape; in `src/summary.rs` for a forced column; integration coverage in `tests/diff.rs` for replacement chosen over rename, an edit overriding a swap, an edit overriding the row summary, and both new issues; CLI snapshots in `tests/cli.rs`.
- [ ] **Refresh the demo datasets and documentation.** Add a `demo/replace-*.parquet` pair whose columns inference would otherwise identify as a rename, describe it and the two `col_edit()` uses in `demo/README.md`, and document all four kinds in `README.md`.
- [ ] **Complete the acceptance pass.** Run `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`, and confirm repeated runs still produce byte-identical output.

# Goal

`col_rename` was the hint kind that could not be done without: no evidence connects a column renamed and rewritten at the same time, so without an instruction that identity is unreachable. The other three are different. Each of them contradicts a conclusion the tool reached on its own, and reaches it wrongly.

Inference identifies a dropped column with an added one whenever their values agree. Usually that is right, and sometimes it is a coincidence about two columns that have nothing to do with each other:

```console
$ data-diff demo/replace-old.parquet demo/replace-new.parquet --key id
col_key([id], basis: declared)
col_rename(region -> zone)

$ data-diff demo/replace-old.parquet demo/replace-new.parquet --key id \
    --hint 'col_drop(region)' --hint 'col_add(zone)'
col_key([id], basis: declared)
col_drop(region)
col_add(zone)
```

Two hints rather than one, and deliberately so. Either alone would produce both lines, the unreserved endpoint having nothing left to pair with, but a replacement is two assertions and the design reads them as one deliberate pair rather than as a contradiction.

`col_edit` overrides the other two ambiguities the design names. A swap is inferred when two same-named columns each hold what the other used to; that is usually the better account, and where it is not, saying one of the columns was edited is enough to withdraw it:

```console
$ data-diff demo/swap-old.parquet demo/swap-new.parquet --key id --hint 'col_edit(price)'
col_key([id], basis: declared)
col_edit(cost, values)
col_edit(price, values)
```

And a rectangular change can be summarized by rows or by columns; where the tool chose rows and the user knows the change was to a column, the hint says which:

```console
$ data-diff demo/scatter-old.parquet demo/scatter-new.parquet --key id --hint 'col_edit(a)'
col_key([id], basis: declared)
col_edit(a, values)
col_edit(c, values)
```

Each of these is an instruction that withdraws an inference, which is why all three wait until the inferences existed to be withdrawn. What they share with `col_rename` is everything else: the grammar, endpoint resolution, deduplication, conflict rejection, and the issue channel, all built in the previous step to be extended here rather than rebuilt.

# Scope

## What changes

* `src/hint.rs`: three more kinds acted on, endpoint claims generalised beyond renames, and `col_edit` resolution and later validation.
* `src/schema.rs`: `ColumnMap` holds reservations as well as pairs, and `reconcile_schema` leaves a reserved endpoint unmatched.
* `src/rename.rs`: reserved endpoints leave the candidate lists.
* `src/swap.rs`: an edited identity joins the hinted one in being ineligible.
* `src/summary.rs`: a hinted column is forced into the edit set beside a retyped one.
* `src/model.rs`: `HintClaim`'s names, `HintKind`'s three new kinds, two new `IssueKind`s, and one `DiffError` removed.
* `src/human.rs`: single-name claims and two new reasons.
* `src/lib.rs`: edit validation after cells, and issue ordering.
* `tests/diff.rs` and `tests/cli.rs`.
* `examples/generate_demo.rs`, `demo/README.md`, and `README.md`.

## What stays and why

`design.md` needs no amendment. Its "Initial hint processing" section describes this step almost line for line, including the conflict shapes, the reading of `col_add` beside `col_drop` as replacement, and the two issue kinds added here; the same is true of the sentence in edit summarization that forces a hinted column into the edit set. The one thing worth checking against it at the end is that all five hint issue kinds it names now exist.

`--hint` and `--hints` do not change. The library owns parsing, the CLI passes spellings through, and a kind the parser learns is a kind the flags accept.

No threshold moves. A hint withdraws an inference; it does not retune the one that made it.

## Explicitly deferred

* **Folding `SchemaMatches` into `ColumnMap`.** The next queue entry owns that merge, along with recording where each identity came from. This step passes the map into `rename::infer` the way `swap::infer` already takes it, which is the shape that merge will remove — worth doing twice rather than doing the merge early and mixing two changes.
* **`invalid_key` as an issue.** Still fatal, still the row-number fallback entry's business.
* **The incompatible same-name pair.** A `col_drop` on such a column would happen to route around the fatal error, since a reserved endpoint is never compared. That is a side effect, not a fix, and the queue entry that settles the reading stands.
* **Budgets and sampling.** Reserved endpoints shrink the candidate lists rename inference walks, which is a real cost saving and not a bound.

# Design

## What each kind asserts

The four kinds make three different kinds of claim against the bijection, and the differences matter more than the count:

* `col_rename(old -> new)` claims both endpoints, as a pair. This is the previous step's work and does not change.
* `col_drop(old)` and `col_add(new)` claim one endpoint each, as having no partner. This is new: the map has held only pairs so far, and "reserved unmatched" is a second thing an endpoint can be.
* `col_edit(...)` claims no endpoint at all. It attaches to an identity that something else established, and its whole effect is on stages that run later.

A reservation has to be held in the map rather than applied as a filter at each stage, for the same reason the previous step moved hint identities into the map: an endpoint that two stages must agree about is one the map should own. Reconciliation then needs no rule about hints — a reserved old column is skipped by name matching because the map refuses to pair it, and falls out as a drop because the map has no pair for it, which is already how every drop is derived.

## The contest, generalised

The current conflict rule is about renames: an endpoint wanted twice, or wanted by a claim when the map holds it for someone else. Adding three kinds could mean adding the design's list of cross-kind shapes one at a time — an old endpoint both renamed and dropped, a new endpoint both renamed and added, an edit contradicted by an add or a drop. It should not, because they are all one shape.

Read every claim as a statement about each endpoint it names:

| Claim | About its old endpoint | About its new endpoint |
|---|---|---|
| `col_rename(a -> b)` | paired with new `b` | paired with old `a` |
| `col_drop(a)` | unmatched | — |
| `col_add(b)` | — | unmatched |
| `col_edit(a -> b)` | paired with new `b` | paired with old `a` |
| `col_edit(a)` | paired with new `a` | paired with old `a` |

Two hints conflict when they say different things about one endpoint: paired with two different partners, or paired against unmatched. Two hints that say the same thing about the same endpoints are duplicates and collapse. Nothing else is a conflict, which is why `col_drop(a)` beside `col_add(b)` is fine — they name endpoints in different halves of the bijection and never meet.

That rule subsumes the rename-versus-rename check rather than sitting beside it, and it produces every shape the design lists without any of them being written down. It extends to the map the same way it does today: a claim conflicts with what the map already holds when the map says something different about one of its endpoints, which is how a key component keeps beating a hint without either knowing about the other.

Group rejection is unchanged. Claims that share an endpoint are reported as one set of rivals, and the whole group goes rather than input order choosing a winner.

## What a `col_edit` names

An edit hint has to name an identity, and an identity has two ends that may carry different names. `col_edit(a)` names the identity with `a` at both ends; `col_edit(a -> b)` names the one from old `a` to new `b`. Both spellings are the ones the format prints for the corresponding rename, so the single form is what a user types for the ordinary case and the pair form covers a renamed column.

Where only one side has the name, the hint still resolves: `col_edit(a)` with no new `a` names an identity whose old end is `a`, whatever the other end turns out to be. That is what lets a `col_edit()` line printed about an inferred rename be fed back in, since such a line carries only the new name. Both sides missing is `hint_missing_target` like any other.

This is the one place the strict reading might annoy. `col_edit(a)` alongside `col_rename(a -> c)`, with a new `a` also present, is a conflict: the edit says old `a` pairs with new `a` and the rename says it pairs with new `c`. The user meant the renamed column and must write `col_edit(a -> c)`. The alternative — letting a single name mean "whatever this end pairs with" even when the other side has that name too — would make `col_edit(price)` ambiguous in exactly the swap case the kind exists for, which is a worse trade than an occasional rejected spelling.

Attachment happens after everything else. The hint matches the final identity that agrees with every endpoint it resolved: both, if it resolved both, and otherwise the one identity holding the end it knows. No such identity is `hint_unresolved_identity` — the column was dropped, or added, or ended up paired with something else.

## When an edit takes effect

The three things a `col_edit` does happen at three different times, and it cannot be otherwise:

1. **Conflict detection**, with the other hints, before anything is resolved. It claims no endpoint but it does assert an identity, and an assertion can contradict one.
2. **Swap protection**, before `swap::infer`. This is the design's stated purpose for the kind and it must be in place before the inference it withdraws. It is added to the protection already there rather than substituted for it: `design.md` requires every hinted identity to be safe from swaps, and a rename hint's identity is protected because the map records it as hinted. An edit is never in the map, having claimed no endpoint, so it needs a second question — but the first one is still load-bearing, and dropping it would let a swap overrule an accepted `col_rename`, which is the one thing that section of the design forbids by name.
3. **Validation and forcing**, after cells are compared. Whether the identity exists, and whether anything about it changed, are not knowable earlier.

Only the third can report, which is why `hint_no_change` and `hint_unresolved_identity` arrive after the comparison has finished while every other issue arrives before it starts. The output must not show that seam: issues render in the order of the hints that caused them, so a user reads their complaints in the order they wrote their instructions. `Diff::issues` is ordered the same way, since a consumer has the same expectation and there is no second ordering worth having.

Forcing is a small change to summarization and not a new mechanism. `summarize` already holds retyped columns out of the optimization and emits them as column edits; a hinted identity joins them on the same terms, its cells leaving the row graph so the row edits recompute around it. That is what makes the `scatter` fixture's `row_edit(1)` become a second `col_edit()`: the minimum cover is minimum over what is left to cover.

## What is not reported

A `col_drop` that changed nothing is not an issue. `col_drop(a)` where old `a` had no partner anyway looks redundant, and is not: it kept the column out of rename inference, which is a decision even when the outcome matches. Redundancy is also not knowable at the point the other issues are raised, and inventing a fourth reporting phase to say "that hint agreed with me" would be noise.

Nor is a `col_drop(a)` beside a surviving new `a`. Reserving the old endpoint leaves the new column with nothing to match, so the output carries a drop and an addition of the same name — which is the assertion, spelled exactly: these two columns are not the same column. `hint_no_change` belongs to `col_edit`, where the design defines it, and means the identity did not change rather than that the hint did not.

# Verification

* `src/hint.rs` unit tests: each of the four kinds resolving; a single-name and a pair `col_edit` naming the same identity; a `col_edit` naming a column present on only one side; a drop and an add reserving their endpoints; and identical claims of each kind collapsing.
* One test per cross-kind conflict shape, driven from the table above: renamed and dropped, renamed and added, edited and dropped, edited and added, and an edit contradicting a rename. Each rejects its whole group, and one test confirms `col_drop(a)` beside `col_add(b)` is not among them.
* `src/swap.rs`'s existing `a_hinted_identity_is_not_reinterpreted_as_a_swap` keeps passing unaltered, which is the check that edit protection was added to the map exclusion rather than put in its place. A second test beside it covers an edit hint protecting an identity the map knows nothing about.
* `src/summary.rs` gains a test that a forced column is emitted as a column edit and that the row summary recomputes around it, which is the claim that forcing goes through the optimizer rather than around it.
* `tests/diff.rs` asserts a complete `Diff` for: a replacement chosen over an inferred rename, with the columns unmatched and no cells; an edit hint withdrawing a swap, with both identities same-named and both edited; an edit hint turning a row summary into a column one; a `col_edit` on an unchanged column reporting `hint_no_change` while the diff still reports `no_changes()`; and a `col_edit` on a column that ended up dropped reporting `hint_unresolved_identity`.
* One test supplies a well-formed hint of each kind together in one invocation and asserts they do not interfere, group rejection being local.
* One test pins the issue ordering: two hints that fail in different phases, supplied in each order, report in supply order both times.
* `tests/cli.rs` snapshots a replacement, an edit overriding a swap, and an ignored edit, confirming the exit status stays zero.
* A rendered `col_edit()` line about an inferred rename is fed back in as a hint and accepted, extending the previous step's round-trip claim to the kind whose printed line carries one name for a two-named identity.
* The `hint_ignored()` fixtures in the field-name guard grow to cover the new reasons, so the fixed field-name set keeps its hold on every line the format can write.
* Repeated runs of every new fixture are structurally and byte-identical.

# Definition of done

This step is complete when:

* `col_add` and `col_drop` reserve an endpoint as unmatched, keeping it out of name matching, rename inference, and swap inference, and a pair of them reads as replacement rather than as a contradiction;
* `col_edit` attaches to an identity without reserving anything, withdraws a swap, and forces its column into the edit set;
* conflicts are detected across all four kinds by one rule about what a claim asserts of an endpoint, rather than by a list of shapes, and still reject their whole connected group;
* `hint_no_change` and `hint_unresolved_identity` reach the `Diff`, completing the five kinds `design.md` names, and render as `hint_ignored()` reasons;
* issues report in the order the hints were supplied, whichever side of the comparison they were raised on;
* `DiffError::UnsupportedHintKind` is gone, every kind the parser reads now being one the tool acts on;
* the demo datasets and both READMEs describe replacement and both uses of `col_edit`; and
* the full test suite, strict Clippy, formatting, and diff checks pass across the workspace, and repeated runs still produce byte-identical output.
