---
title: Paired key components
---

# Todo

- [x] **Parse paired components.** In `src/key.rs`, split each `--key` component on `/` into an old and a new name, resolve the two endpoints independently, and validate that no old or new column is claimed by more than one component. Replace `DiffError::PairedKeyUnsupported` with `MalformedKeyComponent`, and replace `DuplicateKeyComponent` with an endpoint-based `DuplicateKeyColumn { side, column }`.
- [x] **Carry both names on a key column.** Change `KeyColumn::name` to `component`, holding the user's spelling of the component, so error messages name what was declared rather than one half of it.
- [x] **Reserve key identities before same-name matching.** In `src/schema.rs`, claim each key column pair as an identity at its old-column position, mark its new endpoint as taken before the same-name pass, and let the remaining columns match by name as they do today. Reserved endpoints are unavailable to name matching, so the old column whose name a key pair consumed becomes a drop and the new one an addition.
- [x] **Derive and render renames.** In `src/human.rs`, emit `col_rename(old, new)` for every identity whose two schema names differ, ahead of the other column operations, and render a paired key component as `"old" -> "new"` so it cannot be confused with two components of a compound key.
- [x] **Name identities by their new name.** Change `col_edit()` and `col_order()` to resolve their column name against `schemas.new` rather than `schemas.old`, so no operation refers to a column by a name the new file does not have. Add the convention to `design.md`, whose vocabulary table writes `col_edit([old1, ...])` and would otherwise contradict it.
- [x] **Describe the flag accurately.** Update the `--key` help text in `src/main.rs` and the `DiffOptions::key` doc comment in `src/model.rs`, which both say the columns must share a name, and accept the resulting change to the `--help` snapshot in `tests/cli.rs`. That snapshot is the only one this step may change, and it must change: leaving it green would mean the documented interface still contradicts the implemented one.
- [x] **Cover the new form.** Unit tests in `src/key.rs` for parsing, endpoint validation, and each error; in `src/schema.rs` for reservation beating name matching; and in `src/human.rs` for both rendered forms. Integration coverage in `tests/diff.rs` for a renamed key with reordered rows and changed cells, and a CLI snapshot.
- [x] **Refresh the demo datasets and documentation.** Add a `demo/key-rename-*.parquet` pair whose key column is named differently in each file, and document the paired syntax in `demo/README.md` and `README.md`.
- [x] **Complete the acceptance pass.** Run `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`, and confirm repeated runs still produce byte-identical output.

# Goal

Renaming the key column is one of the most ordinary things a script does, and it is the change `data-diff` handles worst. Column identity is name equality, so a renamed key is a drop plus an addition, no column pairs up, no key can be resolved, and the two files look entirely unrelated. `--key` cannot rescue it either: a component is one name used on both sides, and the paired form the design defines is rejected outright by `PairedKeyUnsupported`.

This step implements that paired form:

```console
$ data-diff old.parquet new.parquet --key customer_id/id
col_key(declared: ["customer_id" -> "id"])
col_rename("customer_id", "id")
row_edit(2)
```

It is also where column identity stops being a synonym for name equality. The design defines identity as a partial bijection in which paired key components and rename hints reserve identities first and same-named columns fill in the rest; this step builds the first half of that and leaves the shape ready for hints and rename inference. `col_rename()` enters the output vocabulary for the first time, derived from an identity whose ends carry different names rather than stored as its own event.

# Scope

## Splitting the queued item

The queue entry read "add paired key components and validated rename/add/drop/edit hints". That is two changes, and this plan is the first; the hints return to `plan-next.md` as two items, sequenced after exact rename inference and after swap detection rather than before them.

They are worth separating. Paired key components need no new machinery: `reconcile_schema` already receives the resolved key and already collects its column pairs to mark `is_key`, so reserving those pairs as identities is a small change to an existing pass. Hints need hint parsing, normalization, deduplication, conflict detection over connected groups of contradictory claims, and an issue channel in `Diff` that does not exist yet, with four issue kinds and a rendering for them.

They are also worth postponing. Three of the four kinds exist only to constrain inference — `col_add` and `col_drop` reserve endpoints to keep them out of rename candidates, and `col_edit` protects a column from being read as half of a swap — so landing them before inference exists would mean writing reservations that nothing reads and no test can exercise. `col_rename` is the exception, and the paired form in this step covers its most pressing use, the renamed key column. This changes only the order in which the work is built: `design.md` continues to process hints before key resolution at run time.

What the two halves genuinely share is the identity bijection, and this step is where it starts. Doing it first gives the hints step something to attach to instead of introducing both at once.

## What changes

* `src/key.rs`: component parsing, endpoint resolution, and endpoint uniqueness.
* `src/model.rs`: the two replaced error variants and their messages.
* `src/schema.rs`: reserved identities ahead of the same-name pass.
* `src/human.rs`: `col_rename()`, the paired rendering of a key component, and the switch to naming identities by their new name.
* `design.md`: one sentence recording that display convention.
* `src/main.rs` and `src/model.rs`: the `--key` help text and the `DiffOptions::key` doc comment, both of which currently promise same-name columns.
* `tests/diff.rs` and `tests/cli.rs`, including the `--help` snapshot that carries the flag's description.
* `examples/generate_demo.rs`, `demo/README.md`, and `README.md`.

## What stays and why

`design.md` needs only the display convention added. It already defines the paired form, the reservation order, and the rule that a rename is derived wherever an identity's two names differ, so the reconciliation behavior here is what is already written — with one timing difference recorded under deferrals below. What it gains is the sentence saying an identity is displayed under its new name, which its `col_edit([old1, ...])` notation would otherwise appear to deny.

Guessing is untouched and stays same-name. A guess has no user assertion behind it, and inferring identity between differently-named columns is rename inference, which is a later step with its own evidence rules.

The model gains no rename field. A rename is derived from `columns.identities`, whose ends already carry names through `schemas.old` and `schemas.new`, so storing it again would be a second copy of a fact the bijection already holds — and the design is explicit that identity is what the diff stores.

Duplicate column names remain a fatal input error, which is what lets a component name resolve to exactly one column per side without further checks.

## Explicitly deferred

* **All four hint kinds and the issue channel**, now two queue items placed after inference.
* **Renames from anything but a declared pair.** Inference over matched rows is the next step, and swaps the one after.
* **The list form of `col_rename()`.** The vocabulary's `col_rename([a, b], [b, a])` exists for swaps; a declared pair is always one-to-one, so this step emits the two-argument form and the list form arrives with swap detection.
* **Guessing a compound key**, unchanged.
* **Surviving key validation failure.** The design says a paired component establishes identity *before* key validation, like a rename hint, so that a key rejected as invalid still leaves the user's asserted identity in place. This step reserves identity inside `reconcile_schema`, which `diff_tables` runs only after `resolve_key` returns successfully, so a pair that fails validation establishes nothing. That is unobservable today, because every declared-key failure is fatal and no diff is produced at all. It stops being unobservable as soon as a rejected declared key falls back to a guessed or row-numbered one, which is exactly what the row-number fallback step introduces; that step must then split component parsing and endpoint resolution from validation, so the identities survive into a diff the fallback produced. The queue entry for it now says so.

# Design

## Syntax and validation

A component is `name` or `old/new`. Splitting on `/` yields exactly one or two parts; three or more is `MalformedKeyComponent`, and an empty part on either side is the existing `EmptyKeyComponent`, since an empty half is an empty name. `a/a` is legal and means what `a` means, which needs no special case: the names match, so no rename is derived.

Uniqueness becomes a property of endpoints rather than of the component string. `--key a/b,a/c` claims `a` twice on the old side and `--key a/b,c/b` claims `b` twice on the new side; both are `DuplicateKeyColumn { side, column }`, as is `--key id,id/other`, which the current string-equality check would miss entirely. Each endpoint is resolved with the existing `column_index`, so a name absent from its side is still `MissingKeyColumn` with the side that lacks it.

`KeyColumn::name` becomes `component` and holds the user's spelling — `"id"` or `"customer_id/id"` — because it exists to name the component in `IncompatibleKeyTypes` and `InvalidKeyValue`, and naming half of a pair there would misdescribe what the user wrote.

## Reserving identities

`reconcile_schema` walks the old columns and matches each by name. It gains one step in front: the key's column pairs are claimed first, and their new endpoints are marked as taken before the name pass begins. An old column that a pair already claims takes that identity; every other old column matches a same-named new column that is still free.

The consequence to be deliberate about is that reservation consumes names. With `old = [a, b]`, `new = [a, b]` and `--key a/b`, the identity is `old.a → new.b`; `new.b` is taken, so `old.b` has nothing to match and is a drop, and `new.a` is unclaimed and is an addition. That is the bijection behaving correctly rather than an edge case, and it is the shape a rename hint will produce too, so it gets its own test.

Type compatibility is checked for every identity uniformly, reserved or not. A reserved pair has already been checked by key resolution, so the check cannot fire there, but keeping it unconditional preserves the local guarantee that `compare_cells` relies on when it expects every identity to have a comparison plan.

## Rendering

A rename is derived at render time: for each identity, compare the name at its old end with the name at its new end. Where they differ, emit

```text
col_rename("customer_id", "id")
```

These come first among the column operations, before drops and additions. Every later operation names its column by its old name, so stating the identity first is what makes `col_edit("customer_id", values)` legible when the new file has no such column.

Everything below that line then uses the new name. `col_rename("customer_id", "id")` reads as "customer_id became id", so a later `col_edit("customer_id", values)` would be naming a column the new file does not contain, and sending a reader to look for it there. Naming by the new name gives:

```text
col_rename("customer_id", "id")
col_order("id", 3 -> 1)
col_edit("id", values)
```

The rule is that an operation about an identity names it by its new name, while an operation about an unmatched column uses the only name it has: `col_drop()` keeps the old name because there is no other, and `col_add()` already uses the new one. Positions keep their `old -> new` form, which is explicitly a transition and reads correctly either way.

This is unobservable in the current output — every identity today has the same name at both ends — so no existing snapshot changes, and this step is where the choice first has consequences. `design.md`'s vocabulary table writes `col_edit([old1, old2, ...])`, which describes the operation's subject rather than its display, but it is close enough to read as a contradiction, so a sentence recording the display convention goes with it.

A paired key component renders as `"customer_id" -> "id"` inside the bracketed key list. The obvious alternative, listing both names, would make `--key a/b` and `--key a,b` produce identical output for entirely different keys. The arrow reuses `row_order()`'s idiom for an old-to-new pair, and each name keeps its own quoting.

# Verification

* `src/key.rs` unit tests cover: a paired component resolving to two indices; `a/b/c` rejected as malformed; `a/` and `/b` rejected as empty; an old endpoint claimed twice, a new endpoint claimed twice, and a plain component colliding with a paired one, each naming the side and column; a missing endpoint naming the side that lacks it; incompatible types across a pair naming the component as written; `a/a` accepted; and a compound key mixing plain and paired components.
* `src/schema.rs` unit tests cover a reserved identity beating name matching, including the `--key a/b` case above where the reservation turns `old.b` into a drop and `new.a` into an addition, and confirm the reserved identity is still marked `is_key` and keeps its old-column position in the identity list.
* `src/human.rs` tests pin both new renderings, the position of `col_rename()` among the column operations, and a renamed column that is also edited and reordered, whose `col_edit()` and `col_order()` lines must carry the new name while its `col_drop()`-side neighbours keep theirs. Every existing snapshot passes unchanged, because an identity's two names agree everywhere except behind a declared pair.
* `tests/diff.rs` asserts a complete `Diff` for a renamed key whose rows are also reordered and edited, showing that identity, row matching, ordering, and cells all follow the pair rather than the names, plus a repeated run that is structurally and byte-identical.
* `tests/cli.rs` snapshots `--key customer_id/id` end to end.
* The demo pair `demo/key-rename-*.parquet` renames its key column and edits one row, so the demo shows a rename and an edit together rather than the drop-and-add pair the same files produce without `--key`.

# Definition of done

This step is complete when:

* `--key` accepts `old/new` components alongside plain ones, in any combination within a compound key;
* a component naming a column absent from its side, a malformed or empty component, and an old or new column claimed by more than one component are each rejected with an error naming what is wrong;
* a declared pair establishes column identity, so the two columns are neither a drop nor an addition, and the columns whose names the pair consumed become one drop and one addition;
* an identity whose ends carry different names renders as `col_rename()`, ahead of the other column operations, and a paired key component renders distinguishably from two plain ones;
* no operation names a column by a name its side of the comparison does not have: identities are displayed under their new name, drops under their old one, and `design.md` records that convention;
* `DiffError::PairedKeyUnsupported` no longer exists, and duplicate key columns are detected by endpoint rather than by component string;
* the demo datasets and both READMEs document the paired syntax; and
* the full test suite, strict Clippy, formatting, and diff checks pass across the workspace, and repeated runs still produce byte-identical output.
