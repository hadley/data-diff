---
title: Validated rename hints
---

# Todo

- [x] **Parse the hint vocabulary.** Add `src/hint.rs` with a parser for the subset hints occupy: `kind(name)` and `kind(name -> name)`, where a name is bare and trimmed or JSON-quoted and exact. Every kind that claims an endpoint of the bijection parses — `col_rename`, `col_add`, `col_drop` — so the next step adds what add and drop *do* rather than how they are spelled. Acting on one that is not implemented yet is a fatal `DiffError` distinct from the one an unrecognized kind raises. A malformed line or the wrong shape of argument is likewise fatal.
- [x] **Add the issue channel.** `Diff` gains `issues: Vec<Issue>`, a non-fatal channel whose stable kinds are `hint_missing_target` and `contradictory_hints` for this step. An issue never changes the exit status: it says what reconciliation declined to do, not that it failed.
- [x] **Resolve hint endpoints.** Look each named column up on its own side, reporting `hint_missing_target` for a hint naming a column that is absent, and `hint_incompatible_types` for one whose two columns cannot be compared, rather than failing the comparison in either case. Collapse hints that resolve to the same pair of endpoints.
- [x] **Reject contradictory claims by group.** Build the claim graph over surviving hints, and reject every hint in any connected group that claims an old or new endpoint more than once, reporting `contradictory_hints` once for the group. Input order must not decide a winner.
- [x] **Reserve hint identities first.** Apply accepted identities before key resolution, so a key component can name a renamed key column from either side and key guessing can select one, and pass them into `reconcile_schema` so they reserve their endpoints before names are matched. Validate key uniqueness on resolved coordinates, and keep hinted identities out of swap inference.
- [x] **Accept hints on the command line.** `--hint` takes one hint and repeats; `--hints` takes a file of one hint per line, skipping blank lines and `#` comments. Both feed the same parser, and `DiffOptions` carries the raw spellings so the library owns parsing.
- [x] **Render issues.** Issues follow the `col_key()` line and precede the operations, being context for what the diff does not say. Each renders as `hint_ignored()` applied to what was ignored, with the reason as a field.
- [x] **Cover the machinery.** Unit tests in `src/hint.rs` for the grammar, quoting, trimming, unknown kinds, duplicate collapsing, missing targets, and each conflict shape. Integration coverage in `tests/diff.rs` for a hint that inference could not have found, a hint contradicted by a key component, and a rejected group leaving independent hints standing; CLI snapshots in `tests/cli.rs` for `--hint`, `--hints`, and an ignored hint.
- [x] **Refresh the demo datasets and documentation.** Add a `demo/hint-rename-*.parquet` pair whose renamed column changed too much to be inferred, describe it in `demo/README.md`, and document `--hint` and `--hints` in `README.md`.
- [x] **Complete the acceptance pass.** Run `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`, and confirm repeated runs still produce byte-identical output.

# Goal

Inference now finds a renamed column when its values survived, mostly survived, or were exchanged with another column's. It cannot find one that was renamed and rewritten, and it never will: at that point the values are evidence of nothing, and the only thing that knows the two columns are the same is the person who changed them. This step lets them say so, in the notation the tool already uses to say it back:

```console
$ data-diff demo/hint-rename-old.parquet demo/hint-rename-new.parquet --key id \
    --hint 'col_rename("discount" -> "markdown")'
col_key(["id"], basis: declared)
col_rename("discount" -> "markdown")
col_edit("markdown", values)
```

Without the hint that pair is a `col_drop()` and a `col_add()`, and the values that changed are invisible, unmatched columns having no cells. With it the column keeps its identity, and what happened to its values becomes an edit. A hint asserts identity, not correctness, so the `col_edit()` is not suppressed by the hint that made it visible.

The hint is spelled the way the output spells it, deliberately. A user resolving an ambiguity is looking at the answer they want on their screen, and an LLM generating hints has the same one grammar to target.

Hints are also the first thing a user can get wrong, so the tool has to be honest about ignoring one:

```console
$ data-diff old.parquet new.parquet --key id --hint 'col_rename("discount" -> "mrkdown")'
col_key(["id"], basis: declared)
hint_ignored(col_rename("discount" -> "mrkdown"), missing: "mrkdown")
col_drop("discount")
col_add("markdown")
```

That is the other half of the step, and the larger half in code: parsing, endpoint resolution, deduplication, conflict rejection, and a channel to report all of it. Every later hint kind reuses it, so this step builds it once for a single kind.

# Scope

## What changes

* `src/hint.rs`: new — the grammar, endpoint resolution, deduplication, and conflict grouping, claiming into the shared bijection.
* `src/model.rs`: `Issue` and its kinds, `Diff::issues`, and `DiffOptions::hints`.
* `src/key.rs`: key components resolve through hint identities.
* `src/schema.rs`: `ColumnMap`, the partial bijection every stage claims through, and `reconcile_schema` continuing the one hints began.
* `src/lib.rs`: hints resolve before the key, and their issues reach the `Diff`.
* `src/human.rs`: issues rendered above the operations.
* `src/main.rs`: `--hint` and `--hints`.
* `tests/diff.rs` and `tests/cli.rs`.
* `examples/generate_demo.rs`, `demo/README.md`, and `README.md`.

## What stays and why

`design.md` needs no amendment. Its "Initial hint processing" section is implemented here for one kind, and the ordering constraint in its reconciliation overview — that `col_rename()` is applied before resolving keys, so rows can be matched even when key column names changed — is what this step wires up. The line grammar it now states, and the two lines regularised to obey it, landed first in their own change, on the argument that a format about to become an input language should not have an irregular verb in it.

Nothing about inference changes. A hint identity is an identity like any other, so it is not a rename candidate, and it is not swap-eligible because its ends carry different names. Both fall out of what those stages already ask for, and the tests assert them rather than new code enforcing them.

## Explicitly deferred

* **The other three hint kinds.** `col_add`, `col_drop`, and `col_edit`, their conflict shapes against renames, and the `hint_no_change` and `hint_unresolved_identity` issues are the next queue entry. The grammar and the issue channel are built here to be extended, not rebuilt.
* **`invalid_key` as an issue.** The design describes a rejected declared key as an issue that falls back to guessing; it is a fatal error today. Moving it into this channel belongs with the row-number fallback entry, which owns the surrounding rework.
* **Hints from the UI.** The design expects hints from an interactive session as well as the command line. `DiffOptions` carrying raw spellings is what makes that possible later; nothing interactive is built here.
* **A parser for the whole grammar.** Only the subset hints occupy is parsed. `col_key()`, `col_order()`, and the row operations are output-only, and writing a reader for lines nothing can supply would be inventing a requirement.

# Design

## What hints need from the grammar

`design.md` now states the grammar the human format obeys, and the two lines that broke it were regularised before this step began, so what follows can assume a format with no exceptions in it:

```
line      := kind "(" [ argument { ", " argument } ] ")"
argument  := value | field
field     := name ": " value
value     := quoted | number | word | pair | list | line
pair      := value " -> " value
list      := "[" [ value { ", " value } ] "]"
```

Hints occupy a small subset: `kind(name)` and `kind(name -> name)`. That is the whole of it, because a hint asserts identity and nothing else — the details a `col_edit()` prints are conclusions, not instructions. So the parser is small, and it is written against the grammar rather than against `col_rename` so the next step adds kinds without touching syntax.

An argument is bare or quoted. Bare is trimmed, so `col_rename(discount -> markdown)` means what it looks like rather than naming a column ` markdown`. Quoted is exact, decoded by the same JSON rules the output encodes with, so a name containing a comma, a bracket, an arrow, a newline, or its own leading space can be named. Column names are exact and case-sensitive everywhere else in this tool; quoting is what keeps that true here without making the common case unreadable.

This is deliberately better than `--key`, whose `/` and `,` separators leave a column named `a/b` unreachable. That wart is not worth propagating, and fixing it is not this step's business either.

## Fatal, or reported

Two failure modes look alike and are not:

* The hint is not a hint. An unknown kind, an unclosed bracket, an argument that is not a name or a pair: nothing can be done with the string, and the user has mistyped a command. That is a `DiffError`, like `MalformedKeyComponent`, and the comparison does not run.
* The hint is well formed and wrong about the data. It names a column that is not there, it names two columns whose values cannot be compared, or it contradicts another hint. Reconciliation proceeds perfectly well without it, so it is dropped, reported, and the exit status stays zero.

The line between them is whether the tool can tell what was meant.

The incompatible-types case is the one worth spelling out, because it is the only reason a hint has to be checked against types at all. An accepted identity goes on to be compared cell by cell, and a boolean against an integer has no comparison; left unchecked, the hint would sail through resolution and conflict detection and then abort the entire diff from inside cell comparison. Declining it there instead costs one instruction and keeps everything else.

## What a hinted identity is safe from

Two invariants follow from a hint being an instruction rather than a default.

A key's components must resolve to distinct columns, and that has to be checked on the resolved coordinates rather than the names. `--key id,customer_id` beside a `customer_id -> id` hint resolves both components through that one identity, so two differently spelled components land on the same column while the existing duplicate-name check sees nothing wrong.

Swap inference does not reconsider a hinted identity. Every other exclusion from swap candidacy — a key component, an inferred rename — is about a default that better evidence may override. A hint is not a default. This only bites for a hint whose two ends carry the same name, since every other hinted identity is already ineligible for having different ones, and it is what makes `col_rename(a -> a)` mean something: it pins `a` against being read as half of an exchange.

## Conflicts, and why the group loses

Rename claims form a bipartite graph, each hint an edge from an old endpoint to a new one, and a valid set is a matching. Any endpoint with two edges is a contradiction: one old column renamed to two new ones, or two old columns to one new one.

The design rejects the whole connected group rather than picking a winner, and the reason bears restating because a smaller rule looks tempting. Given `col_rename(a -> b)` and `col_rename(a -> c)`, keeping the first decides by input order, making the result depend on which flag came first or how a file was written. Keeping neither is the only answer that treats two equally supported, mutually exclusive claims as what they are. Which claims go is settled endpoint by endpoint: one is rejected exactly when a rival wants an endpoint of it. Grouping them into connected components changes only the reporting — it cannot reach a claim that was not already contested, since reaching one means sharing an endpoint — and what it buys is one issue naming a whole set of rivals rather than one per claim, so a reader can see that they conflict with each other.

Deduplication runs first, so two identical hints are one claim rather than a self-conflict, and identity is judged after endpoint resolution, so a quoted and a bare spelling of the same pair collapse.

## One bijection, claimed in stages

`design.md` describes column identity as a partial bijection with a settled order of precedence: paired key components and rename hints claim first, remaining same-named columns take what is left, and inference fills in from there. That is one object, and it is now one object in the code — `ColumnMap`, holding the pairs and refusing any claim on an endpoint already spent.

The alternative, which this step started out with, was for hints to keep their own list of identities that `reconcile_schema` merged with the key's before matching names. That works, but it means the bijection exists in two places at once, with the invariant that no endpoint is used twice enforced separately in each. Claiming through one map instead makes the precedence order literally the order of the calls, and lets the drops and additions fall out of what the map has no pair for, which is exactly the definition.

Hints run before the key, so they cannot be handed a `SchemaMatches` to add to — the schema is reconciled after row matching. What they are handed instead is nothing, and what they return is the seed. `ColumnMap` carries one bit of provenance beside each pair, whether a hint asserted it, because swap inference must not reconsider an instruction. Recording where an identity really came from, and rendering it, is its own queued step.

## Key components claim first

Rename identities apply before key resolution, which is what lets `--key id` work when the old file calls that column `customer_id`. A key component naming a single column therefore resolves through the hint identities: it looks for that name on both sides, and where one side lacks it, an identity whose other end carries it supplies the missing endpoint.

That leaves a key component and a hint claiming the same endpoint differently — `--key a/b` beside `col_rename(a -> c)`. The key wins and the hint is reported. Key components are not hints: they are load-bearing for row matching, and the design already has them establishing identity before validation. Making them peers in the conflict graph would let a mistyped hint invalidate the key and change every row event, a far worse failure than an ignored hint. So key claims are reserved before the graph is built, and hints conflict against them rather than with them.

## How an issue renders

An issue is not an operation. It says what reconciliation declined to do, which makes it the same sort of thing as the `col_key()` line: context for reading what follows. So issues render immediately after the key line and before the first operation.

The head is `hint_ignored()` rather than the issue kind, with the kind carried as the reason field — `missing:` for an absent target, `contradictory` for a rejected group. One head keeps the line grammar's field names fixed and makes the important thing, that an instruction was dropped, the first thing read. The stable kinds stay in the model, where `Diff::issues` is what a consumer matches on.

The subject is whatever the reason applies to: a single hint for a missing target, and the whole group for a contradiction, which is one line per group rather than one per hint repeating its rivals.

```
hint_ignored(col_rename("discount" -> "mrkdown"), missing: "mrkdown")
hint_ignored([col_rename("a" -> "b"), col_rename("a" -> "c")], contradictory)
```

Issues go to stdout and leave the exit status alone. Stderr and a non-zero status mean the comparison did not happen; an ignored hint means it happened without one instruction, which a user piping the output needs to see because it changes what the operations mean.

# Verification

* `src/hint.rs` unit tests cover: a bare and a quoted spelling resolving to one identity and collapsing; names needing quotes, including a comma, a bracket, an arrow, and a newline; whitespace trimmed around bare arguments and kept inside quoted ones; an unknown kind, an unclosed bracket, and a malformed argument each rejected as errors; a missing old target and a missing new target each reported; and the three conflict shapes — one old to two new, two old to one new, and a connected chain — each rejecting their whole group.
* One test asserts an independent valid hint survives beside a rejected group, group rejection being meant as local rather than a reason to drop everything.
* `tests/diff.rs` asserts a complete `Diff` for a hint inference could not have found, with the identity established, `added` and `dropped` empty, and the changed values reported as an edit — which is also the check that a hint asserts identity without asserting equality. A second test pins a hint contradicted by a key component, showing the key intact, the hint reported, and the columns left unmatched.
* `tests/cli.rs` snapshots `--hint`, `--hints` reading a file with a comment and a blank line, and an ignored hint beside a successful one, confirming the exit status stays zero.
* One test feeds a rendered `col_rename()` line straight back in as a hint and asserts the diff is unchanged, which is the claim that the format is an input language rather than merely resembling one.
* Separate tests pin what the fixes above are for: an incompatible claim declined rather than aborting, two key components resolving through one hint rejected, a hinted identity surviving swap inference, a hint identity selected by key guessing, and a diff of two identical files still reporting `no_changes()` beneath a declined hint.
* Two tests defend the ordering. One renames a key column and supplies only `--key id`, which can resolve only through the hint. One supplies a hint whose endpoints inference would otherwise have paired differently, showing the hint reserved them first.
* The `hint_ignored()` line joins the field-name guard's fixtures, so `missing:` is checked against the fixed set like every other field, and the guard keeps covering every line kind the format can write.
* Repeated runs of every new fixture are structurally and byte-identical.

# Definition of done

This step is complete when:

* a `col_rename` hint establishes column identity, spelled as the output spells it, supplied inline with `--hint` or from a file with `--hints`;
* a quoted argument can name any legal column name and a bare one is trimmed;
* a malformed hint is a fatal error while a well-formed hint the data contradicts is reported and ignored, with the exit status unchanged;
* identical hints collapse, and contradictory claims reject their whole connected group without input order deciding anything;
* hint identities are reserved before key resolution and before name matching, so a key component can name a renamed key column;
* `hint_missing_target` and `contradictory_hints` reach the `Diff` through an issue channel built for the kinds that follow, and render as `hint_ignored()` above the operations;
* the demo datasets and both READMEs describe hinting a rename; and
* the full test suite, strict Clippy, formatting, and diff checks pass across the workspace, and repeated runs still produce byte-identical output.
