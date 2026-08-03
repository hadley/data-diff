---
title: Booleans join the numeric domain
---

# Todo

- [x] **Extend the matrix.** `Domain` in `src/compare.rs` gains `BoolInt` and `BoolDouble`. Canonicalization reads `true` as `1` and `false` as `0` in those domains — a boolean canonicalizes to `Int`, and a double follows the existing `IntDouble` exact rule, so `true` equals `1.0` through `Int(1)`. The `(Kind, Kind)` match becomes exhaustive: every pair of supported kinds now has a domain.
- [x] **Make `ComparisonPlan::new` infallible.** It returns `Self`, with `kind()` resolved by an `expect` naming the contract: `validate_tables` admits only the kinds the plans cover. Every arm that handled `None` disappears — the guess-candidate skip in `src/key.rs`, the `is_some_and` chains through `plan_for` in `src/rename.rs` and `src/swap.rs`, and the `expect`s in `src/cells.rs` and `src/swap.rs` that pointed at schema reconciliation.
- [x] **Retire the incompatible vocabulary.** `DiffError::IncompatibleColumns` and its `Display` arm, the whole-map check in `reconcile_schema`, `RejectionReason::IncompatibleTypes` and its rejection path in `declared_key`, `IssueKind::HintIncompatibleTypes` and its check in `hint::endpoints`, the `incompatible:` rendering arm in `src/human.rs`, and `incompatible` from the line grammar's fixed field set. None of them is constructible once every pair compares.
- [x] **Let the ripple simplify `src/lib.rs`.** `reconcile_schema` returns `()`, `run_pass` returns `Pass`, and the testing helper in `src/schema.rs` returns `ColumnMap`, its call sites losing their `unwrap()`s.
- [x] **Update `design.md`.** The comparison matrix gains its two boolean rows and loses the `Incompatible` row; comparison semantics records the exact-encoding rule and the non-transitive triangles it accepts; the rejected broadening of boolean string parsing is recorded so it is not re-proposed; the normalized-types section records the settled meaning of an incomparable pair for the types that will reintroduce one; and the sweep below removes compatibility language that no longer gates anything — key comparison, key rejection reasons, guessed-key eligibility, rename inference, and the hint issue kinds.
- [x] **Update `README.md`.** `incompatible_types` leaves the `key_invalid()` reasons list. (A sentence on cross-type value comparison was drafted for the output section and cut in review; the demo and the matrix in `design.md` carry the story.)
- [x] **Refresh the demo.** A new cross-type swap pair, written by `examples/generate_demo.rs`, joins the swaps section showing two apparent retypes resolved as an exchange. The value-edits section stays exactly as it was, by owner instruction; the plain retype case is covered by the integration and CLI tests instead. Transcripts verified by `cargo test --test readme`, prose written by hand, no orphaned fixtures.
- [x] **Cover it.** Unit tests for the new domains' canonicalization and edges; flipped tests where incompatibility was asserted; integration coverage in `tests/diff.rs` for each path under Verification; CLI snapshots in `tests/cli.rs` including the exit-status flip; determinism checks.
- [x] **Complete the acceptance pass.** `cargo build --workspace --all-targets`, `cargo test --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`, and byte-identical repeated runs.

# Goal

Changing a column from boolean to integer kills the whole comparison today: `reconcile_schema` pairs the two columns by name, finds no comparison plan for boolean ↔ numeric, and returns the fatal `DiffError::IncompatibleColumns`. One retyped column costs the user every other finding in the diff — and the retype it punishes is a common, well-defined representation change, the 0/1 encoding that `astype(int)`, SQL bit columns, and plenty of writers produce.

This step extends the comparison matrix instead: `true` compares equal to `1` and `false` to `0`, exactly, across both boolean ↔ int64 and boolean ↔ double. That is the same species of rule the matrix already applies everywhere else — `1` equals `1.0`, `"1"` equals `1`, `"true"` equals `true` — and it turns both of the queue item's motivating cases from fatal errors into their true descriptions. A faithful re-encoding is a type-only `col_edit(paid, type: Boolean -> Int64)` and nothing more, and two columns of those types whose contents were exchanged — which today reads as two impossible retypes and dies — resolve through the ordinary machinery: the same-name pairs form identities, both measure as rewritten, the crossings agree on identical source types, and swap inference reports the exchange exactly as it does for same-typed columns:

```console
$ data-diff demo/swap-types-old.parquet demo/swap-types-new.parquet --key id
col_key([id], basis: declared)
col_rename(flag -> count, basis: swapped)
col_rename(count -> flag, basis: swapped)
col_order(flag, 3 -> 2)
```

With the extension in place, every pair of supported kinds is comparable, so the *incompatible pair* ceases to exist in the MVP. The code that handled one — the fatal error, the `incompatible_types` key rejection, the `hint_incompatible_types` issue — becomes unconstructible and is removed rather than left as untestable arms. The question the queue item asked, what an incomparable pair *means*, is settled in `design.md` prose for the post-MVP types that will make one constructible again: identity requires comparability, and a same-named pair the matrix cannot relate reads as a drop and an addition.

# Scope

## What changes

- `src/compare.rs`: two new domains, their canonicalization, and an infallible `ComparisonPlan::new` over an exhaustive kind match.
- `src/schema.rs`: the post-hoc compatibility check goes; `reconcile_schema` becomes infallible.
- `src/key.rs`: the `IncompatibleTypes` rejection path and the guess-candidate compatibility skip go.
- `src/hint.rs`: the rename-hint compatibility check goes.
- `src/model.rs`, `src/human.rs`: the three retired vocabulary items and their rendering.
- `src/lib.rs`: `run_pass` loses its `Result`.
- `design.md`, `README.md`, `demo/README.md`, `examples/generate_demo.rs`, and the test suites.

## What stays and why

- **Normalization.** Booleans keep their own normalized type; nothing about how a column is read or displayed changes. Only comparability across the boolean/numeric boundary is new, and the domain decides canonicalization per pair as it always has.
- **String ↔ boolean parsing.** `bool::from_str` still accepts only `"true"` and `"false"`; `"1"` still does not equal `true`. The reasoning is in the design section, and the rejection is recorded in `design.md` so it is not re-proposed.
- **Swap inference's identical-source-type crossings.** The bar for an exchange is unchanged; the cross-type swap case passes it because the crossed columns *are* identically typed — that is what reveals the apparent retypes as a swap.
- **Input validation.** An unsupported column type remains fatal. Softening that is the broader-types step now queued in `plan-next.md`, which owns the unknown-type fallback.
- **Rename inference and thresholds.** Boolean/integer candidate pairs become measurable, and the existing $p_o$ and $\kappa$ thresholds judge them like any other low-cardinality pair; nothing is tuned here.

## Explicitly deferred

- **Broader parquet types and the unknown-type fallback.** Now a `plan-next.md` item: temporal, decimal, and binary types, nested data behind them, and a defined degradation for types the tool does not know, replacing today's fatal `UnsupportedColumn`. That step also revives the incomparable-pair machinery this one retires, per the reading recorded in `design.md`.
- **Accepting `"1"`/`"0"` as boolean strings.** Considered and rejected, see the design section; recorded in `design.md` as settled.
- **Interactive resolution of any of this.** The UI item owns overrides, as ever.

# Design

## The rule

`true` ≡ `1` and `false` ≡ `0`, exactly; every other number is unequal to both. This is encoding equality, not truthiness — `2` does not equal `true`, and neither does `-1` — mirroring the matrix's one existing cross-numeric rule, int64 ↔ double, where a double equals an integer only when it represents it exactly. Truthiness would import a language convention the data never asserted; the 0/1 encoding is the one convention writers actually emit. Mechanically, a boolean canonicalizes to `Int` in the two new domains, and the `BoolDouble` domain reuses the `IntDouble` arm's double handling, so `true` meets `1.0` at `Int(1)` and `false` meets `-0.0` at `Int(0)` without a new equality rule anywhere.

## Why extension, not the drop-and-add reading

The queue item offered three readings of an incompatible same-name pair: fatal, a drop and an addition, or an edit. Fatal fails on proportionality — every other defect of this shape is reported and worked around, and a retyped column is more ordinary than a broken key. The drop-and-add reading is the honest fallback *when values genuinely cannot be compared*: an identity is used — by cells, agreement, swap inference, change mass — so a pair no plan can serve has nothing to be an identity with. But for boolean ↔ numeric the premise is the tool's own choice rather than a fact about the data: the values compare perfectly well under the encoding every real writer uses, and refusing them was the decision actually up for review. Extending the matrix dissolves the case instead of styling its failure, and it reaches the descriptions the other readings approximate — the faithful retype is a type-only `col_edit()`, the true value change is a measured `col_edit()`, and the exchange is a swap, each on evidence the tool actually examined. The design's own preference for the most parsimonious reading decides this: one type change beats a drop plus an addition wherever both fit, and now both fit.

## What becomes structural

After the extension the `(Kind, Kind)` domain match is exhaustive — sixteen pairs, sixteen domains — so the compiler proves what the retired checks used to assert: every pair of supported kinds is comparable. `ComparisonPlan::new` returning `Self` makes the remaining fallibility explicit as a contract with `validate_tables`, which is the one place unsupported types are refused; the `expect` names it. Everything downstream simplifies honestly: no caller branches on a `None` that cannot occur, no error variant models a state that cannot arise, and `reconcile_schema` — whose only error this was — becomes infallible, taking `run_pass`'s `Result` with it.

Retiring the vocabulary is the honest half of that simplification. `RejectionReason::IncompatibleTypes` and `IssueKind::HintIncompatibleTypes` become unconstructible, and keeping them would mean documented reasons no run can produce and match arms no test can reach. They go now and return with the broader-types step, whose queue entry says so; the alternative — carrying them dormant — was considered and rejected because a vocabulary the format cannot emit is a promise the tests cannot hold.

## The triangles, owned

Comparison is defined per column pair, and with booleans in the numeric domain that pairwise definition stops composing: `"true"` equals `true` and `true` equals `1`, but `"true"` does not equal `1`, because string ↔ int parsing reads digits; `"1"` equals `1` and `1` equals `true`, but `"1"` does not equal `true`, because `bool::from_str` reads words. The current matrix has no such triangle, and this step accepts two. They are harmless mechanically — no stage ever chains comparisons across pairs — but `design.md` must own them as deliberate: each string domain parses by its partner column's spelling rules, and that is the whole of the definition.

Broadening boolean string parsing to accept `"1"`/`"0"` was considered as a patch and rejected. It closes one triangle while leaving the other — `"true"` versus `1` has no coherent fix at all — and it would read genuine digit strings as booleans in columns where `"1"` means one, inviting exactly the false equivalence the exact parsers exist to avoid. Pairwise incoherence at the corners is the cost of pragmatic per-pair semantics, and it is recorded rather than half-fixed.

## The reading on record

Post-MVP types will make incomparable pairs constructible again — `design.md` already sketches their regime: comparable only between identical source types, excluded from cross-type rename inference, hintable into identity. For when that happens, this step records the settled meaning the queue item asked for, as design prose beside that sketch: identity requires comparability, whoever proposes the pair — a name match, a declared component, a hint — so a same-named pair the matrix cannot relate is declined where it would be claimed and reads as a drop and an addition, never a fatal error and never an edit with unmeasurable counts. The broader-types step implements it when there is something real to implement against.

## Ripples accepted

A boolean column may now pair with a numeric one anywhere compatibility used to gate: as a declared key component (lawful, and useless past two rows for the same uniqueness reasons as any two-valued column), as a guess candidate (same), and as a rename-inference candidate, where a dropped boolean against an added 0/1 integer column is measured by the same $p_o$ and $\kappa$ thresholds as every other low-cardinality pair — chance correction is what those thresholds are for. Change mass, ordering, summarization, and reconsideration consume identities and cells as before and need no changes. Hash stability holds because canonicalization decides the hash: a boolean canonicalized to `Int(1)` hashes as the integer it now equals.

# Verification

- Unit tests in `src/compare.rs`: `true`/`1`, `false`/`0`, `true`/`1.0`, `false`/`-0.0` equal; `2`, `-1`, and `NaN` unequal to both booleans; null/null agrees and null/present disagrees across the new domains; the matrix test flips to assert every supported pair constructs a plan; the triangle facts (`"true"` ≠ `1`, `"1"` ≠ `true`) asserted so the recorded incoherence is pinned.
- Flipped unit tests: the incompatible-pair test in `src/schema.rs` becomes an identified boolean/integer pair whose equal-encoded values produce a type-only edit; the two `IncompatibleTypes` rejection tests in `src/key.rs` are replaced by a boolean/integer declared pair that validates; the hint and rendering tests for `hint_incompatible_types` are removed.
- Integration coverage in `tests/diff.rs`: a faithful bool→int retype diffs as a type-only `col_edit()` with no changed cells; an unfaithful one reports its changed cells; the cross-type exchange resolves as two `basis: swapped` renames with the expected `col_order()`; a boolean/integer pair declared as the key validates on a small fixture; rename inference relates a dropped boolean to an added 0/1 integer column on exact evidence.
- CLI snapshots in `tests/cli.rs`: the retype case exits zero with the diff on stdout, where it previously exited non-zero with an error on stderr.
- The demo held to real output by `tests/readme.rs`: the new cross-type swap pair, written by `examples/generate_demo.rs` and read by its section's command, with every pre-existing section byte-for-byte unchanged.
- Determinism: repeated runs byte-identical on every new path.

# Definition of done

This step is complete when:

- `true` compares equal to `1` and `false` to `0` across boolean ↔ int64 and boolean ↔ double, exactly and only;
- a faithful boolean re-encoding diffs as a type-only `col_edit()` and a cross-type exchange as a swap, with no pair of supported columns able to make `diff_tables` fail;
- `DiffError::IncompatibleColumns`, `RejectionReason::IncompatibleTypes`, and `IssueKind::HintIncompatibleTypes` are gone, `reconcile_schema` is infallible, and the every-pair-comparable fact is structural in the exhaustive domain match;
- `design.md` carries the extended matrix, the exact-encoding rule, the owned triangles, the rejected parsing broadening, and the recorded drop-and-add reading for future incomparable pairs; `README.md` matches; the demo shows both cases and `tests/readme.rs` holds it to real output; and
- the full test suite, strict Clippy, formatting, and diff checks pass across the workspace.
