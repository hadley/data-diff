---
title: Guess eligible single-column keys
---

# Todo

- [ ] **Separate declared and guessed key resolution.** Preserve the current syntax and strict validation path whenever `DiffOptions.key` is non-empty. When it is empty, try key guessing instead of immediately returning `MissingKey`. Keep declared compound keys supported; guessed keys are single-column only.
- [ ] **Reject impossible guesses before candidate work.** After choosing automatic resolution, check the two row counts before enumerating or canonicalizing columns. If either input has zero rows, return `MissingKey` immediately because no shared value can support a guess.
- [ ] **Build exact candidate evidence.** Examine same-name column identities in stable old-column order. For each compatible pair, canonicalize both sides with one `ComparisonPlan`, then retain it only if neither side contains null or `NaN`, values are unique independently on both sides, and the sides share at least one canonical value. Count shared values with hash buckets plus equality checks so collisions cannot manufacture overlap.
- [ ] **Select one deterministic guessed key.** Choose the eligible candidate with the greatest number of shared values and break ties by old-column position. Return the already-canonicalized values with the selected column so row matching does not repeat work. If no candidate is eligible, return `MissingKey`.
- [ ] **Record exact overlap evidence.** Add `Guessed` to `KeyBasis`, retain the selected collapsed column coordinate, and store exact `shared` and `possible` counts. Serialize those counts as the normalized ratio `shared / min(old_rows, new_rows)` while preserving `Eq` on the result model. Declared keys have no overlap.
- [ ] **Expose every resolved key in human output.** Lead output with `col_key(declared: [...])` for an explicit key or `col_key(guessed: ..., overlap: ...)` for a guessed key. Use the existing quoting rules for column names and serialize the same normalized overlap value used by JSON.
- [ ] **Make declaration an optional override.** Remove the CLI requirement that `--key` be present. An omitted flag attempts guessing; a supplied simple or compound key always takes precedence and is reported as `declared`, even when another column would be the strongest guess. Keep errors in an explicitly supplied key fatal rather than silently substituting a guess.
- [ ] **Integrate guessing through reconciliation.** Propagate the resolved basis, columns, and overlap from key resolution into `Diff.key`, while leaving schema identity, row matching, ordering, cell evidence, and edit summarization driven by the selected key exactly as they are for a declared key.
- [ ] **Complete the acceptance pass.** Add focused inline unit tests, library-level integration coverage, CLI coverage for omission and override, human and JSON snapshots, byte-identical repeated-output checks, and documentation examples. Run the full tests, strict Clippy, formatting, and diff checks.

# Goal

Make the common invocation work without requiring the user to know the row key when the inputs contain an eligible same-name single column:

```console
data-diff old.parquet new.parquet
```

`data-diff` should select the column with the strongest exact cross-table evidence, use it for reconciliation, and expose the selected basis and normalized overlap. A user who knows the correct identity can override the guess explicitly:

```console
data-diff old.parquet new.parquet --key account_id,revision
```

The guess is an evidence-backed input to the existing pipeline, not a schema or row event. Downstream reconciliation must receive the same canonical key values it receives today, so complete cell evidence and deterministic output retain their current meanings.

# Scope

This step adds automatic selection among provisional same-name identities. Rename inference and paired key names do not exist yet, so an identified candidate in this step is exactly one column name present once on each side. Added, dropped, and differently named columns are not candidates.

The resolution rule is:

1. If `DiffOptions.key` is non-empty, validate and use it exactly as today.
2. Otherwise, if either input has zero rows, return `MissingKey` without enumerating columns.
3. Evaluate every same-name single-column pair.
4. Discard a pair unless its types are compatible under `ComparisonPlan`.
5. Discard a pair if either side contains null or `NaN`.
6. Discard a pair unless its canonical values are unique on both sides.
7. Compute the exact intersection size of its canonical old and new values and discard it when that size is zero.
8. Select the greatest intersection size, breaking ties by old-column order.
9. If no candidate remains, return `MissingKey`.

Uniqueness and overlap use values canonicalized for that specific old/new type pair. Thus cross-type representations such as string and integer may form a guessed key when the existing comparison rules make their values equal. Unparseable strings remain tagged string values and participate normally; missing values and `NaN` make the whole candidate ineligible.

The normalized overlap is descriptive evidence:

```text
overlap = shared_values / min(old_rows, new_rows)
```

It does not alter ranking because the denominator is the same for every candidate. An eligible guessed key always has a non-zero denominator and a ratio in `(0, 1]`.

An explicit declaration is the override mechanism for this non-interactive step. A supplied key is never compared with guesses and never silently replaced when it is invalid. Existing missing-column, incompatible-type, missing-value, non-unique-old, and unsupported-fanout errors therefore retain their current behavior for declared keys.

Explicitly deferred:

* row-number fallback and the audit of which reconciliation stages are valid without a semantic key;
* paired old/new key components and rename-aware identities;
* recovery from an invalid declared key via an issue plus a guessed fallback;
* guessed compound keys;
* accepting duplicate new-side values as bounded fanout;
* candidate lists, interactive confirmation, and an in-process rerun UI;
* sampling, budgets, approximate overlap, and partial key searches.

Row-number fallback is the first item in `plan-next.md`. That step will decide how schema reconciliation, row and column ordering, changed-cell interpretation, edit summarization, rename inference, and incomplete-stage reporting behave when row position is the only basis. Reserve `col_key(row_number)` as its human representation, but do not emit it in this step.

Reuse the existing public `MissingKey` variant when automatic guessing is impossible or exhausts its candidates, and update its message so it explains that no key was supplied and no eligible key could be guessed. Errors in a non-empty declared key remain specific and fatal.

# Key-resolution design

## Declared path

Keep syntax validation and compound-key assembly separate from guessing. `validate_components()` continues to reject empty, paired, or repeated declared components. Each declared component continues to require a column on both sides, a compatible comparison plan, present values, and independent uniqueness.

The resolved internal key should carry:

```rust
struct ResolvedKey {
    basis: KeyBasis,
    columns: Vec<KeyColumn>,
    old: Vec<Vec<CanonicalValue>>,
    new: Vec<Vec<CanonicalValue>>,
    overlap: Option<KeyOverlap>,
}
```

Declared keys set `basis: Declared` and `overlap: None`. The public result continues to contain every component coordinate in declaration order.

## Guessed candidates

Keep candidate evaluation in `key.rs`, next to canonicalization and validation, rather than coupling it to `schema.rs` or row matching. Once both row counts are known to be non-zero, enumerate old schema fields in position order, find the unique same-name field in the already name-validated new schema, and skip incompatible pairs.

Candidate rejection is ordinary control flow, not a `DiffError`: a nullable, duplicated, incompatible, or disjoint column simply is not a guess. Reuse small validation and intersection helpers where their semantics match declared-key validation, but retain declared errors with their current row and side context.

Use stable hashes only as bucket indexes. Confirm canonical equality inside a bucket both when checking uniqueness and when counting cross-side matches. Because eligible candidates are unique on each side, every matched canonical value contributes exactly one to the intersection. Put this logic behind a small helper that accepts a value-hash function: production passes `stable_hash`, while tests pass a constant function to force every distinct value into one bucket and prove that collisions do not create duplicates or overlap.

Store canonical columns in each candidate and move the winning vectors into `ResolvedKey`; do not canonicalize the winner a second time. Candidate comparison should use a tuple equivalent to:

```text
(Reverse(shared_values), old_column_position)
```

No `HashMap` or filesystem iteration order may influence enumeration, selection, or output.

# Result and interface

Extend `KeyBasis` with `Guessed`. Model overlap exactly:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyOverlap {
    pub shared: usize,
    pub possible: usize,
}
```

`KeyDiff` stores `Option<KeyOverlap>`. `KeyOverlap` implements `Serialize` as the numeric quotient `shared / possible`, so guessed JSON remains compact:

```json
{
  "key": {
    "basis": "guessed",
    "columns": [3],
    "overlap": 0.6666666666666666
  }
}
```

The selected column uses the existing collapsed, one-based coordinate, so a same-name column moved from old position 1 to new position 3 is `[1, 3]`. Keeping overlap absent for declared keys avoids attaching guess-specific evidence to an explicit choice. Storing exact counts preserves `Eq` on `KeyOverlap`, `KeyDiff`, and `Diff`; the quotient can never be `NaN` or infinite because a guessed key has `possible > 0`.

At the CLI boundary, `--key` keeps its comma-delimited syntax but is no longer required. Help and README text should explain that omission attempts one same-name guessed key, failure asks for `--key`, and an explicit declaration overrides guessing.

Human output always announces the resolved key before change operations. Column names use the existing JSON-string quoting so unusual names remain unambiguous:

```text
col_key(declared: ["account_id", "revision"])
col_key(guessed: "customer_id", overlap: 0.6666666666666666)
```

Only one line appears. `col_key` is informational context rather than a change operation, so `no_changes()` still follows it when no change operations exist. Declared keys use component declaration order; guessed keys use the selected old-side name and the same normalized overlap serialization as JSON. The future row-number step reserves `col_key(row_number)`.

# Verification

Keep unit tests inline in `key.rs`. Use compact in-memory tables to cover:

* an empty old side and an empty new side returning `MissingKey` before any candidate canonicalization;
* one eligible same-type column;
* compatible cross-type values;
* rejection for null, `NaN`, duplicates on either side, incompatible types, and zero shared values;
* preference for the largest exact intersection;
* an equal-intersection tie resolved by old-column order;
* forced hash collisions, using an injected constant hash function, that cannot create false duplicates or false overlap;
* reuse of selected canonical values in the returned key;
* declared compound keys bypassing the automatic zero-row check and guessing;
* invalid declared keys retaining their existing errors;
* no eligible automatic candidate returning `MissingKey`; and
* repeated resolution returning the same candidate and exact overlap.

Add library integration coverage showing that:

* default options guess a key and correctly align reordered rows;
* `Diff.key` reports `guessed`, the collapsed column coordinate, and exact overlap;
* an explicit key overrides a stronger eligible guess and reports `declared`;
* moved key columns retain paired coordinates through schema reconciliation;
* a guessed key remains excluded from top-level changed cells;
* empty automatic inputs and non-empty inputs without a candidate return `MissingKey`;
* empty inputs still reconcile when a valid key is declared;
* repeated complete comparisons serialize to byte-identical JSON.

Add human-output snapshots for a declared compound key, a guessed key with normalized overlap followed by change operations, and each basis followed by `no_changes()` for identical tables. Verify quoting with at least one unusual declared or guessed column name.

At the CLI boundary, update the help snapshot, add one successful invocation without `--key`, add one omitted-key failure that reports `MissingKey`, and keep one explicit-key invocation proving the override syntax. One compact JSON assertion is enough to cover the guessed basis and overlap; do not duplicate the full eligibility matrix outside `key.rs`.

Update the README and demo guidance so the primary example exercises guessing and an adjacent example demonstrates `--key` as an override. Explain that no eligible guess currently fails and that row-number fallback is planned separately. Retain an explicit key in fixtures whose purpose depends on a particular compound key or on an error from a declared key.

# Definition of done

This step is complete when:

* automatic resolution rejects an empty side before enumerating or canonicalizing candidates;
* omitting a key deterministically selects the eligible same-name single column with the greatest exact shared-value count;
* every guessed key is compatible, present, unique on both sides, and supported by at least one shared canonical value;
* ties resolve by old-column order and repeated runs are byte-identical;
* guessed results expose their basis, selected coordinate, exact overlap evidence, and normalized JSON and human ratios;
* human output reports declared and guessed keys with the unified `col_key(...)` syntax;
* an explicit simple or compound key takes precedence and retains strict validation;
* absence of an eligible guess returns the existing `MissingKey` error;
* row-number fallback is explicitly queued as a separate design and implementation step;
* existing schema, row, cell, summary, and declared-key behavior remains covered;
* documentation describes automatic guessing, explicit override, and the current no-guess limitation; and
* the full test suite, strict Clippy, formatting, and diff checks pass.
