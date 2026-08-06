---
title: Native verification and measurement for same-type pairs
---

# Todo

- [x] **Extend the native machinery from equality to agreement.** `compare.rs` gains a native full-row measurement beside `NativeEq`: for an eligible same-type pair under its identity plan, one pass over the matched rows counts agreeing rows through the same per-type equality and accumulates both sides' value frequencies into borrowed-key maps — `&str` for strings, normalized bits for floats, the raw integer for the widths, mantissas for decimals — so an `Agreement` is produced with no `Vec<CanonicalValue>` and no cloned map keys. The counts group exactly as canonical values do because each eligible type's canonicalization is injective up to the comparator's inline normalization — the same per-type arguments `NativeEq` already wrote, consulted rather than restated — so `rows`, `agreeing`, and `expected` come out identical to the materialized path's, and `expected()` still sums commutatively in `u128`.
- [x] **Verify natively.** `Aligned::verify`'s digest-equal branch asks the native comparator before materializing: an eligible pair confirms or refutes equality by comparing the arrays over every matched row, and only ineligible pairs still build both projections. The digest-differs shortcut, the memo, and the meter charge are untouched — a verification still costs its matched rows, and a collision is still caught by comparing values, just values read in place.
- [x] **Measure natively.** `Aligned::measure` under `Over::Full` asks for the native agreement first and falls back to `ensure_counts` exactly as today; the memo key and the charged cost do not move, so a native and a materialized measurement of the same pair are interchangeable, which the shared memo already assumes. `Over::Sampled` stays on the take-then-canonicalize path: it reads at most a sample and was never the cost.
- [x] **Gate on output identity against `45e4e72`.** The worktree comparison over all eight scenarios at sampling and exhaustion sizes, as the last two steps ran it; identical `Diff` digests everywhere, with any divergence read as a broken equivalence argument rather than a tolerance.
- [x] **Re-run the grid and record it.** `benches/README.md` gets the fresh baseline table and the measured statement for `renamed_strings`, whose verification and informativeness materialization this step exists to remove (~4 of its ~4.6s at 1M×10, 2026-08-06); the acceptance rule's two measured halves are re-verified, non-adversarial completeness on the string scenarios included.
- [x] **Update `design.md` and the leads.** The rename-inference section's fast-path sentence widens from discovery to verification and informativeness; the costs note stops attributing materialization to the exact stage for eligible types; the promoted lead leaves `plan-next.md` and what this step still does not touch — cross-type and dictionary pairs, the sampled paths — is stated where it was.
- [x] **Cover it.** Per-type tests beside the existing ones: the native `Agreement` equals the canonicalizing `Agreement` — `rows`, `agreeing`, and `expected` all three — over the same adversarial arrays the verdict tests use, nulls and normalization cases included; native verification agrees with materialized verification on equal, unequal, and forced-collision pairs, the last through the injected digest that disables the native path; ineligible pairs fall back and still measure; the pinned rename suite passes unchanged.
- [x] **Complete the acceptance pass.** `cargo build --workspace --all-targets`, `cargo test --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`, byte-identical repeated runs, and `cargo bench` compiling and running.

# Goal

The fast-path step removed materialization from cell comparison and digest discovery, and its own measurement shows what is left: `renamed_strings` at 1M×10 spends ~4 of its ~4.6 seconds in exact-rename verification and informativeness, which still build two `Vec<CanonicalValue>` per candidate pair and a frequency map of cloned values per column. Both flow through single choke points in `Aligned` — `verify` and `measure` under `Over::Full` — and both need answers that are pure functions of the raw arrays for exactly the types the fast path already argued: equality per matched row, and value frequencies that group as canonical values group, which injectivity gives for free. This step extends the native machinery to those two questions, under the same gate as before — bit-identical output, verified against the committed baseline, with every ineligible type falling back to the materializing path it uses today.

Nothing about evidence or budgets moves: a verification still reads and is charged every matched row, a collision is still caught by comparing values, informativeness still consults real frequencies, and the memoized `Agreement` is the same numbers whichever path produced it.

# Scope

## What changes

- `src/compare.rs`: the native full-row agreement beside `NativeEq`, with per-type borrowed-key counting.
- `src/agreement.rs`: `verify`'s digest-equal branch and `measure`'s `Over::Full` arm consulting the native path first.
- `benches/README.md`, `design.md`, `plan-next.md`, and the test suites.

## What stays and why

- **Every threshold and rule.** Agreement, kappa, informativeness, mutual uniqueness — the native path produces the same `Agreement` counts, so every judgement downstream reads the same numbers.
- **Charges and memos.** A full-row examination costs its matched rows whichever path answers it, and the memo stores the same value either way.
- **The sampled paths.** `Over::Sampled` reads at most `agreement_rows` values through take-then-canonicalize; it was never the cost and stays untouched.
- **The fallback.** Cross-type pairs, dictionaries, and opaque columns materialize exactly as today; the native path exists only where its answer is provably the same.

## Explicitly deferred

- **Cross-representation widenings and dictionary hydration**, as before — each wants its own argument.
- **`CellCoordinate` shrinking and parallelism**, still leads.

# Design

## Frequencies group natively because canonicalization is injective

`expected` is a sum over shared values of the two sides' frequency products, and its inputs are only the *partitions* of each column's matched values into equality classes. For every eligible type, raw equality (under the comparator's inline normalization) coincides with canonical equality — that is what eligibility means — so partitioning by borrowed raw keys yields the same classes, the same counts, and the same sum as partitioning by materialized canonical values. Nulls form one class keyed apart from every value, exactly as `CanonicalValue::Null` does. The per-type test asserts all three `Agreement` fields against the canonical path, so the argument is checked, not trusted.

## One pass, both questions

The native measurement walks the matched rows once, counting agreement and filling both frequency maps as it goes — the same shape as the materialized path's zip-plus-counts, minus the two materialized columns between the array and the arithmetic. Verification reuses the per-row equality alone with an early exit on the first disagreement, which the materialized `Vec` comparison also had.

## The injected digest still reaches everything it reached

Forced-collision tests disable the native digest through `with_digest`; the same flag governs native verification and measurement, so a test that forces every digest alike still drives the materialized machinery it was written to reach, and the fallback path keeps its full coverage.

# Verification

- Per-type native-equals-canonical `Agreement` tests and native-equals-materialized verification tests in `src/compare.rs` and `src/agreement.rs`, over the adversarial fixtures the verdict tests established.
- The pinned rename suite — pre-pass, digest join, budgets, uninformative refusals — passes unchanged.
- The worktree output-identity comparison against `45e4e72` over all eight scenarios.
- The grid re-run recorded in `benches/README.md`, with the `renamed_strings` before/after stated.

# Definition of done

This step is complete when:

- exact-rename verification and full-row measurement run natively for every eligible type and fall back everywhere else;
- the native `Agreement` is proven equal to the canonical one per type, and output is verified bit-identical to `45e4e72` across all scenarios;
- `renamed_strings`' before/after is measured and recorded, with the string scenarios still reporting nothing incomplete at any grid point;
- `design.md`'s fast-path sentences and costs note cover verification and measurement, and the promoted lead has left `plan-next.md`; and
- the full test suite, strict Clippy, formatting, and diff checks pass across the workspace.
