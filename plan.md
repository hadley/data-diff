---
title: Cut the pipeline's constant factor on large inputs
---

# Todo

- [x] **Pin the hash encodings before touching them.** Golden-digest unit tests in `src/compare.rs` hard-code today's `u128` digests for one value of every `CanonicalValue` variant — including a string long enough to cross xxh3's streaming block boundary — and for a representative key tuple through `sequence_hash`. The row sample is the bottom `agreement_rows` key digests, so these values reaching output is exactly what makes bit-identical digests, not merely run-to-run determinism, the regression gate for everything below.
- [x] **Stream values into the hasher.** `hash_with` stops materializing a `Vec<u8>` per value — the profile's single largest allocation source, several `realloc`s per value hashed — and instead feeds the identical byte sequence to the hasher incrementally: fixed-size variants through a stack buffer, variable-size ones through xxh3's streaming `update`. `sequence_hash` streams its same length-prefixed frame the same way. The `StableHasher` injection point for forced collisions survives; the encoding lives in one place serving both the streaming production path and the byte-buffer test path, so the two cannot drift, and the golden tests hold both to today's digests.
- [x] **Hash each key tuple once and carry the digests.** `ResolvedKey` stores each side's key digests beside the values, computed once where the keys are built (with the same injection tests use to force collisions today). `KeyIndex` takes the precomputed digests instead of re-hashing every tuple on construction and again on every `rows()` lookup; `validate_unique_old` catches its duplicate while the buckets fill instead of re-querying per row; `match_rows` and `RowSample::select` read the stored digests. Today the old side is sequence-hashed three to four times per pass and the new side twice — the profile shows ~23% of the identical run inside key resolution and another ~11% in `match_rows`, most of it this rework. The sample selection remains the same pure function of the same digests, so selected rows do not move.
- [x] **Flatten the per-row key representation.** `ResolvedKey`'s `old` and `new` become a width-strided flat store — one `Vec<CanonicalValue>` plus the component count, indexed as `row(i) -> &[CanonicalValue]` — replacing `Vec<Vec<CanonicalValue>>`'s heap allocation per row. `transpose` fills it without per-row `Vec`s, a single-component key (the common case) moves the canonicalized column in whole rather than cloning value by value, and `positional_key` writes the flat form directly.
- [x] **Take the per-row allocations out of matching.** `match_rows` stops `collect`ing each key's group into a fresh `Vec` — one allocation and free per old row, the attributed source of the allocator time under it — reusing one scratch buffer across rows. `KeyIndex` stops allocating a `Vec<usize>` per distinct key: buckets become compact groups behind a `u128`-keyed map with an in-repo identity-style hasher, the keys already being xxh3 output, with group rows stored in one shared vector. Buckets still fill in row order and lookups still filter rather than sort, so results cannot depend on map iteration order.
- [x] **Make `Aligned` projections lazy and sample-sized.** The per-`(side, column, plan)` cache entry splits into independently built pieces — sampled values, full values, digest, full counts — each constructed on first demand and kept for good. A sampled measurement takes the sampled matched rows out of the arrow column (`arrow-select`'s `take`) and canonicalizes just those, canonicalization being value-wise and therefore commuting with `take`; `digest`, `verify`, and `measure_full` build the full pieces exactly as today. This is the profile's headline: on an identical 1M×10 pair, 57% of all samples sit under swap inference's rewritten filter forcing full-column projections — full canonicalization, a full digest, and a million-entry counts map per column — to answer a 4096-row sampled question. Memo keys, meter charging, and every threshold are untouched: budgets change when work runs, and this changes only how much a sampled unit costs.
- [x] **Retire SipHash from the hot maps.** The projection counts maps (`HashMap<CanonicalValue, u64>`) and the other per-row-scaled maps get a deterministic xxh3-backed `BuildHasher` defined in-repo, no new dependency; std's default SipHash is ~25% of the identical run's samples. Safe per use because no such map's iteration order reaches output — `expected()` sums commutatively in `u128` — and each converted map's consumption is checked for that property as it is converted.
- [x] **Re-run the benchmark grid and record it.** `cargo bench --bench pipeline` over the full grid before and after; `benches/README.md` gets the new dated baseline table and keeps the old one for comparison, the acceptance rule is re-verified in both halves — the floor itself drops, so every adversarial ratio is re-measured rather than assumed, and the tuned budget constants change only if the rule demands it — and its profiling section's pointer to `plan-next.md`'s constant-factor item is rewritten to point at this plan's evidence, that queue item no longer existing.
- [x] **Update `design.md`'s costs note.** Present tense, re-dated: projection construction is lazy — a sampled question builds a sample-sized projection and full projections are built at most once per (column, plan) when a full-row question first asks; each side's key tuples are hashed exactly once per pass; the rest of the note stands.
- [x] **Cover it.** Unit tests: the golden digests; streaming-equals-buffered for every variant; the flat key store round-trips and the forced-collision tie-breaks stay reachable through the moved injection points; a sampled measurement leaves the full-projection pieces unbuilt, asserted in-module on the cache; `match_rows` behavior pinned unchanged over its existing cases. Integration: the full suite, CLI snapshots, and `tests/readme.rs` transcripts pass byte-identical with no snapshot churn; repeated runs byte-identical on sampled and unsampled paths.
- [x] **Complete the acceptance pass.** `cargo build --workspace --all-targets`, `cargo test --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`, byte-identical repeated runs, and `cargo bench` compiling and running.

# Goal

The budgets step made reconciliation's searches bounded; this step cuts what the remaining linear work costs per row. A fresh profile (2026-08-05, macOS `sample`, identical 1M×10 at ~3.75s) attributes the run as: 57% under swap inference's rewritten filter, which forces `Aligned::fill` to canonicalize, digest, and count full million-row columns to answer 4096-row sampled measurements; ~23% in key resolution and ~11% in `match_rows`, dominated by re-hashing the same key tuples up to four times, a heap `Vec` per row in `ResolvedKey`, a `Vec` per distinct key in `KeyIndex`, and a `collect` per row in `match_rows`; and throughout, `stable_hash` growing a fresh `Vec<u8>` per value through repeated `realloc` and std's SipHash burning ~25% of samples in maps whose order never reaches output. The genuinely necessary linear passes — canonicalization, xxh3 over the bytes, cell comparison, the LCS — are a small minority. The rename-heavy and rewrite-heavy scenarios tell the same story from different angles: `renamed_distinct` spends its extra time in the exact stage's full-column digests re-paying the per-value allocation, and `full_rewrite` pays the rewritten filter on every same-name pair before the retained cell diff.

None of this is semantic. Every fix preserves each digest bit-for-bit (the sample selection is a function of those bits), every threshold, every memo and budget semantics, and therefore every byte of output; the wins come from not allocating per value, not hashing the same tuple twice, and not building million-row projections behind sample-sized questions.

# Scope

## What changes

- `src/compare.rs`: streaming `stable_hash`/`sequence_hash` over the identical byte encoding; golden-digest tests.
- `src/key.rs`: cached key digests, the flat width-strided key store, compact `KeyIndex` buckets, `validate_unique_old` without per-row lookups.
- `src/rows.rs`: the reused scratch buffer in `match_rows`.
- `src/agreement.rs`: lazy per-piece projections; `RowSample::select` reading cached digests.
- A shared in-repo xxh3-backed `BuildHasher` for the hot maps, wherever it naturally lives.
- `benches/README.md`: the new baseline table and the corrected profiling pointer.
- `design.md`: the costs note.

## What stays and why

- **Every digest value.** The byte encoding fed to xxh3 is unchanged, so `stable_hash`, `sequence_hash`, column digests, and the bottom-k sample selection produce today's exact values; output stays byte-identical even where sampling binds, which is the regression gate the queue item set.
- **All inference semantics.** Thresholds, memo keys, meter charging, examination order, budgets and their defaults — untouched. If re-verifying the acceptance rule against the faster floor demands a constant change, that change is made under the recorded tuning rule and called out for review, not slipped in.
- **The complete cell-level diff.** `compare_cells` and `changed_cells` remain full passes; their cost is the retained invariant, not overhead.
- **Determinism.** No map whose iteration order reaches output changes hasher; bucket and group orders remain row-fill orders.

## Explicitly deferred

- **Sharing canonicalization between `compare_cells` and `Aligned`.** Measured ~3% on the integer fixtures; string-heavy tables would raise it, but that wants a string generator scenario and its own look.
- **A string-heavy benchmark scenario.** Worth adding when something needs it; today's fixes are shape-level and the integer grid already exposes them.
- **Parallelism and arrow-kernel vectorized comparison.** Different risk class; nothing here forecloses them.
- **Bounding key guessing and uniqueness checks.** Stays deferred as before; both get faster here anyway through the shared primitives.
- **CLI/UI exposure of budgets.** Unchanged, still queued behind UI decisions.

# Verification

- Golden-digest tests pass before and after the streaming rewrite, pinning the encoding; streaming-equals-buffered covered per variant.
- The full test suite, CLI snapshots, and demo transcripts pass with zero snapshot updates — `UPDATE_README=1` must not be needed.
- Repeated runs byte-identical on paths with and without sampling, with and without binding budgets.
- The benchmark grid re-run shows the identical 1M×10 floor at most half its 2026-08-04 baseline (3.59s), improvement at every 100k+ row point, no regression at the small end, and the acceptance rule holding at every grid point.

# Definition of done

This step is complete when:

- no value hashed by `stable_hash`/`sequence_hash` allocates on the heap for its encoding, and the digests are proven bit-identical by golden tests;
- each side's key tuples are sequence-hashed exactly once per pass, held in a flat per-side store with no per-row heap allocation, and `KeyIndex` allocates no per-key `Vec`;
- a sampled agreement measurement builds only sample-sized projections, with full canonicalization, digests, and counts deferred until a full-row question asks;
- the hot maps hash with the in-repo deterministic hasher and every converted map is checked order-independent at its consumption site;
- the entire test suite, snapshots, and demo transcripts pass byte-identical with no refreshed fixtures;
- the benchmark grid is re-run, recorded in `benches/README.md` with the acceptance rule re-verified, and the identical 1M×10 floor is at most half its previous baseline; and
- the full test suite, strict Clippy, formatting, and diff checks pass across the workspace.
