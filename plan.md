---
title: Same-type fast paths, gated on output identity
---

# Todo

- [x] **Add the string-heavy generator scenarios.** `test-support` gains `identical_strings` — the same all-string table twice, distinct values shaped like the integer floor's — and `renamed_strings`, every string column renamed in place with distinct values, the digest join's string case. Both are non-adversarial, seeded and reproducible like the rest, and join the benchmark grid and `benches/README.md`'s scenario table. They exist because the integer grid understates what this step removes: every string value clones its bytes into `CanonicalValue`, and cell-comparison canonicalization measured only ~3% on integer fixtures (2026-08-05).
- [x] **Take the string baseline before changing anything.** `cargo bench` over the two new scenarios plus a `sample` profile of `identical_strings` at 1M×10, recorded so the fast path's win is measured against evidence rather than assumed. The integer baseline is the 2026-08-06 table as it stands.
- [x] **Build the native comparator for cleanly-arguable same-type pairs.** A `NativeEq` behind one constructor — `NativeEq::for_pair(old_column, new_column, plan)` returning `Option<Self>` — that answers `equal(old_row, new_row)` straight off the arrow arrays, with `None` falling back to the canonicalizing path. Eligible types, each with its equivalence argument written at the dispatch: booleans and every integer width (each maps bijectively into the canonical `i64`, `u64` by wrapping cast); `Utf8`/`LargeUtf8` against itself (string-vs-string keeps its bytes); `Float32`/`Float64` with the canonical normalization applied inline — NaN collapses to one bit pattern and both zeros to one — because raw bits would wrongly split `-0.0` from `0.0` and NaN payloads from each other; timestamps of identical unit and awareness and `Date32`/`Date64` against themselves (exact scaling is injective); decimals of identical precision and scale (a fixed scale makes mantissa equality value equality). Everything else falls back: dictionaries (logical-null and hydration subtleties), opaque columns (their canonical form *is* the comparison), and every cross-type or cross-unit pair. Null equals null and differs from every value, exactly as canonical `Null` does.
- [x] **Use it in cell comparison.** `compare_cells` asks `NativeEq::for_pair` first and materializes two `Vec<CanonicalValue>` per column only on fallback; the matched-row loop and the fanout comparison consult the same comparator, so a fanout still costs no extra pass. The output is coordinates either way — nothing downstream reads the values `compare_cells` used to build.
- [x] **Stream same-type digests straight from arrow.** `Aligned`'s digest piece gains a native path for the same eligible types: per matched row, encode the value's canonical byte frame directly from the array — the same tag-and-bytes `encode_value` writes for the value's `CanonicalValue`, produced without constructing it — into the per-value `stable_hash` and the column's `sequence_hash` frame. Digests stay bit-identical because the encoding is a pure function of the canonical value and the canonical value is a pure function of the raw value for exactly these types; the golden-digest tests plus a per-type native-equals-materialized digest test hold it there. Verification and counts still materialize on first demand, unchanged — the digest join's discovery pass is what stops paying for `Vec<CanonicalValue>`, and verification only runs on digest-equal candidates under the row budget.
- [x] **Take the profiled ride-alongs.** Three output-identical cleanups the 2026-08-05 leads recorded, each a few lines beside the code it fixes: `KeyIndex::new` pre-sizes its digest map from the row count and remembers each row's bucket id from the counting pass instead of looking every digest up twice; `minimal_moves` returns empty on an already-increasing sequence before building the Fenwick tree, the commonest case of all; `RowSample::select` partitions with `select_nth_unstable` at the cap and sorts only the kept prefix — the selected set is identical because `(digest, position)` is a total order with distinct positions.
- [x] **Verify output identity against the committed baseline.** Build `42bac58` in a worktree with the new generators copied in, and compare complete-`Diff` digests over every scenario — the six integer ones and the two string ones — at sizes that exercise sampling and budget exhaustion. Byte-identical everywhere is the gate this step's title promises; any divergence is a bug in an equivalence argument, not a tuning knob.
- [x] **Re-run the grid and record it.** The benchmark grid including the new scenarios; `benches/README.md` gains the string rows in the current baseline table and the before/after for the string scenarios; the acceptance rule's recorded multipliers are refreshed where they moved and the non-adversarial half now covers the string scenarios too.
- [x] **Update `design.md`.** The value-changes and rename sections record the fast path in a sentence each: same-type columns compare and digest natively where the canonical verdict is a pure function of the raw values, each type's argument lives at the dispatch, and any type without a clean argument keeps the canonicalizing path — an optimization with identical output, never a mode. The costs note stops attributing a full materialization to cell comparison and the digest join for eligible types.
- [x] **Cover it.** Unit tests: for each eligible type, the native verdict equals the canonicalizing verdict over adversarial values — NaN payloads, `-0.0` against `0.0`, nulls on one and both sides, extreme integers per width, equal strings of unequal capacity, boundary decimals — pinned by comparing both paths over the same arrays; native digests equal materialized digests per type, alongside the golden digests; dictionary and cross-type pairs return `None` and fall back; the ride-alongs pin selection identity (forced-collision tie-break unchanged) and the early-out (an ordered matching yields no moves, a rotated one still does).
- [x] **Complete the acceptance pass.** `cargo build --workspace --all-targets`, `cargo test --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`, byte-identical repeated runs, and `cargo bench` compiling and running.

# Goal

Cell comparison and exact-rename discovery each materialize two `Vec<CanonicalValue>` per column before comparing or hashing a single value, and for most real tables that materialization is pure ceremony: both sides are the same type, and the canonical equality verdict is a pure function of the raw arrow values. On the integer grid this ceremony is a few percent; on string tables every value clones its bytes into the enum, which is why the step starts by building the string scenarios that can measure it honestly. The fast path compares and digests straight off the arrow arrays for exactly the types where that is provably the same answer — each type's argument written down, every type without one falling back — so the output is bit-identical by construction and verified against the committed baseline, not assumed. Three small profiled cleanups from the 2026-08-05 leads ride along under the same gate.

This is an optimization, never a mode: no flag, no changed answer, no weakened evidence. A collision still costs time and never correctness, because nothing here touches what hashes decide — the digest path produces the same digests from the same canonical bytes, and equality is still equality of values.

# Scope

## What changes

- `test-support/src/generate.rs`, `benches/pipeline.rs`: the two string scenarios.
- `src/compare.rs`: `NativeEq` and the native digest encoding, beside the canonicalization they mirror.
- `src/cells.rs`: `compare_cells` consulting `NativeEq` before materializing.
- `src/agreement.rs`: the digest piece's native path; `RowSample::select`'s partial sort.
- `src/key.rs`: `KeyIndex::new` pre-sizing and single-lookup construction.
- `src/order.rs`: the `minimal_moves` early-out.
- `benches/README.md`, `design.md`, and the test suites.

## What stays and why

- **Every canonical semantic.** Cross-type comparison, parsing, unit conversion, NaN and zero normalization, null rules — the fast path exists only where it reproduces them exactly, and the fallback is always the canonicalizing path itself, so no input can reach different semantics.
- **Digest values, bit for bit.** The native encoding writes the same bytes `encode_value` writes; golden digests and per-type digest-equality tests pin it. Sampling, the digest join, and the budgets all read the same numbers as before.
- **Verification and measurement materialization.** Digest-equal candidates are still verified over materialized values under the row budget; agreement is still measured over canonical values. Only discovery and cell equality stop materializing.
- **Budgets, meters, and incompleteness reporting.** Untouched; charges are the same rows for the same examinations.

## Explicitly deferred

- **Cross-representation fast paths.** Same family, different unit or scale (a seconds column against a milliseconds one) is a bijection argument away, but each widening deserves its own review; same `DataType` is the strict, obviously-sound start.
- **Dictionary fast paths and opaque row-byte comparison.** Both have hydration and logical-null subtleties; both fall back today and lose only the optimization.
- **Shrinking `CellCoordinate`.** A model change with its own trade-offs, recorded in the leads.
- **Parallelism.** Different risk class; the per-column seams this step cleans up are where it would land later.

# Design

## The eligibility rule is an argument, not a type list

A pair is eligible when the canonical equality verdict is a pure function of the raw values — equivalently, when canonicalization restricted to that type is injective up to the normalization the comparator applies inline. Each arm of `NativeEq::for_pair` states its argument in a sentence; anything that would need a paragraph falls back instead. That keeps the reviewer's question local — is this one argument sound? — and makes the default safe: falling back costs the materialization we pay today, never a wrong answer.

## Digests are the same bytes by construction

The streaming hasher already separates encoding from hashing: `encode_value` writes a canonical byte frame into a `Sink`. The native digest path writes that same frame from the raw value without building the `CanonicalValue` in between, so bit-identity is a property of the code shape — one encoding, two producers — and the per-type test that native and materialized digests agree closes the loop.

## Identity is verified against the baseline, not argued from it

The equivalence arguments justify the design; the worktree comparison proves the build. Complete-`Diff` digests against `42bac58` across every scenario and the exercised sampling and exhaustion paths make "output identity" an observed fact of this change, the same gate the constant-factor step used.

# Verification

- Per-type native-equals-canonical verdict tests over adversarial values, and native-equals-materialized digest tests, in `src/compare.rs`'s test module beside the golden digests.
- `compare_cells` pinned unchanged over its existing cases, plus a string-table case that exercises the native path and a dictionary case that exercises the fallback.
- Ride-along pins: sample selection identical under the forced-collision digest; ordered matchings yield no moves and the existing move cases still yield theirs; `KeyIndex` behavior pinned by its existing tests.
- The worktree output-identity comparison over all eight scenarios.
- The full grid re-run recorded in `benches/README.md`, string scenarios included.

# Definition of done

This step is complete when:

- same-type cell comparison and digest discovery run natively for every type with a written equivalence argument, and every other pair falls back to the canonicalizing path;
- the string scenarios exist, their before/after is recorded, and the measured win is stated in `benches/README.md`;
- output is verified bit-identical to `42bac58` across all scenarios, and digests are pinned by golden and per-type tests;
- the three ride-along cleanups are in with their identity pins;
- `design.md` records the fast path and its fallback rule; and
- the full test suite, strict Clippy, formatting, and diff checks pass across the workspace.
