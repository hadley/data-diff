---
title: Approximate renames and swaps
---

# Todo

- [x] **Measure agreement in one place.** Add `src/agreement.rs`, moving the aligned projection cache out of `src/rename.rs` and extending it to count, for a candidate pair under its comparison plan, the aligned rows, the rows that agree, and the expected-agreement numerator. Derive `Hash` on `CanonicalValue` in `src/compare.rs` so canonicalized values can be counted, and express every threshold as an exact integer inequality rather than a floating-point one. The thresholds both inference stages share live here beside the predicates that apply them.
- [x] **Decide what an uninformative exact pair means.** The queue asked for an information-content requirement so that two all-null columns stop being paired. Under review that was cut back to an ambiguity rule: a pair whose values narrow nothing down is accepted when it is the only exact match available to both of its ends, and rejected when several candidates compete. The reasoning is in the design section below.
- [x] **Infer approximate renames.** Among the candidates exact inference left behind, accept a compatible pair when more than 90% of the aligned rows agree and chance-corrected agreement exceeds 0.8. Accept only mutually unique candidates; leave overlapping ones as drops and additions.
- [x] **Infer swaps.** Add `src/swap.rs`. Two non-key identities whose ends carry the same name are a swap candidate when each identity agrees in fewer than half of the aligned rows and both cross-pairs have identical types and pass the approximate thresholds. Accept a candidate only when each of its two identities belongs to exactly one candidate, and rewrite both identities atomically. The module reads and rewrites `identities` and never touches `dropped` or `added`.
- [x] **Document the absence of a row minimum.** The design's 20-row minimum is deliberately not implemented, so `design.md` carries the argument for declining it, and `MIN_AGREEMENT_PERCENT` in `src/agreement.rs` carries what the thresholds imply in its place: eleven aligned rows before one disagreement can be tolerated, and no floor at all for a pair that agrees everywhere.
- [x] **Run the new stages in the pipeline.** `rename::infer` runs exact then approximate inference where `infer_exact` is called now, and `swap::infer` follows it as its own line in `src/lib.rs`.
- [x] **Amend `design.md`.** Settle the "initially" in "Exact renames": neither a minimum row count nor an information-content requirement is wanted, and the ambiguity rule replaces the latter. Drop the 20-row minimum from "Approximate renames" and "Swaps", along with the `approximate_rename_insufficient_rows` issue that only existed to report it, and record why. Promote "Swaps" from a subsection of "Rename inference" to its own reconciliation step, renumbering the outline in "Reconciliation" to match. Call the stage swap inference rather than swap detection, in that section and in "Column identity model".
- [x] **Cover the measurement.** Unit tests in `src/agreement.rs` for the counts themselves, for nulls counting as an ordinary category on both sides, for the integer thresholds at and either side of their boundaries, and for a constant pair being rejected rather than dividing by zero.
- [x] **Cover the inference.** Unit tests in `src/rename.rs` for an approximate rename found and one rejected by each of the three requirements separately, and for ambiguous approximate candidates left unresolved from either end. Unit tests in `src/swap.rs` for a swap that dissolves the type changes its same-name readings reported, one rejected because a crossing would itself change type, one rejected because a third identity competes for it, and one not proposed between identities that inference itself established. Integration coverage in `tests/diff.rs` and CLI snapshots in `tests/cli.rs` for both an approximate rename and a swap.
- [x] **Refresh the demo datasets and documentation.** Add `demo/approx-rename-*.parquet`, which needs at least eleven rows for an imperfect match to clear $p_o > 0.9$ at all, and `demo/swap-*.parquet`, which needs only enough rows to read. Describe both in `demo/README.md`, which is where the worked examples live; the top-level `README.md` keeps its operation table without a prose account of inference.
- [x] **Complete the acceptance pass.** Run `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`, and confirm repeated runs still produce byte-identical output.

# Goal

Exact inference identifies a renamed column only when its values survived the rename untouched. A column that was renamed *and* edited in the same change — the ordinary case when someone renames a field and fixes a few of its values — still reads as an unrelated drop and addition. This step accepts the pair when the evidence is strong but imperfect:

```console
$ data-diff demo/approx-rename-old.parquet demo/approx-rename-new.parquet --key id
col_key(declared: ["id"])
col_rename("amount" -> "total")
row_edit(7)
```

The single disagreement is no longer proof that these are different columns; it is the edit the rename carried with it, and it is reported as one.

The same measurement answers a second question. When two same-named columns both change beyond recognition, the likeliest explanation is not that both were rewritten but that their contents were exchanged:

```console
$ data-diff demo/swap-old.parquet demo/swap-new.parquet --key id
col_key(declared: ["id"])
col_rename("price" -> "cost")
col_rename("cost" -> "price")
col_order("price", 3 -> 2)
```

Two `col_edit()` events, each saying that nearly every value in a column changed, become two renames that say what actually happened. The `col_order()` line follows from the bijection rather than being asserted separately: the column holding the price values is second in the new file and was third in the old one.

Finally, the same measurement settles a question the previous step left open. A column of one repeated value agrees with another perfectly without that agreement narrowing anything down, and the queue entry asked for such pairs to be rejected outright. They are not: one rename accounts for the evidence that a drop and an addition spend two operations describing, and parsimony is how the design breaks exactly this kind of tie. What the measurement is used for instead is ambiguity — several constant columns all match each other, and choosing between them in column order would invent a relationship rather than resolve a tie.

# Scope

## What changes

* `design.md`: what an uninformative exact pair means in "Exact renames", "Swaps" promoted to its own reconciliation step, and the stage renamed to swap inference.
* `src/agreement.rs`: new — the aligned projection cache, the agreement counts taken from it, the shared thresholds, and the predicates over those counts.
* `src/compare.rs`: `CanonicalValue` derives `Hash`.
* `src/rename.rs`: the aligned cache moves out, exact inference gains its ambiguity rule for uninformative pairs, and approximate inference joins it behind one entry point.
* `src/swap.rs`: new — swap candidacy, mutual uniqueness, and the atomic rewrite of two identities.
* `src/lib.rs`: `rename::infer_exact` becomes `rename::infer`, and `swap::infer` follows it.
* `tests/diff.rs` and `tests/cli.rs`.
* `examples/generate_demo.rs` and `demo/README.md`.

## What stays and why

This implements the "Approximate renames" and "Swaps" sections of `design.md` using the thresholds they state: $p_o > 0.9$, $\kappa > 0.8$, and $p_o < 0.5$ for a swapped column's own name pair. It does not implement their 20-row minimum, for the reasons the design section below records; that is the third `design.md` amendment. Those sections also describe deterministic sampling and a pair budget, which belong to the benchmarking step and are deferred below; the queue entry for this step says the same, asking for all matched rows initially.

`design.md` takes three amendments. Two of them record settled decisions without changing what it asks for; the third changes a threshold.

The first is in "Exact renames", whose closing sentence says there is "initially" no minimum row-count or information-content requirement. That word was a promise to come back, and this is the step that does. The answer turns out to be that neither requirement is wanted, so the sentence is replaced by the reasoning rather than by a rule: complete agreement needs no correcting for chance, and rejecting a pair for saying little costs a parsimonious reading that nothing contradicts. What is added is the ambiguity rule that concern really points at.

The second removes the 20-row minimum from "Approximate renames" and "Swaps", and with it the `approximate_rename_insufficient_rows` issue, which existed only to report a refusal that no longer happens. The design section below gives the reasoning; `design.md` records the conclusion and the fact that a minimum was considered, so that a future reader does not reintroduce one by assuming it was never thought about.

The third is structural. "Swaps" is currently a subsection of "Rename inference", which reads as though a swap were a kind of rename inference; it is not. Rename inference consumes unmatched columns and produces identities, while a swap consumes two identities and exchanges their ends, and the two cannot interact for the reasons the module section gives. So "Swaps" is promoted to its own step, and the numbered outline in "Reconciliation" — which today runs from "Detect column renaming" straight to reordering — gains a step for it and renumbers. The shared vocabulary is unaffected: both still produce a `col_rename()`, which is what the nesting was recording and what the "Vocabulary" table already says.

Nothing in the human format changes, for the third time and for the same reason: an approximate rename is one identity whose ends carry different names, and a swap is two of them. `col_rename()` already renders such an identity, so the work is establishing the identities, not describing them. That a swap also produces a `col_order()` entry, and that an approximate rename's disagreements flow into cells and then into the minimum cover as ordinary edits, are consequences of the bijection that need no new code and are worth a test each precisely because of that.

Without the row minimum, the two new stages are no longer out of reach of the existing fixtures by construction, so the argument that they leave the suite alone has to be made on their rules instead.

Approximate rename inference still cannot touch anything: it needs a drop and an addition in the same diff over at least eleven aligned rows, and every fixture that has both is far smaller. Swap inference has no size floor at all, so its reach is bounded only by what it asks for — two same-named non-key columns whose values agree crosswise in more than 90% of aligned rows while agreeing straight in fewer than half. That is a demanding coincidence: a fixture would have to hold values that line up under the exchange and fail to line up without it, which is the signature of an actual swap rather than something a fixture stumbles into. The closest thing in the suite, the `src/cells.rs` fixture whose columns are reordered and edited, disagrees on both cross-pairs as well as both straight pairs, and is a unit test that never reaches inference in any case.

That is an argument, not a proof, which is why the acceptance pass runs the whole suite and the verification section keeps the requirement that every other existing snapshot and assertion passes unchanged. The ambiguity rule reaches existing fixtures more directly, and none is affected: the all-null pairing the previous step pinned keeps its result, having no competitor.

## Explicitly deferred

* **Deterministic sampling and pair budgets.** Approximate inference compares every remaining drop against every remaining add, and swap inference every eligible identity against every other, each over all matched rows. Both are quadratic in candidates and linear in rows, and both are named in the benchmarking queue entry. Exact inference runs first and removes its pairs from the candidate lists, which is a real reduction but not a bound.
* **Hint exclusions.** Approximate candidates would exclude endpoints reserved by `col_add`/`col_drop` hints, and swap candidates would exclude identities protected by `col_edit` hints. Hints do not exist yet, and their queue entries carry the requirement.
* **Rotations longer than a swap.** Only two-column exchanges are inferred, as the design specifies. Three columns rotating remain three edits.
* **Assignment algorithms.** Overlapping approximate candidates and competing swaps stay unresolved rather than being resolved by scoring. The design leaves them to the user, and the hint kinds that let a user resolve them are the next two queue entries.

# Design

## Agreement, counted in integers

Every question this step asks about a pair of columns is answered by three counts over the aligned rows: $n$, the number of aligned rows; $m$, the number of them where the two canonicalized values are equal; and $S = \sum_v c_{old}(v) \, c_{new}(v)$, taken over the canonicalized value counts on each side. The proportions the design is written in follow directly, with $p_o = m/n$ and $p_e = S/n^2$.

Keeping the counts rather than the proportions is what makes the result deterministic. Expected agreement is a sum over a frequency map, and floating-point addition is not associative, so summing the terms in hash order would let the last bit of $p_e$ depend on the iteration order of a `HashMap` — and a threshold comparison could in principle follow it. Integer addition has no such property, so `S` is accumulated in `u128` and every threshold is then rearranged into an exact integer inequality:

| Design condition | Integer form |
|---|---|
| $p_o > 0.9$ | $100 m > 90 n$ |
| $p_o < 0.5$ | $100 m < 50 n$ |
| $p_e = 1$ | $S = n^2$ |
| $\kappa > 0.8$ | $100 (mn - S) > 80 (n^2 - S)$ |

The last line is $\kappa > k$ multiplied through by $n^2 (1 - p_e)$, which is positive exactly when $p_e < 1$, so the rearrangement is faithful once the $p_e = 1$ case is rejected first — which is what the design asks for anyway, $\kappa$ being undefined there. The percentages stay named constants in the style of `MAX_FANOUT_PERCENT`, so the design's "deliberately conservative and tunable" is tunable in one place.

Values are counted as they compare: canonicalized under the pair's own plan, with null an ordinary category alongside the rest, which is what the design requires of both $p_o$ and $p_e$.

## What the aligned cache becomes

`Aligned` already projects a candidate column onto the matched rows and caches the result per column and plan, because canonicalization depends on the pair rather than on the column. It moves to `src/agreement.rs` and its cache entry grows a frequency map beside the values it already holds, so a column that takes part in several candidate pairs is canonicalized, projected, and counted once per plan. Exact inference keeps its digest, and therefore keeps its property that only equal-digest pairs are ever compared elementwise; the new stages have no equivalent shortcut and compare each candidate pair directly.

## Information content, and what it is actually good for

An exactly equal pair has $p_o = 1$, so its $\kappa$ is 1 whenever it is defined at all, and undefined exactly when $p_e = 1$. That algebra makes it tempting to give exact inference the requirement the approximate rules already state — reject when $p_e = 1$ — and this plan originally did. Review rejected it, and the objection is right.

The case is `old.a` holding `true` in every row and `new.b` holding `true` in every row. Nothing about that contradicts a rename, and `col_rename(a -> b)` accounts for it in one operation where `col_drop(a)` plus `col_add(b)` spends two. Parsimony is the design's own tie-breaker for exactly this kind of ambiguity, listed in its vocabulary section, and rejecting the pair does not make the tool more careful — it makes it noisier about a change it could have explained. The reason the approximate rules reject $p_e = 1$ does not carry over either: chance correction exists to tell an imperfect match how much of its agreement was luck, and complete agreement has no such question to answer.

What survives the objection is ambiguity. When values narrow nothing down, every constant column matches every other, so the column-order tie-break stops being a choice between indistinguishable answers and becomes the invention of one relationship out of many. Informative columns can tie too, but their evidence is complete on both sides and ties are incidental; among constants, ties are the normal case. So an uninformative exact pair is accepted only when it is the only exact match available to both of its ends — the same mutual uniqueness approximate inference uses, applied for the same reason.

This costs exact inference its early exit: ambiguity is a property of the whole candidate set, so every exactly agreeing pair is collected before any is taken. The pairs are found by digest as before, so this adds bookkeeping rather than comparisons.

## Approximate renames

The candidates are the drops and adds that exact inference left, walked in column order on both sides. A pair qualifies when its types are compatible, $p_e < 1$, $p_o > 0.9$, and $\kappa > 0.8$. There is no row minimum; the section below records why, and why $p_o > 0.9$ supplies an implicit one anyway.

Acceptance is mutual uniqueness, not first match: a pair is accepted when the old column qualifies with that new column and no other, and the new column qualifies with that old column and no other. This is a stronger rule than exact inference's, and deliberately so — the design resolves ambiguous *exact* matches in column order because the evidence is complete and the choice is arbitrary, while ambiguous approximate matches differ in how well they match, and picking the first is picking against evidence we have chosen not to weigh. Overlapping candidates stay drops and additions.

## Why there is no row minimum

The design requires at least 20 aligned pairs before approximate inference will propose anything. This step does not implement it, by decision of the owner, and the reasoning belongs in the code rather than only here — hence the checklist item putting it beside the predicates in `agreement.rs`.

The minimum was narrower than it looks. Requiring $p_o > 0.9$ with at least one disagreement already forces eleven aligned rows: at $n = 10$ the best imperfect pair agrees in nine, which is exactly $0.9$ and not above it. Below eleven rows approximate inference can only re-derive a pair that agrees everywhere, which exact inference has already settled on its own terms. So a 20-row minimum decided nothing until $n = 11$, and everything it decided lay between eleven and nineteen rows.

What it bought in that band was control over variance, which $\kappa$ does not provide, being a point estimate with no notion of sample size. Two unrelated balanced boolean columns over eleven rows have $p_e \approx 0.5$; agreeing in ten of them gives $p_o \approx 0.909$ and $\kappa \approx 0.82$, which passes. Independent coin flips agree that closely about six times in a thousand. The judgement is that this costs little in practice: it needs low-cardinality columns, a table with barely more than ten rows, and a candidate list long enough for a rare event to appear somewhere in it, and the three rarely coincide. Against that, a minimum silently withholds a correct rename from every small table, which is the more common case and the one a user is more likely to notice.

The asymmetry between the two stages is worth recording, because it is the part that is not obvious. For approximate renames the $p_o$ floor above means removing the minimum changes behavior only between eleven and nineteen rows. Swap inference has no such floor: its cross-pairs may agree *perfectly*, and a perfect cross-match satisfies $p_o > 0.9$ at any size, so two columns whose values are exactly exchanged are now inferred as a swap in a two-row table. That is consistent with exact rename inference, which has never had a minimum and accepts a two-row rename on the same evidence, and it is defensible for the same reason: an exact exchange is not a coincidence that thresholds are protecting us from. It does mean swap inference reaches small fixtures that no previous stage reached, which the fixture note in the scope now accounts for.

## Swaps

Swap inference has its own module, `src/swap.rs`, for the reasons the next section gives; what follows is its rule.

The stage infers, and is named for it. `design.md` calls this one "swap detection" while calling its neighbours rename inference, and the asymmetry is not principled: a swap is no more observed than a rename is. Both weigh evidence against thresholds and can be wrong, which is precisely what the word "infer" admits and what "detect" quietly denies. The codebase already draws the line the other way and correctly: `detect_order` computes a longest common subsequence over identities that are already resolved, with no thresholds and nothing to be wrong about. So the stage is `swap::infer`, and `design.md` is amended to match rather than the code being made to match `design.md`.

A swap candidate is a pair of identities, not a pair of columns, and the identities eligible for it are the provisional ones: not a key, and with the same name at each end. That test excludes exactly the right things without a new field on `ColumnIdentity`. A paired key component is a key. An identity established by exact or approximate inference has different names at its ends, having been established from evidence this stage would only be second-guessing. A future rename hint will also have different names, and a future edit hint will need the exclusion the deferrals list.

Two eligible identities $A$ and $B$ form a candidate when each of $A$ and $B$ agrees in fewer than half of the aligned rows, both cross-pairs have identical source types, and each cross-pair satisfies the same $p_e < 1$, $p_o > 0.9$, $\kappa > 0.8$ that an approximate rename does. The type requirement is stricter than rename inference's, which asks only for compatibility: a cross-type rename relates columns that would otherwise be a drop and an addition, related not at all, while a swap overrides an identity that name matching already established and so answers to a higher bar. A swap therefore never carries a type change. Candidates are enumerated over unordered pairs of eligible identities in column order. With no matched rows there is nothing to measure and the stage is skipped, as rename inference already is.

A candidate is accepted only when each of its two identities takes part in exactly one candidate, so competing swaps cancel rather than compete. Acceptance rewrites both identities in place: $A$'s old end pairs with $B$'s new end and $B$'s old with $A$'s new, `type_changed` is recomputed from the new endpoints, and neither is a key. Old positions do not move, so the identity list stays sorted and `minimal_moves` keeps its precondition.

## Where the stages run, and why swaps are their own module

Only one ordering here is forced. Exact inference must precede approximate inference, both because the design says approximate inference considers what exact inference left and because it is a real dependency: the two draw from the same candidate lists, and a pair that agrees everywhere would otherwise be decided by whichever rule reached it first. So those two live together in `rename.rs` behind `rename::infer`, which owns the candidate lists and re-sorts `identities` by old position once at the end.

Swap inference is not in that relationship with either, which is what earns it a module of its own. Its candidates are provisional same-name identities; the other two stages consume dropped and added columns and produce identities whose ends carry different names. Those sets cannot intersect, because `reconcile_schema` matches equal names before anything else, so a dropped column and an added column never share a name, and an inferred identity is therefore never a same-name one. Swap inference also rewrites entries of `identities` in place rather than moving anything out of `dropped` or `added`, so it frees nothing the other stages could consume and appends nothing that would disturb their sort.

Keeping it separate makes those properties checkable instead of argued. A reader of `swap.rs` can see that `dropped` and `added` are never named in it and that no identity is added or removed, which is the disjointness this section claims; a reader of `rename.rs` sees the two stages that genuinely constrain each other and nothing else. The alternative — one `infer` running all three — would put a stage that commutes with everything in the same function as two that do not, and the only thing holding the reader to the right mental model would be this paragraph. This is the same split the `design.md` amendment above makes, and for the same reason.

`src/lib.rs` therefore reads `rename::infer` then `swap::infer`, both after `reconcile_schema` and before `detect_order` and `compare_cells`, so ordering and cells see the final bijection. The two calls are in the order `design.md` lists, not in an order the result depends on; the `swap.rs` test that no swap is proposed between identities inference itself established pins one half of that independence.

# Verification

* `src/agreement.rs` unit tests cover the three counts on a pair with known values; nulls counted as a category on both sides rather than skipped; each integer threshold at its boundary and one step either side of it, which is where a floating-point form would be at risk; and a constant pair rejected by the $p_e = 1$ test rather than reaching the $\kappa$ arithmetic.
* `src/rename.rs` unit tests cover an approximate rename accepted, and the same pair rejected with one disagreement too many and with agreement that is high but no better than chance because the column is nearly constant — each test moving one requirement, so each is separately load-bearing.
* The implicit floor is pinned rather than left as an argument in prose: a ten-row pair agreeing in nine rows is rejected, because $0.9$ is not above $0.9$, and the same pair over eleven rows agreeing in ten is accepted. Those two tests are what would fail if the threshold were ever relaxed to $\ge$, and they are the reason no separate row minimum is needed for renames.
* A swap over a two-row table is accepted, pinning the deliberate asymmetry: swaps have no implicit floor because a perfect cross-match clears $p_o$ at any size, and this is the test that says so on purpose rather than by omission.
* `src/swap.rs` unit tests cover a swap accepted, a swap rejected because a third eligible identity also forms a candidate with one of the two, and no swap proposed between identities that inference itself established. The last is the disjointness the module boundary is for, so it belongs beside the code that has to keep it.
* Mutual uniqueness is tested from both ends: two old columns approximately matching one new column, and one old column approximately matching two new ones. The two are not symmetric in the code — candidates are enumerated by walking old columns and scanning new ones — so one test would leave the other endpoint's count untested, and a rule that only counted one way would still pass it. Both cases leave all three columns unresolved.
* The accepted swap has old `price` an integer beside a string `cost`, with the new file holding them the other way round. Its crossings are integer to integer and string to string; it is the *same-name* readings that changed type, and the swap is what explains them away. Both identities therefore carry `type_changed` before the rewrite and neither does after it, so a stale flag copied from the identity being replaced fails the test, which a same-type fixture could not detect.
* The all-null test from the previous step keeps its result and gains company: a constant non-null pair is also accepted, so the rule is visibly about ambiguity rather than about nulls; two constant old columns facing two constant new ones are left unresolved, as is one facing two; and a tied pair of *informative* columns still pairs off in column order, which is the test that keeps the distinction from collapsing into "ties are rejected".
* `tests/diff.rs` asserts a complete `Diff` for an approximate rename, showing the identity established, `added` and `dropped` empty, the disagreeing rows present as changed cells, and those cells summarized as edits; and a complete `Diff` for a swap, showing both identities rewritten, the induced `col_order()` entry, and the residual cells. Both repeat the run and assert the diff and its rendering are unchanged.
* `tests/cli.rs` snapshots both cases end to end, which is again the check that no rendering work was needed.
* Every other existing snapshot and assertion passes unchanged. With the row minimum gone this is no longer guaranteed by fixture size, so it is a result the acceptance pass has to produce rather than a property the scope can assume.

# Definition of done

This step is complete when:

* a dropped and an added column that agree in more than 90% of the aligned rows, by more than chance, are identified as one renamed column, with the remaining disagreements reported as ordinary cell changes;
* two same-named, non-key identities that each agree in fewer than half their aligned rows, and whose cross-pairs meet the approximate thresholds, are rewritten as a swap, with the resulting column movement and residual edits following from the bijection;
* an ambiguous approximate candidate and a competing swap are both left unresolved rather than being decided arbitrarily;
* two columns holding one value in every aligned row are identified when nothing else competes for either of them, and left as a drop and an addition when something does;
* every threshold is evaluated in exact integer arithmetic, so no result depends on the summation order of a frequency map;
* neither new stage imposes a row minimum, `design.md` no longer asks for one and records why, and the floor the thresholds imply instead is documented on the constant that creates it;
* the demo datasets and `demo/README.md` describe inferred renames and swaps; and
* the full test suite, strict Clippy, formatting, and diff checks pass across the workspace, and repeated runs still produce byte-identical output.
