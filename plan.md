---
title: Guess eligible single-column keys
---

# Todo

- [ ] **Separate declared and guessed key resolution.** Preserve the current
  syntax and strict validation path whenever `DiffOptions.key` is non-empty.
  When it is empty, enter a guessing path instead of returning `MissingKey`.
  Keep declared compound keys supported; guessed keys are single-column only.
- [ ] **Build exact candidate evidence.** Examine same-name column identities
  in stable old-column order. For each compatible pair, canonicalize both sides
  with one `ComparisonPlan`, then retain it only if both inputs are non-empty,
  neither side contains null or `NaN`, values are unique independently on both
  sides, and the sides share at least one canonical value. Count shared values
  with hash buckets plus equality checks so hash collisions cannot manufacture
  overlap.
- [ ] **Select one deterministic guessed key.** Choose the eligible candidate
  with the greatest number of shared values and break ties by old-column
  position. Return the already-canonicalized values with the selected column so
  row matching does not repeat work. If there is no eligible candidate, return
  a dedicated error that tells the caller to supply a key.
- [ ] **Record the inference at the result boundary.** Add `Guessed` to
  `KeyBasis`, retain the selected collapsed column coordinate, and report the
  selected candidate's normalized overlap
  `shared / min(old_rows, new_rows)`. Keep declared-key JSON unchanged by
  omitting overlap when the basis is `declared`. In human output, announce a
  guessed key with a leading `col_key(...)` line; declared keys stay silent.
- [ ] **Make declaration an optional override.** Remove the CLI requirement
  that `--key` be present. An omitted flag uses guessing; a supplied simple or
  compound key always takes precedence and is reported as `declared`, even when
  a different column would be the strongest guess. Keep errors in an explicitly
  supplied key fatal rather than silently substituting a guess.
- [ ] **Integrate guessing through reconciliation.** Propagate the resolved
  basis and overlap from key resolution into `Diff.key`, while leaving schema
  identity, row matching, ordering, cell evidence, and edit summarization
  driven by the selected key exactly as they are for a declared key.
- [ ] **Complete the acceptance pass.** Add focused inline unit tests,
  library-level integration coverage, CLI coverage for omission and override,
  byte-identical repeated-output checks, and documentation examples. Run the
  full tests, strict Clippy, formatting, and diff checks.

# Goal

Make the common invocation work without requiring the user to know the row key:

```console
data-diff old.parquet new.parquet
```

When both inputs contain an eligible same-name single column, `data-diff`
should select the column with the strongest exact cross-table evidence, use it
for reconciliation, and expose that the key was guessed. A user who knows the
correct identity can override the inference explicitly:

```console
data-diff old.parquet new.parquet --key account_id,revision
```

The guess is an evidence-backed input to the existing pipeline, not a schema or
row event. Downstream reconciliation must receive the same canonical key values
it receives today, so complete cell evidence and deterministic output retain
their current meanings.

# Scope

This step adds automatic selection among provisional same-name identities.
Rename inference and paired key names do not exist yet, so an identified
candidate in this step is exactly one column name present once on each side.
Added, dropped, and differently named columns are not candidates.

The resolution rule is:

1. If `DiffOptions.key` is non-empty, validate and use it exactly as today.
2. Otherwise, evaluate every same-name single-column pair.
3. Discard a pair unless its types are compatible under `ComparisonPlan`.
4. Discard a pair if either input has zero rows, or if either side contains a
   null or `NaN`.
5. Discard a pair unless its canonical values are unique on both sides.
6. Compute the exact intersection size of its canonical old and new values and
   discard it when that size is zero.
7. Select the greatest intersection size, breaking ties by old-column order.

Uniqueness and overlap use values canonicalized for that specific old/new type
pair. Thus cross-type representations such as string and integer may form a
guessed key when the existing comparison rules make their values equal.
Unparseable strings remain tagged string values and participate normally;
missing values and `NaN` make the whole candidate ineligible.

The normalized overlap is descriptive evidence:

```text
overlap = shared_values / min(old_rows, new_rows)
```

It does not alter ranking because the denominator is the same for every
candidate. An eligible guessed key always has a non-zero denominator and a
ratio in `(0, 1]`.

An explicit declaration is the override mechanism for this non-interactive
step. A supplied key is never compared with guesses and never silently replaced
when it is invalid. Existing missing-column, incompatible-type, missing-value,
non-unique-old, and unsupported-fanout errors therefore retain their current
behavior for declared keys.

Explicitly deferred:

* paired old/new key components and rename-aware identities;
* recovery from an invalid declared key via an issue plus a guessed fallback;
* row-number fallback when no guessed key is eligible;
* guessed compound keys;
* accepting duplicate new-side values as bounded fanout;
* candidate lists, interactive confirmation, and an in-process rerun UI;
* sampling, budgets, approximate overlap, and partial key searches.

Until row-number fallback is planned, omission with no eligible candidate is a
fatal `NoEligibleKey` error whose message recommends `--key`. This makes the
temporary limitation explicit rather than inventing row identity.

# Key-resolution design

## Declared path

Keep syntax validation and compound-key assembly separate from guessing.
`validate_components()` continues to reject empty, paired, or repeated declared
components. Each declared component continues to require a column on both
sides, a compatible comparison plan, present values, and independent
uniqueness.

The resolved internal key should carry:

```rust
struct ResolvedKey {
    basis: KeyBasis,
    columns: Vec<KeyColumn>,
    old: Vec<Vec<CanonicalValue>>,
    new: Vec<Vec<CanonicalValue>>,
    overlap: Option<f64>,
}
```

Declared keys set `basis: Declared` and `overlap: None`. The public result
continues to contain every component coordinate in declaration order.

## Guessed candidates

Keep candidate evaluation in `key.rs`, next to canonicalization and validation,
rather than coupling it to `schema.rs` or row matching. Enumerate old schema
fields in position order, find the unique same-name field in the already
name-validated new schema, and skip incompatible pairs.

Candidate rejection is ordinary control flow, not a `DiffError`: a nullable,
duplicated, incompatible, or disjoint column simply is not a guess. Reuse small
validation and intersection helpers where their semantics match declared-key
validation, but retain declared errors with their current row and side context.

Use stable hashes only as bucket indexes. Confirm canonical equality inside a
bucket both when checking uniqueness and when counting cross-side matches.
Because eligible candidates are unique on each side, every matched canonical
value contributes exactly one to the intersection.

Store the canonical columns in the candidate and move the winning vectors into
`ResolvedKey`. Do not canonicalize the winner a second time. Candidate
comparison should use a tuple equivalent to:

```text
(Reverse(shared_values), old_column_position)
```

No `HashMap` or filesystem iteration order may influence enumeration,
selection, or output.

# Result and interface

Extend `KeyBasis` with `Guessed`. Extend `KeyDiff` with an optional numeric
`overlap` field that is serialized only when present:

```json
{
  "key": {
    "basis": "guessed",
    "columns": [1],
    "overlap": 0.6666666666666666
  }
}
```

The selected column uses the existing collapsed, one-based coordinate, so a
same-name column moved from old position 1 to new position 3 is `[1, 3]`.
Keeping the field absent for a declared key avoids changing every existing
declared-key snapshot. `PartialEq` remains sufficient for result assertions if
the new floating-point field prevents `Eq` derives on containing result types;
the ratio can never be `NaN` or infinite.

At the CLI boundary, `--key` keeps its comma-delimited syntax but is no longer
required. Help and README text should explain that omission guesses one
same-name column and that `--key` is the explicit override.

Human output announces a guessed key as the first line, using the existing
operation style and old-side column name:

```text
col_key("id")
```

`col_key` is informational context, not a change operation: it is emitted only
when the basis is guessed (a declared key just restates the user's own input),
it does not affect whether the diff counts as changed, and `no_changes()` still
follows it when no change operations exist. Overlap stays out of the human
line; JSON is the inspectable record of the chosen basis and evidence.

# Verification

Keep unit tests inline in `key.rs`. Use compact in-memory tables to cover:

* one eligible same-type column;
* compatible cross-type values;
* rejection for null, `NaN`, duplicates on either side, incompatible types,
  empty inputs, and zero shared values;
* preference for the largest exact intersection;
* an equal-intersection tie resolved by old-column order;
* multiple candidates whose hash buckets require equality confirmation;
* reuse of selected canonical values in the returned key;
* declared compound keys bypassing guessing;
* invalid declared keys retaining their existing errors; and
* repeated resolution returning the same candidate and overlap.

Add library integration coverage showing that:

* default options guess a key and correctly align reordered rows;
* `Diff.key` reports `guessed`, the collapsed column coordinate, and overlap;
* an explicit key overrides a stronger eligible guess and reports `declared`;
* moved key columns retain paired coordinates through schema reconciliation;
* a guessed key remains excluded from top-level changed cells;
* no eligible candidate returns `NoEligibleKey`;
* empty inputs still reconcile when a valid key is declared but cannot guess;
  and
* repeated complete comparisons serialize to byte-identical JSON.

Add one human-output snapshot showing `col_key` leading the operations for a
guessed key, one showing `col_key` followed by `no_changes()` for a guessed key
over identical tables, and confirm declared-key human snapshots are unchanged.

At the CLI boundary, update the help snapshot, add one successful invocation
without `--key`, and keep one explicit-key invocation proving the override
syntax. One compact JSON assertion is enough to cover the guessed basis and
overlap; do not duplicate the full eligibility matrix outside `key.rs`.

Update the README and demo guidance so the primary example exercises guessing
and an adjacent example demonstrates `--key` as an override. Retain an explicit
key in fixtures whose purpose depends on a particular compound key or on an
error from a declared key.

# Definition of done

This step is complete when:

* omitting a key deterministically selects the eligible same-name single column
  with the greatest exact shared-value count;
* every guessed key is compatible, present, unique on both sides, and supported
  by at least one shared canonical value;
* ties resolve by old-column order and repeated runs are byte-identical;
* guessed results expose their basis, selected coordinate, and normalized
  overlap, and human output leads with `col_key` for a guessed key;
* an explicit simple or compound key takes precedence and retains strict
  validation;
* lack of an eligible guess fails clearly without inventing a row identity;
* existing schema, row, cell, summary, and declared-key behavior remains
  covered;
* documentation describes both automatic guessing and explicit override; and
* the full test suite, strict Clippy, formatting, and diff checks pass.
