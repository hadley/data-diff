---
title: Where each column identity came from
---

# Todo

- [x] **Give a pair a basis, and let the map say how wide it is.** `ColumnPair` gains an `IdentityBasis` and the `is_key` flag `ColumnIdentity` carried, and `hinted` goes: a hinted pair is one whose basis says so. `ColumnMap` learns its two column counts, so `dropped()` and `added()` are the positions it has no pair for. `ColumnMap::new(&Schema, &Schema)` replaces `Default`, here and on `Hints`.
- [x] **Retire `SchemaMatches`.** `reconcile_schema` claims into the map it was given and returns nothing but errors; the three accounts of one bijection become one. Key columns are marked rather than re-derived from `key.columns` at the end.
- [x] **Move rename inference onto the map.** `infer` takes one `&mut ColumnMap` instead of a `SchemaMatches` and a second map to consult, draws its candidates from `dropped()` and `added()` minus the reserved endpoints, and claims each accepted pair with `Exact` or `Approximate`. Claiming in ascending old position removes the sort that put the identities back in order.
- [x] **Move swap inference onto the map.** Eligibility stops comparing names: a provisional same-name identity is exactly a pair whose basis is `Name`, which is what recording the basis was for. `ColumnMap::exchange` swaps two pairs' new ends atomically and records `Swapped` on both.
- [x] **Let ordering and cell comparison read the map, and derive `type_changed` where it is used.** No pair carries a type change any more: `compare_cells` already reads both data types to build its comparison plan, and deriving the flag there ends the recompute that `swap::rewire` had to do to keep a stale one honest.
- [x] **Carry the basis into `Diff`.** `ColumnsDiff::identities` becomes a `Vec<ColumnIdentity>` of a coordinate and its basis, `IdentityBasis` names the six sources with a word each, and the renderer reads the basis rather than working it out.
- [x] **Render the basis on every `col_rename()` line.** One field, `basis`, the one `col_key()` already carries, and no line without it.
- [x] **Read a printed line back whatever detail it carries.** A hint's claim is its first argument; every argument after it is a field, which is detail the format prints about the operation rather than part of the claim, and is ignored. This is what keeps `col_rename(amount -> total, basis: exact)` an instruction, and it makes `col_edit(value, changed: values)` one for the first time.
- [x] **Write the last bare flag as a field.** `col_edit(price, values)` and `row_fanout(4 -> [4, 5], values)` become `changed: values`, so that a bare word is never an argument and `col_edit(values)` can only be the column called `values`. That is what leaves the parser one rule with no vocabulary in it.
- [x] **Settle how a swap reads, and say so in `design.md`.** Two `col_rename()` lines, each `basis: swapped`, rather than the combined list form that section names today.
- [x] **Cover the machinery.** Unit tests in `src/schema.rs` for the derived drops and additions, the first claim's basis surviving a redundant second, marking, and exchange; in `src/rename.rs` and `src/swap.rs` for the basis each stage records; in `src/human.rs` for the rendered word and for a swap's two lines; in `src/hint.rs` for the arguments the parser now ignores and the ones it still refuses. `tests/diff.rs` asserts the basis for all six sources and re-states the round-trip claim as one about identity rather than about basis; `tests/cli.rs` snapshots follow.
- [x] **Update `design.md` and both READMEs.** The identity model records a basis, the swap section renders two lines, and the output tables show the field.
- [x] **Complete the acceptance pass.** Run `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`, and confirm repeated runs still produce byte-identical output.

# Goal

`col_rename(region -> zone)` is written the same way whether the user asserted it, a declared key pair established it, or the tool guessed it from values that happened to agree. Three of those are certainties and two are judgements, and the reader cannot tell which they are looking at. A guessed key already says so — `col_key([id], basis: guessed, overlap: 0.67)` — and an inferred rename should be held to the same standard:

```console
$ data-diff demo/approx-rename-old.parquet demo/approx-rename-new.parquet --key id
col_key([id], basis: declared)
col_rename(amount -> total, basis: approximate)
row_edit(7)

$ data-diff demo/approx-rename-old.parquet demo/approx-rename-new.parquet --key id \
    --hint 'col_rename(amount -> total)'
col_key([id], basis: declared)
col_rename(amount -> total, basis: hinted)
row_edit(7)
```

The same line, and now two different claims: the first is what the values suggest, the second is what the user said. Nothing else about the diff changes, which is the point — the basis is how an identity was reached, not what it means.

Recording it also settles a piece of untidiness the last four steps have worked around. `SchemaMatches` and `ColumnMap` are two accounts of one bijection: the map is authoritative while identity is being decided, then reconciliation copies it into a `SchemaMatches` whose `identities`, `added`, and `dropped` every later stage reads, and inference mutates all three by hand. Once a pair records where it came from and whether it is a key column, the map holds everything `SchemaMatches` held: `identities` is the pairs, and `added` and `dropped` are the positions with no pair. So the two collapse into one value that flows from hints through to cells, and the stages that maintained the second account by hand stop having one to maintain.

That merge earns its place here rather than in a step of its own because this is the step that has to touch the model anyway, and because the basis pays for itself inside it immediately: swap inference asks whether an identity is a provisional same-name one by comparing the two schema names, which is exactly the fact the basis now records.

# Scope

## What changes

* `src/schema.rs`: `ColumnPair` gains a basis and `is_key`, `ColumnMap` gains its widths, the derived `dropped()` and `added()`, `mark_key`, and `exchange`; `SchemaMatches` and `ColumnIdentity` go, and `reconcile_schema` claims into the map it is given.
* `src/rename.rs`, `src/swap.rs`, `src/order.rs`, `src/cells.rs`: one map instead of a `SchemaMatches`, plus the basis each inference records; `type_changed` derived in `cells`.
* `src/hint.rs`: `Hints` loses its `Default`, `validate_edits` takes the map, and the parser ignores every argument after the first, each of which must be a field.
* `src/key.rs`: `claimed_identities` builds a map that knows its widths and claims with `Declared`.
* `src/model.rs`: a public `ColumnIdentity` and `IdentityBasis`, and `ColumnsDiff::identities` carrying them.
* `src/human.rs`: `basis` on every `col_rename()` line, and `changed: values` in place of the bare flag on `col_edit()` and `row_fanout()`.
* `src/lib.rs`: `Hints` destructured once resolution is done, so the map travels on its own and `swap::infer` and `validate_edits` take the edits directly.
* `design.md`, `README.md`, `demo/README.md`, `tests/diff.rs`, and `tests/cli.rs`.

## What stays and why

No demo dataset changes. Every source this step names is already reachable from the existing `demo/` pairs — a declared pair in `key-rename-*`, a hint in `hint-rename-*`, exact inference in `rename-*`, approximate inference in `approx-rename-*`, a swap in `swap-*`, and names everywhere else — so the demos gain new output rather than new files.

No threshold moves, and no agreement statistic is rendered. The thresholds put every accepted rename above nine in ten, so `basis: approximate` already says as much as a number would about how good the match was, and printing `0.94` beside it would dress a judgement up as a measurement. `KeyOverlap` is the precedent for the other reading and does not apply: a guessed key's overlap says how much of the data the key accounts for, which the reader cannot infer from the fact that it was guessed.

`--hint` and `--hints` do not change. The parser learns to skip detail it does not need, which no flag has to know about.

The `IncompatibleColumns` check stays exactly where it is, over every pair the map holds. A hinted pair was type-checked when the hint resolved and a key pair when the key resolved, so the check is redundant for them and total for everything else, which is the cheaper thing to reason about.

## Explicitly deferred

* **Row-number fallback**, and with it the separation of component parsing from key validation. The next queue entry owns both. This step does move key marking off `key.columns` and onto the map, which is a small piece of the same tidying, but it changes nothing about when a key is rejected.
* **The incompatible same-name pair.** Still fatal, still its own entry.
* **Combined list forms.** `col_rename([a, b] -> [b, a])` is declined below, and the same question is open for `col_drop()` and `col_add()`, which the vocabulary also lists with lists and which the renderer also writes one line at a time. Whenever the format decides to group, it should decide for all of them at once.
* **Budgets and sampling.** Unchanged; the queue entry stands.
* **Exposing key-ness through `Diff`.** `ColumnIdentity` carries a coordinate and a basis, not the `is_key` flag the internal pair carries: `KeyDiff::columns` already says which identities are the key, and two accounts of that is the mistake this step is undoing elsewhere.

# Design

## The six sources

| Basis | Word | An identity established by |
|---|---|---|
| `Declared` | `declared` | a component of the declared key, whether it named one column or a pair |
| `Hinted` | `hinted` | an accepted `col_rename` hint |
| `Name` | `name` | both files calling the column the same thing |
| `Exact` | `exact` | inference, from values that agree in every matched row |
| `Approximate` | `approximate` | inference, from values that agree closely and by more than chance |
| `Swapped` | `swapped` | swap inference, exchanging this identity's new end with another's |

Six rather than the four the queue entry names, because a complete account has to cover every pair the map holds, not only the pairs whose two names differ. `Name` is the ordinary case and never reaches a `col_rename()` line — a same-named identity is not a rename — and `Swapped` is the fifth source the entry asks about, decided below. Both are still recorded, because a stage that wants to know whether an identity is provisional should ask rather than re-derive, and because `Diff` should describe every identity the same way.

The field is `basis`, which `col_key()` already carries, rather than a new name. It is the same question — on what basis is this asserted — and `declared` means the same thing in both places. Field names are drawn from a fixed set precisely so that a reader learns them once; spending a second name on the same idea would be the more expensive choice. The values differ by operation, which the grammar has always allowed: `basis: guessed` is a thing a key can be and a rename cannot, and `basis: exact` the reverse.

Precedence falls out of the map rather than being stated anywhere: `claim` inserts only when neither endpoint is spent, so the first claimer's basis is the one that survives, and the claiming order is already the design's order of authority — declared key, then hints, then names, then inference. A hint that agrees with a declared component is redundant rather than contested and leaves the basis `declared`, which is true: the key asserted the pair before the hint was read.

## What the merge removes

Three things stop being maintained by hand:

* **Swap eligibility stops comparing names.** `eligible` currently asks for an identity that is not a key, not hinted, not edit-protected, and whose two schema names agree. The last is a re-derivation of what claiming recorded: a pair with basis `Name` is a same-name pair, and every other basis is excluded on its own account — `Exact` and `Approximate` pairs agree too closely to be swap candidates, `Hinted` is an instruction, `Declared` is the key. So the test becomes basis `Name`, not a key, not edit-protected. The `is_key` clause is still load-bearing, because a guessed key's column is identified by its name like any other and so carries basis `Name`.
* **Rename inference stops taking two maps.** It has been taking a `SchemaMatches` to mutate and a second map to consult for reservations, which the previous step called out as the shape this merge would remove. One map answers both.
* **Nothing recomputes `type_changed`.** Three places compute it today, and `swap::rewire` has to recompute it because exchanging ends invalidates the copy the identity was carrying. `compare_cells` is the only consumer and already has both data types in hand, so deriving it there deletes the flag, the recompute, and the comment explaining the recompute.

`exchange` is the one new map operation. It takes two old positions, swaps their pairs' new ends, and records `Swapped` on both. Neither old position moves, so the pairs stay sorted by old position and `minimal_moves` keeps the precondition it asserts.

## How a swap reads

The queue entry asks whether a swap should render as the vocabulary's `col_rename([a, b], [b, a])` instead of the two lines it writes today. It should not:

```console
$ data-diff demo/swap-old.parquet demo/swap-new.parquet --key id
col_key([id], basis: declared)
col_rename(price -> cost, basis: swapped)
col_rename(cost -> price, basis: swapped)
```

Two arguments carry the decision. The first is that being a cycle is a property of the bijection, not of a source. Two rename hints, `col_rename(a -> b)` and `col_rename(b -> a)`, are accepted together — they contest no endpoint — and they are exactly as much of an exchange as an inferred swap is. Rendering one of them as a group and not the other would make the format's shape depend on which stage happened to produce the permutation, and rendering both as groups means detecting cycles in the renderer, which is a larger question than this step and unrelated to the basis.

The second is that a list form here would be the only one in the output. The vocabulary table lists `col_drop([old1, old2, ...])` and `col_add([new1, new2, ...])` too, and the renderer writes one line per column for both. So the table describes the vocabulary rather than the rendering, and grouping is a format-wide decision the deferred list above records. What the two lines needed was not to be merged but to stop looking like two independent renames, and `basis: swapped` on each is precisely that: a reader who acts on `price -> cost` alone has been told, on the same line, that it is half of an exchange.

`design.md`'s swap section says the combined form today, in a sentence written before the line grammar was settled, and is amended to say this and why.

## Reading a printed line back

Adding a field to `col_rename()` breaks something the last step established: a rendered line, taken exactly as printed, is a hint asserting what it describes. `col_rename(amount -> total, basis: exact)` is not currently parseable, because the parser reads everything between the parentheses as the claim and a name may not contain a comma.

The fix is to say what the parser has always assumed and never enforced: **a hint's claim is its first argument, and anything after it is detail.** Every later argument must be a field, and is ignored. That accepts `basis: exact`, `changed: values`, and `type: Int32 -> Int64` — every suffix the format writes — and refuses `col_rename(a -> b, c -> d)`, whose second argument is shaped like a second claim and is much likelier to be a user meaning something else. Ignoring rather than checking is right because none of that detail is the user's to assert: what a hint contributes is the identity, and supplying `basis: exact` does not make the basis exact — it makes it `hinted`, which is what the hint is.

One rule with no vocabulary in it, because the grammar's colon is what marks detail and every detail the format writes now carries one. That is a change to the format rather than to the parser: `col_edit(price, values)` and `row_fanout(4 -> [4, 5], values)` were the only bare-word arguments written anywhere, and a bare word in that position is indistinguishable from a name — `col_edit(values)` is a column called `values`, and a reader should not need the flag vocabulary to see it. Written `changed: values`, the parser needs no flag list, and admitting bare words in general — the alternative — would have made `col_edit(price, cost)`, a user naming two columns, quietly mean `col_edit(price)`.

The flag itself stays, under its new spelling. It carries no evidence of its own, unlike `type: Int32 -> Int64`, but without it an edit that changed both type and values reads exactly like one that changed only its type.

This completes a claim that was previously only partly true. `col_edit(value, values)` is a line the format writes and the parser has always refused; after this it is an instruction like any other. The round-trip is then a property of every printed line whose operation is one of the four column-hint kinds, rather than of the ones that happen to carry no detail. It is not a property of the whole format and is not meant to become one: `col_key()`, `row_edit()`, and the rest describe what the data is, not an identity a user could assert, and the parser rejects them by kind as it always has.

The round-trip test changes meaning in one respect worth stating, because it is the step working as intended rather than a regression. Feeding back a rendered `col_rename(amount -> total, basis: exact)` produces the same identity and the same diff, but the line comes back as `basis: hinted`: the identity is preserved, the basis is not, and it should not be. The test asserts the identity coordinates agree, that no issue is raised, and that the basis moved from `exact` to `hinted`.

## Where the basis does not go

Only `col_rename()` carries it. `col_edit()`, `col_order()`, and the cell-level facts are about an identity that has already been announced: if `total` was reached by inference, the `col_rename()` line above says so, and repeating it on every later line about the same column would be noise. `col_drop()` and `col_add()` have no basis to carry, being what is left when the bijection is complete — including where a hint reserved the endpoint, which the hint's own presence explains and which the last step deliberately declined to report.

# Verification

* `src/schema.rs` unit tests: `dropped()` and `added()` derived from the widths, including a reserved endpoint appearing in them; a same-name match recording `Name` and a declared component `Declared`; a redundant second claim leaving the first basis in place; `mark_key` marking a pair a later stage will read; and `exchange` swapping two new ends, recording `Swapped` on both, and leaving the pairs sorted by old position.
* `src/rename.rs`: an exactly agreeing pair records `Exact` and a closely agreeing one `Approximate`, so the two stages remain distinguishable in the result rather than only in the code.
* `src/swap.rs`: an accepted exchange records `Swapped` on both pairs, and the existing `a_hinted_identity_is_not_reinterpreted_as_a_swap` and its edit-hint counterpart keep passing unaltered, which is the check that basing eligibility on `Name` did not quietly drop either protection.
* `src/human.rs`: one test over `IdentityBasis::name()` pinning all six words, so a source added later cannot reach the output unnamed; a rename snapshot for each of the five bases a rename can carry; and a snapshot of a swap's two lines. `Name` has no snapshot because it cannot have one — a same-named identity is not a rename — and is covered through the structured identity tests below. The field-name guard keeps its fixed set, `basis` already being in it.
* `src/hint.rs`: each suffix the format writes — `basis: exact`, `changed: values`, `type: Int32 -> Int64` — parsing to the same claim as the bare line; a second pair-shaped argument still malformed, along with a bare `values`, which is a column name wherever it is not a field's value; and a name that legitimately contains a comma still needing its quotes.
* `tests/diff.rs` asserts the basis on the identities of a complete `Diff` for all six sources, and that the identities of two runs that differ only in how the same rename was reached differ only in basis.
* The rename round-trip test carries the printed line back with its field and asserts identity preserved and basis changed, as above; a second round-trip feeds back a printed `col_edit(value, changed: values)`.
* `tests/cli.rs` snapshots every rename line's new field, including the swap pair, and confirms the exit status stays zero.
* Repeated runs of every changed fixture are structurally and byte-identical, and the derived `dropped()` and `added()` are asserted to ascend, since two stages read them positionally.

# Definition of done

This step is complete when:

* every identity in `Diff` carries the basis on which it was established, one of six, and the renderer reads it rather than reconstructing it;
* every `col_rename()` line carries `basis`, and a swap's two lines both say `swapped`, with `design.md` amended to that and to why the combined list form was declined;
* `SchemaMatches` and its `ColumnIdentity` are gone, `ColumnMap` holds the pairs and derives the drops and additions, and reconciliation, rename inference, swap inference, ordering, and cell comparison all work on that one map;
* swap eligibility asks the basis rather than comparing schema names, and no stage recomputes `type_changed`;
* any printed line whose operation is one of the four column-hint kinds is accepted as a hint of that kind, the fields after the first argument being ignored, and the round-trip test says that identity survives and basis does not;
* no line the format writes carries a bare-word argument, `changed: values` having replaced the last flag, so the parser's rule about detail names nothing;
* `README.md` and `demo/README.md` show the field in their outputs and describe what the words mean; and
* the full test suite, strict Clippy, formatting, and diff checks pass across the workspace, and repeated runs still produce byte-identical output.
