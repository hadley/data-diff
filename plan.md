---
title: Row-number fallback
---

# Todo

- [x] **Separate parsing from resolution and validation.** `declared_components` keeps returning `DiffError` for a `--key` string that cannot be read. Everything after it — endpoint resolution, type compatibility, and validation — returns a typed `KeyRejection` instead, so a well-formed key this data cannot support becomes a value the chain acts on rather than an error that unwinds it.
- [x] **Give the chain a shape.** `resolve_key` becomes declared, then guessed, then row number, returning the first that holds along with any rejection the declared attempt produced. Nothing below it learns which attempt won except through `ResolvedKey`.
- [x] **Synthesize the fallback key.** The fallback builds `old` and `new` as one `CanonicalValue::Int` per row position, so `match_rows` pairs equal positions with no change to it at all, and the key can never fan out.
- [x] **Add `KeyBasis::Fallback` and `KeyDiff::rejection`.** Retire `DiffError::MissingKey` and the five variants that become rejections. `Issue` stays hint-only, which is what its `hints` member has always assumed.
- [x] **Keep every stage running.** No stage takes a branch on the basis, so nothing below key resolution changes at all. Pin that with tests rather than leaving it to inspection: rename inference, swap inference, and cell comparison all work under a fallback basis, and row order is always empty under it.
- [x] **Render the new lines.** `col_key([#row], basis: fallback)`, and a `key_invalid()` line naming the subject and `reason:`.
- [x] **Accept `--key '#row'`.** A reserved component naming the positional key, which yields the same synthesized key the fallback does under `basis: declared`. It is a whole key or none of it, and `--key id,#row` is malformed.
- [x] **Give every declined instruction a `reason:` field.** `hint_ignored()`'s bare trailing words become `reason: contradictory`, `reason: unresolved`, and `reason: unchanged`, so no line in the format ends in an unnamed value.
- [x] **Put problems first, above a `----` separator.** Every problem — an invalid key, a declined hint — moves to the head of the output, then a `----` line, then what the comparison learned. With no problems there is no separator and nothing changes. This touches every snapshot that has a `hint_ignored()` line in it.
- [x] **Keep the identities a rejected key asserted.** A paired component claims a column identity before validation runs, so `col_rename(customer_id -> id, basis: declared)` survives a key that turns out to be unusable.
- [x] **Cover it.** Unit tests per module, integration coverage in `tests/diff.rs` for a complete `Diff` on a fallback basis, CLI snapshots in `tests/cli.rs`, and a demo fixture with its own section in `demo/README.md`.
- [x] **Update `design.md` and both READMEs.** The key-resolution section names the basis `fallback` rather than `row_number`, the operation `key_invalid` rather than `invalid_key`, documents `#row`, says that the fallback disables no stage and what that costs, and corrects its claim that the rejection carries *all* reasons to the one that stopped validation. `README.md`'s output section gains the problems block, the `reason:` field, and `--key '#row'`, and loses "the first line always says which key was used".
- [x] **Complete the acceptance pass.** Run `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`, and confirm repeated runs still produce byte-identical output.

# Goal

Every way of failing to find a key is currently fatal. A declared key that is not unique, a `--key` naming a column one file does not have, a pair of files whose columns are all repeating categories — each aborts the comparison and prints one line to stderr, and the user learns nothing about two files that may differ in a single cell:

```console
$ data-diff old.parquet new.parquet --key id
old key is non-unique at rows 2 and 7 (non_unique_old)
```

`design.md` has always said otherwise. Rows resolve "from a declared key, a guessed key, or finally row number", and a declared key that fails validation "records an `invalid_key` issue with all reasons, then continues to key guessing and, if necessary, row-number matching". None of that fallback exists. This step builds it, so a readable pair of files always produces a diff and the first line says what identifying rows had to fall back to:

```console
$ data-diff old.parquet new.parquet --key id
key_invalid([id], reason: non_unique_old)
----
col_key([#row], basis: fallback)
col_edit(price, changes: 2)
```

The second half of the step is what a rejected key leaves behind. `--key customer_id/id` asserts two things at once: that those two columns are one column, and that the column identifies rows. The second can fail while the first still holds, and today the failure discards both. Separating resolution from validation is what lets the assertion outlive the rejection.

# Scope

## What changes

* `src/key.rs` splits into resolution and validation, `resolve_key` becomes a fallback chain rather than a single attempt, and `declared_components` learns the reserved `#row` component.
* `src/main.rs` says what `#row` means in `--key`'s help text, which `tests/cli.rs` snapshots.
* `src/model.rs` gains `KeyBasis::Fallback` and `KeyDiff::rejection`, and loses `DiffError::MissingKey` along with the five variants that become rejection reasons.
* `src/lib.rs` carries any rejection into the diff. It is otherwise unchanged: no stage branches on the basis.
* `src/human.rs` renders the new key line and the new problem, and gains the problems block and its separator. Every existing snapshot with a `hint_ignored()` line moves with it, in `src/human.rs`, `tests/cli.rs`, and `demo/README.md`.
* `src/schema.rs` handles a key with no columns, which its `for column in &key.columns` already does correctly and which a test should pin rather than leave to luck.

## What stays and why

* `match_rows` is untouched. A synthesized fallback key satisfies everything it already assumes, so positional matching is the existing algorithm over different values rather than a second path beside it.
* `cells`, `summary`, `order`, and `hint::validate_edits` are untouched. They read the row match and the column map, neither of which changes shape.
* Fanout stays one-directional and keeps its 10% limit. Nothing here revisits what makes a key usable; it changes only what happens when there is not one.

## Explicitly deferred

* Reconsidering the key when rename inference identifies a column that was a candidate for it. That is the next item in `plan-next.md`, and it depends on the machinery this step builds.
* Any budget, sample, or partial-result reporting. That is a later item, and this step must not pre-empt its vocabulary.
* Incompatible same-name column pairs, which stay fatal in `reconcile_schema`.
* A flag to refuse the fallback and fail instead. Worth having, but it is a question about the CLI and this step is about reconciliation.
* Aggregating several rejection reasons into one problem, for the reason given below.

The problems block is scope this step grew rather than scope it began with, and it is worth saying why it belongs here rather than in a step of its own. This step introduces the first problem in the format that is not a declined hint, which is exactly what makes "what went wrong, then what we found" the right shape and what makes the old arrangement — one problem beside the key line, the rest after it — untenable. Splitting it out would also churn the same snapshots twice. If the owner would rather have it separately, it lifts out cleanly: the rest of the step works with the rejection rendered as one more line after the key.

# Design

## Which failures fall back and which stay fatal

Key failures divide by whether the instruction could be read at all.

**Fatal, unchanged.** `EmptyKeyComponent`, `MalformedKeyComponent`, and the `DuplicateKeyColumn` that `declared_components` raises on names. These are faults in the `--key` string itself: there is no key to reject and nothing about the data is in question, so falling back would answer a question the user has not managed to ask. These are the only three `declared_components` can raise, which is what makes parsing the clean seam.

**Rejected, then fall back.** `MissingKeyColumn`, `IncompatibleKeyTypes`, the `DuplicateKeyColumn` that `validate_distinct` raises on resolved indices, `InvalidKeyValue`, `NonUniqueOldKey`, and `ExcessiveFanout`. Each is a well-formed key that this pair of files cannot support, which is the case `design.md` says should continue.

`MissingKeyColumn` is the arguable one, since `--key nosuchcolum` is usually a typo rather than a claim about the data. It falls back anyway, because reconciliation cannot tell a typo from a column that was dropped between the two files, and the `key_invalid()` line names the component either way.

## How a rejection is represented

The seam is parsing, not resolution. Two of the six rejectable failures — a missing column and an incompatible type pair — arise while resolving a component to its endpoints, so a split that kept resolution fatal and made only validation recoverable would contradict the policy above. Resolution and validation therefore share one result type, and only `declared_components` above them still returns `DiffError`:

```rust
/// A declared key that could not be used, and what about it failed.
pub(crate) struct KeyRejection {
    pub subject: KeySubject,
    pub reason: RejectionReason,
}

/// What the rejection is about.
pub(crate) enum KeySubject {
    /// One component, as the user spelled it.
    Component(String),
    /// The declared key entire, each component as spelled.
    Key(Vec<String>),
}

pub(crate) enum RejectionReason {
    MissingColumn { side: Side },
    IncompatibleTypes { old_type: String, new_type: String },
    DuplicateColumn { side: Side },
    InvalidValue { side: Side, row: usize },
    NonUniqueOld { first_row: usize, row: usize },
    ExcessiveFanout { affected: usize, shared: usize },
}
```

`RejectionReason` is deliberately the payload of the `DiffError` variants it replaces, so nothing today's error messages can say is lost by making them recoverable — the detail moves into the diff rather than out of the program. `DiffError::DuplicateKeyColumn` survives for the parsing case while `RejectionReason::DuplicateColumn` covers the resolved-index case, because those are two different discoveries that happen to share a name.

## Which subject a reason takes

A rejection is not always about a component. Uniqueness and fanout are properties of the whole compound key: `--key account,date` can be non-unique while both `account` and `date` resolve perfectly, and there is no one component to blame. The subject follows the reason:

| Reason | Subject | Because |
|---|---|---|
| `missing_column` | component | One endpoint of one component is absent. Naming it still works, the two names coming from parsing rather than resolution. |
| `incompatible_types` | component | One old/new pair cannot be compared. |
| `duplicate_column` | component | This component resolved onto a column an earlier one already claimed. |
| `invalid_value` | component | One component holds the null or `NaN`. |
| `non_unique_old` | key | Uniqueness is a property of the tuple, not of any column in it. |
| `excessive_fanout` | key | The rate is computed over the tuple. |

The reason is a named field, `reason:`, like every other field the format writes. The two subject forms are distinguished by brackets, matching the key line's own shape, so a reader can tell at sight which is meant:

```
key_invalid(amount, reason: missing_column)
key_invalid([account, date], reason: non_unique_old)
```

A single-component key that fails uniqueness therefore prints `key_invalid([id], reason: non_unique_old)` — bracketed, because uniqueness is still a fact about the key rather than about the column that happens to be all of it.

Both forms name a component the way the key line names it: `--key customer_id/id` produces `key_invalid([customer_id -> id], reason: non_unique_old)`, matching the `col_key([customer_id -> id], ...)` it sits directly above. The `/` is `--key` input syntax and appears nowhere in the output, so echoing it would introduce a second spelling for one component. The arrow form needs only the two names, which parsing yields whether or not either resolves to a column, so it is available for every reason including a missing one.

## One reason, not all

`design.md` says the issue records "all reasons". It should not, because the reasons are not independent: the fanout rate is only defined once the old side is known unique, which is why `validate_unique_old` runs before `validate_fanout` today. A fanout rate computed over a non-unique old side would be a number that means nothing. The issue therefore carries the one reason that stopped validation, and `design.md` is corrected to say so.

## Declaring the positional key

The fallback is otherwise the one basis a user cannot ask for. Guessing is the default and `--key` declares a real key, but matching by position is reachable only by failing to find anything else, so a user who knows two exports hold the same rows in the same order has to contrive a broken key to get it. `--key '#row'` says it directly.

The name is reserved rather than looked up. A bare name in this format is letters, digits and underscores and never starts with `#`, so `#row` cannot collide with any column the output would write bare, and a column genuinely called `#row` renders quoted as `"#row"` and stays distinguishable. The cost of the reservation is that such a column can never itself be declared as a key, which is worth one sentence in `README.md` and nothing more.

It is a whole key or none of it. `--key id,#row` is refused by `declared_components` as a malformed key, beside the other faults in the `--key` string that stay fatal: a positional key has no components to compound with, and the mixture has no reading. `#row` also has no endpoints to resolve and nothing to validate — positions are distinct, present, and comparable by construction — so it can never produce a rejection.

What it changes downstream is only the basis. `--key '#row'` yields the same synthesized key the fallback yields, with `basis: Declared` rather than `basis: Fallback`:

```
col_key([#row], basis: declared)     you asked for it
col_key([#row], basis: fallback)     nothing else worked
```

The key line names `#row` in the list rather than showing an empty one, so `basis` keeps meaning throughout the format exactly what it means for every other key: how this one was arrived at. An empty component list is therefore never rendered and never occurs — `declared_components` rejects an empty `--key`, so `columns.is_empty()` holds for positional keys and for nothing else, which is the invariant the renderer reads.

## What the fallback key is

`ResolvedKey` gains no new shape. The fallback fills it in as `basis: Fallback`, `columns: vec![]`, `overlap: None`, and `old`/`new` holding `vec![CanonicalValue::Int(position)]` for each row.

This is worth doing rather than special-casing `match_rows`, because every invariant that function relies on holds here by construction. Row positions are distinct, so the key is unique in `old` and can never fan out; they are never null or `NaN`, so no value is an invalid key; and equal positions compare equal across sides. Positional matching then falls out as the ordinary algorithm — rows `0..min(n_old, n_new)` match and the tail of the longer side becomes additions or drops — with no branch anywhere below key resolution.

## Which stages remain valid

Every one of them. The item this step comes from asks which stages must be skipped without a semantic row key, and the answer is none: two files compared without a key are usually two versions of the same file, whose rows are in the same order, and on that reading positional matching is not a degraded key but the right one. A stage disabled here would cost every such comparison the thing it was most likely to find.

| Stage | Under `fallback` | Note |
|---|---|---|
| `schema::reconcile_schema` | runs | Reads `key.columns` only to mark key columns, and an empty list marks none. |
| `rename::infer` | runs | Agreement across positionally matched rows. |
| `swap::infer` | runs | The same. |
| `order::detect_order` | runs | Column order reads identities, not rows. Row order is computed over positional matches, which ascend on both sides, so it is always empty — a fact worth a test rather than a special case. |
| `cells::compare_cells` | runs | Reports what the positional comparison found. |
| `hint::validate_edits` | runs | Needs an identity and the cells, both of which exist. |
| `summary::summarize` | runs | Reads the cells alone. |

So no stage takes a branch on `KeyBasis::Fallback`, and `src/lib.rs` is unchanged below key resolution. That is the strongest form of the answer: the fallback produces a key like any other, and the pipeline below it never learns which kind it got.

What the reading costs is worth recording, because it is the case a reader will eventually hit. Where the rows are *not* in the same order, positional matching is wrong about which row is which, and every stage that reads a matched row inherits that: cells report widespread edits, and rename or swap inference can identify two columns on agreement that is coincidental. The first is loud, the second quiet — a wrong identity changes what every stage below it compares and prints a confident `basis: exact` with no trace of the assumption underneath. The safeguard is the first line saying `basis: fallback`, and a user who sees widespread change under it should read the whole diff as conditional on the row order. Should that prove too weak in practice, disabling inference under this basis is a one-line change and a later step.

Identities that were asserted rather than inferred do not depend on this at all. A `col_rename()` hint and a paired `--key` component both survive into a fallback diff whatever the row order.

## What a rejected key leaves behind

`claimed_identities` already claims a paired component's identity into the map before hints resolve, and `diff_tables` already threads that map through every stage below. The only thing discarding it is the `?` on `resolve_key` unwinding the whole comparison. Once rejection is a value rather than an error, the claim survives with no further plumbing, and this becomes true:

```console
$ data-diff old.parquet new.parquet --key customer_id/id
key_invalid([customer_id -> id], reason: non_unique_old)
----
col_key([#row], basis: fallback)
col_rename(customer_id -> id, basis: declared)
row_edit(2, changes: 1)
```

The pair asserted two things and one of them still holds.

## Problems first, then what we learned

The output gains one division. Everything that went wrong comes first — a rejected key, a declined hint — then a `----` line, then everything the comparison found:

```
key_invalid([id], reason: non_unique_old)
hint_ignored(col_edit(id), reason: unchanged)
----
col_key([#row], basis: fallback)
col_edit(price, changes: 2)
```

With nothing to report the separator does not appear and the output is exactly what it is today, beginning with the key line. The separator marks a boundary rather than opening a section, so there is nothing to close and no trailing rule.

This settles where a key rejection sits, which was open while problems were split either side of the key line. It also settles the ordering *within* the block, and does so without inventing a position for something that has none. Hint issues are ordered by the input position of the hint each concerns, which is what `Pending::at` is; a key rejection has no such position and is not given a fake one. Instead it never enters that list: it is carried on `KeyDiff` as `rejection: Option<KeyRejection>` and written first, and the renderer composes the block from two typed sources rather than from one list it must sort. `Issue` and `IssueKind` stay hint-only, which their `hints: Vec<HintClaim>` member has always assumed.

That is a departure from `design.md`, which calls the rejection an `invalid_key` *issue*. Both halves of that need amending. The container is wrong: an `Issue` in this codebase is an instruction declined, and every one of them names the hints it concerns, so the rejection is recorded on the key instead. And the name is written subject-last, where every operation in the format is written subject-first — `col_add`, `row_edit`, `hint_ignored` — so it becomes `key_invalid`.

Every declined instruction gains the same field. `hint_ignored()` today ends three of its five forms in a bare word — `contradictory`, `unresolved`, `unchanged` — which become `reason: contradictory` and so on, so that nothing in the format ends in an unnamed value. Its other two forms, `missing:` and `incompatible:`, already name a field and carry a detail in it; they are left as they are, and whether they should become `reason:` with the detail in a field of its own is a question worth settling separately rather than inside this step.

The reason identifiers reuse what the design and the existing errors already name where they can — `non_unique_old` is already the identifier in `DiffError::NonUniqueOldKey`'s message — giving the set `missing_column`, `incompatible_types`, `duplicate_column`, `invalid_value`, `non_unique_old`, and `excessive_fanout`. Detail beyond the identifier stays in the model: `invalid_value` knows its row and side, and the line does not say them, because the line's job is to name what went wrong and the model's is to hold it.

Two consequences to carry through. `README.md` says "The first line of output always says which key was used", which stops being true whenever there is a problem, and `write_human`'s own doc comment says the same. And the "nothing changed" test becomes a question about the operations below the separator, not about the length of the whole output.

# Verification

* Unit tests in `src/key.rs` for each rejection reason reaching the fallback, for a guessed key being reached after a declared one is rejected, and for the synthesized key being unique and fanout-free.
* Unit tests in `src/human.rs` for the key line and the `key_invalid()` line, including an issue that concerns no hints.
* Integration coverage in `tests/diff.rs` for a complete `Diff` on a fallback basis: identities claimed by a paired component present, an inferred rename found across positionally matched rows, row order empty, cells and summary populated.
* A test that two files whose columns all repeat produce a fallback diff rather than an error, replacing `tests/smoke.rs`'s `library_boundary_requires_a_key`, which asserts the behaviour this step removes.
* CLI snapshots in `tests/cli.rs` for the fallback reached with and without a rejected declaration, for `--key '#row'` reaching the same key under `basis: declared`, and for `--key id,#row` being refused.
* A test that `--key '#row'` and an unusable declared key produce identical diffs but for the basis, which is the sharpest statement that the two routes reach one key.
* A demo fixture pair no column can identify rows in, and a `demo/README.md` section for it. `tests/readme.rs` then holds that section to the real output like every other.
* Determinism: repeated runs byte-identical, including the order of issues where a rejection and a hint issue coexist.

# Definition of done

This step is complete when:

* no readable pair of files fails for want of a key, and `DiffError::MissingKey` is gone;
* a declared key this data cannot support is reported as a `key_invalid()` issue naming the component and the one reason that stopped it, and the comparison continues to a guessed key and then to row number;
* the identities a paired `--key` component asserted survive its rejection, appearing as `col_rename(..., basis: declared)`;
* a fallback basis prints `col_key([#row], basis: fallback)` and disables no stage, nothing below key resolution branching on the basis at all;
* `--key '#row'` reaches that same key deliberately and prints `col_key([#row], basis: declared)`, and cannot be compounded with a real component;
* no line in the format ends in an unnamed value, `hint_ignored()`'s bare words having become `reason:`;
* `match_rows` is unchanged, the fallback key being synthesized to satisfy what it already assumes;
* problems precede a `----` separator and findings follow it, with no separator when there are no problems;
* `design.md` records the basis as `fallback`, says that it disables no stage and what that costs, and no longer claims the rejection carries every reason;
* `README.md` and `demo/README.md` show the fallback, and `tests/readme.rs` verifies the demo section; and
* the full test suite, strict Clippy, formatting, and diff checks pass across the workspace, and repeated runs still produce byte-identical output.
