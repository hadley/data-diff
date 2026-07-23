# Review of `plan.md`

This review reads the plan against `design.md` and focuses on decisions that an
implementor would otherwise have to make while working through the MVP
checklist. Each note has a response field so that decisions can be recorded in
place.

## Likely MVP blockers

### 1. Duplicate keys in `new` have no explicit MVP outcome

The behavior table says "Key non-unique after canonicalization → Fail key
validation," but the design treats new-side duplication differently from
old-side duplication: a key that is unique in `old` and duplicated in `new` is
*valid* fanout when the affected-key rate is at most 10%. Fanout is deferred to
post-MVP step 5, so the MVP presumably fails on *any* new-side duplication,
including cases the full design would accept. This should be stated directly:
does the behavior-table row cover both sides, and does the error distinguish
`non_unique_old` from unsupported fanout so the message doesn't wrongly imply
the key is broken?

**Response:**

### 2. `columns.edited` has no defined rule without summarization

In the design, `values_changed` is set during edit summarization: columns enter
the edit set by type change, by hint, or by minimum vertex cover, and incident
cells then mark `values_changed`. The MVP defers summarization (post-MVP step
1) and hints (step 3), which under a strict reading leaves only type-changed
columns in `edited`. But the example JSON shows
`{"column": 2, "type_changed": false, "values_changed": true}` — a value-only
entry that summarization would have produced. The MVP rule needs to be
explicit. The most plausible reading is that MVP `edited` is an evidence-level
rollup: every identified column with a source-type change or at least one
changed cell gets an entry. If so, say that, and note how post-MVP step 1
replaces or augments it with the minimum-cover summary. Otherwise the example
is wrong.

**Response:**

### 3. There is no `rows.edited`, and the asymmetry is unexplained

Related to the previous note: the example emits per-column edit entries but no
per-row equivalent, even though `row_edit()` is in the design vocabulary. That
is defensible — rows have no type changes, so row edits only arise from
summarization — but an implementor reading the plan alone will wonder whether
`rows.edited` was forgotten. State that row edits first appear with post-MVP
step 1.

**Response:**

### 4. Same-name columns with incompatible types have no defined outcome

MVP identity is by name, so `old.flag: boolean` and `new.flag: int64` receive
an identity, but the comparison matrix marks `boolean ↔ numeric` incompatible.
Neither document says what happens for a *non-key* identified pair with
incompatible types. Options include: fail the whole comparison (consistent with
the MVP's reject-early posture); keep the identity, record `type_changed`, and
treat every non-null cell pair as changed; or refuse the identity and report
drop plus add. Each gives a materially different result. The same question
applies to a supported type paired with itself under the "source types outside
these categories" rule once type support expands.

**Response:**

### 5. Column coordinates in `edited` and `key` are only shown for the trivial case

The example uses bare integers (`"column": 2`, `"columns": [1]`), but a
column's old and new positions can differ in the MVP whenever a column is
inserted or removed before it. Presumably these fields use the same collapsed
convention as `identities` — one integer when positions agree, `[old, new]`
otherwise — but the plan never says so, and the example cannot show it. Confirm
the rule and add a coordinate-shape unit test for the disagreeing case.

**Response:**

### 6. `rows.matched` collapsed pairs versus `order.rows` needs one sentence

If `matched` follows the collapsing rules, a matched row whose position changed
appears as `[old, new]` — while `order.rows` separately reports the
LCS-minimal moved set. These are genuinely different (inserting one row at the
top shifts every matched pair's positions but moves nothing relatively), but
the plan never states that `matched` carries the full position mapping while
`order.rows` is the minimal relative-move subset, and the example shows only
bare integers in both. Spell out the relationship and the emission order of
`matched` (old-position order, presumably).

**Response:**

### 7. The library's table type is undefined

"Accepts two typed tables" leaves open the concrete representation: a single
Arrow `RecordBatch`, a `Vec<RecordBatch>`, or an Arrow `Table`-like chunked
structure. Parquet files with multiple row groups naturally load as multiple
batches, so the choice affects the loader (concatenate first?), row-coordinate
mapping, and every test builder. Deciding this before the scaffold item avoids
reworking the test vocabulary.

**Response:**

## Post-MVP sequencing notes

### 8. Approximate rename inference (step 6) precedes sampling (step 7)

The design describes approximate inference and expected agreement in terms of
"the same deterministic sample," but sampling infrastructure arrives one step
later. Presumably step 6 initially runs over all matched rows and the "sample"
is the full set until step 7 introduces the key-based sample. Worth one
sentence so step 6's tests aren't written against machinery that doesn't exist
yet.

**Response:**

### 9. Step 1 summarization without budgets needs a stance on `optimal`

The design's minimum-cover section includes bounded optimization and an
`optimal: false` marker, but computation budgets arrive in step 7. Decide
whether step 1 implements exact cover only (always optimal, no flag needed
yet) or includes the initial pick-the-smaller-side fallback and the `optimal`
field from the start. The former is simpler and consistent with "do not add
placeholder fields for post-MVP stages."

**Response:**

## Smaller mistakes and inconsistencies

### 10. Key-column type edits are missing from the checklist and behavior table

The design requires an identified key column to produce a type-only
`col_edit()` when its source type changes even though key cells are excluded
from comparison, and says the initial implementation needs source-type
detection. The plan's "Compare cells" item mentions only non-key columns, and
the behavior table has no row for it. Since the MVP explicitly supports
cross-type key components, this case is reachable on day one; add it to the
checklist item and the behavior table.

**Response:**

### 11. Out-of-range integer detection needs a stated timing

Failing on a `uint64` value above `i64::MAX` requires scanning values, not
just the schema. The plan places it under "Load and validate inputs," which
implies an eager full scan of unsigned 64-bit columns at load time — fine, but
say so, and decide whether the error includes the offending row position or
only side, column, and source type as the behavior table currently states.

**Response:**

### 12. Dictionary columns with non-string values are unaddressed

The plan supports "dictionary-encoded strings." A Parquet/Arrow dictionary
column whose value type is numeric is neither clearly supported (the design's
`string` normalization covers dictionaries "using their logical values") nor
clearly rejected. Make rejection (or logical-type normalization) explicit in
the loader item so the unsupported-type test list is complete.

**Response:**

### 13. The scaffold item depends on the result model that follows it

Checklist item 1 asks for "structured diff assertions" before item 2 defines
the result model. Presumably the scaffold ships a placeholder result and the
real assertions arrive with item 2; a parenthetical would prevent an
implementor from designing assertion helpers twice.

**Response:**

## Suggested plan additions

Two small additions would remove most of the remaining guesswork:

1. A second JSON example exercising the non-trivial coordinate shapes — a
   moved column, a moved row, a pair-form `[[old_row, old_col],
   [new_row, new_col]]` cell, and a key or edited column whose positions
   differ — since the current example collapses everything to bare integers.
2. Behavior-table rows for the cases raised above: new-side duplicate keys,
   key-column type-only edits, and same-name columns with incompatible types.

**Response:**
