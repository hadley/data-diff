# Review of `data-diff-design.md`

This review focuses on decisions that a potential implementor would otherwise
have to make. Each note has a response field so that decisions can be recorded
in place.

## Likely MVP blockers

### 1. Cross-type equality is not an equivalence relation

Lines 94–98 say integers and doubles can compare equal, and strings are parsed
according to the other column's type. This can make equality non-transitive:

- `"1.0"` equals `double(1)`;
- `double(1)` equals `int64(1)`; but
- `"1.0"` may not equal `int64(1)` if the integer parser rejects decimal
  syntax.

Hash keys require a stable equivalence relation, and line 218 requires the same
equality semantics everywhere. We need either separate equality modes for key
identity, rename inference, and cell comparison, or a canonical common
representation that guarantees equal values hash identically and equality is
transitive.

**Response:**
I think we need to make sure that the integer parser ignores decimals?

**Resolution:**
Numeric comparisons use an exact comparison domain. Integer-like strings may
include a fractional part or exponent only when their exact value is integral;
they are never truncated or parsed through floating point. Comparison and
hashing use a type-pair-specific comparison plan with consistent
canonicalization, rather than requiring one global cross-type equivalence
relation.

### 2. Missing-value semantics are unspecified

The design rejects missing key values, but never defines equality for nulls in
ordinary cells or rename inference:

- Does null equal null?
- Are typed nulls equal across compatible types?
- Is floating-point `NaN` considered missing for key validation?
- Does `NaN` equal `NaN` in keys, given that it does for values?
- How do nulls contribute to approximate agreement and frequency estimates?

**Response:**
- Yes (because this is a matching problem)
- Yes
- Yes
- no longer relevant
- include them

**Resolution:**
Null equals null across compatible types and does not equal a present value.
All `NaN` values equal one another, but remain distinct from null. Both null
and `NaN` invalidate keys. Outside key validation they participate in
comparison, hashing, agreement, and frequency calculations as distinct value
categories.

### 3. The JSON contract is not concrete enough to implement

Lines 206–216 list required information, but leave the actual data model open:

- How are Arrow/Parquet source and normalized types encoded?
- What is a column identity?
- How are compound keys represented?
- What distinguishes an absent field from an unresolved result?
- How are schema changes, issues, and ambiguities tagged?
- What coordinates does a changed cell contain?
- Is output ordering normative?
- Is there a schema/version field?

A small example JSON document or JSON Schema would remove considerable
guesswork.

**Response:**
- Can you propose something?
- A changed cell should get a pair of tuples ((old_row, old_col), (new_row, new_col)), collapsing to (row, col) if new and old are the same
- No need to version or add a schema, this is just a lightweight representation we'll use for testing/experimentation. Overall, the JSON schema doesn't matter too much as it's purely internal.

**Resolution:**
Added an illustrative, intentionally unstable JSON representation. All
coordinates are one-based and use the same collapsing convention: an unchanged
old/new position is one integer, while a moved position is `[old, new]`; cells
likewise collapse from a pair of coordinates to `[row, column]`. The complete
column mapping is stored as `identities`; renames are derived from identities
whose schema names differ. Adds, drops, and edits remain explicit. Separate
add/drop lists avoid needing a sentinel coordinate such as zero.

### 4. “All original schema differences” conflicts with later rename resolution

Initially, a renamed column is observed as a drop plus an addition. Once
identity is resolved, does the structured diff retain:

- the original drop/add observations;
- only the semantic rename; or
- both, with one identified as raw and one as reconciled?

The requirement to preserve “all original schema differences” suggests both,
but no relationship between the two layers is defined.

**Response:**
Only the semantic rename.

**Resolution:**
Schema additions and removals are provisional until column identity is
resolved. A resolved rename replaces its drop/add observations rather than
being stored alongside them. The original and normalized schemas remain
available, but the structured operations contain only resolved semantic schema
differences.

### 5. Column-order change is not defined operationally

`col_order()` means the “relative order of existing columns changed,” but
“existing” could mean:

- same-named columns only;
- resolved column identities, including renames; or
- every column after accounting for additions and removals.

For example, is changing `[a, b]` to `[x, a, b]` an addition only, or also an
order change? The sequence being compared and the exact output need to be
specified.

**Response:**
You tell me, but I think this needs to be a mapping from the location of each column in old to its position in new. So your example would be (2, 3). If it was [a, b, c] to [c, a] the output would be (2, 0, 1) (using 0 to represent removed)

Hmmmm, but that would report a lot of changes if you insert one column at the beginning. Can we simplify this using a classic diff algorithm to describe as a minimal change set?

**Resolution:**
Compare the sequences of resolved column identities after removing additions
and drops. Columns outside a longest common subsequence are the minimum set
that must move. Because identities are unique, use a linearithmic
longest-increasing-subsequence implementation. Break ties by retaining the
lexicographically earliest sequence of old-column positions. Report moved
columns using their collapsed, one-based old/new coordinates.

### 6. Row-order change is not defined operationally

Lines 137–139 say to record how order changed, but not how:

- Does insertion of new rows count?
- Are dropped and fanout rows excluded?
- Is every matched row whose absolute position changed reported?
- Is order based on a longest common subsequence, so only relative movement is
  reported?

Absolute-position comparison would report widespread movement after a single
insertion, contrary to “relative order.”

**Response:**
Same as above

**Resolution:**
Apply the same LCS/LIS algorithm to one-to-one matched row identities in their
original input orders. Exclude added, dropped, and fanout rows. Break ties by
retaining the lexicographically earliest sequence of old-row positions and
report the minimum moved set using collapsed, one-based old/new coordinates.
Fanout ordering remains part of the fanout event.

### 7. “Ordered by key” lacks an ordering definition

Canonical ordering must cover compound and cross-type keys, decimals,
timestamps, strings, and possibly `NaN`. Arrow types may not share a total
ordering. Sorting is also unnecessary if matches can be deterministically
ordered by old row position or canonical key bytes. We should choose one
explicit rule.

**Response:**
So maybe we don't need to sort then? We just match?

**Resolution:**
Do not sort keys. Align one-to-one matches in original old-row order:
`old_matching` contains the old rows in that order and `new_matching` contains
their corresponding new rows in the same positions. Retain both original row
coordinates. This provides deterministic alignment without requiring a total
order over heterogeneous or compound keys.

### 8. Supported MVP Parquet types are unclear

Line 229 says to normalize supported scalar types, while line 100 says
unsupported values can be compared when source types are identical. Should the
MVP:

- reject files containing binary, list, struct, map, duration, interval, or
  large decimal columns;
- preserve but not compare those columns; or
- compare them using Arrow equality?

This needs a precise support matrix and failure behavior.

**Response:**
Lets reject for now, but handling them to future work.

**Resolution:**
The MVP supports booleans, integers whose values fit in `int64`, `float32` and
`float64`, UTF-8 and dictionary-encoded strings, and nulls within those types.
It rejects the entire comparison when either input contains decimal, binary,
nested, temporal, interval, or other unsupported columns, identifying the
column and source type. Broader type support is future work.

### 9. Decimal normalization is incomplete

“At most 18 significant digits” does not fully determine representability. The
implementation also needs rules for:

- positive and negative scales;
- precision versus the actual coefficient;
- decimal-to-integer comparison;
- decimal-to-double comparison;
- rescaling when neither direction avoids overflow; and
- invalid or out-of-declared-precision values.

**Response:**
Can you sketch out something reasonable, assuming that decimal values will be relatively rare, and I think typically are used to represent currency, so will be well under 18 significant digitrs

**Resolution:**
All decimal support is deferred. The MVP rejects decimal columns and the
initial normalized numeric domain covers only integers and floating-point
values. Exact decimal representation, cross-type comparison, overflow, and
string parsing will be designed together as a future extension.

### 10. Date/time normalization combines types with different semantics

Dates, local times, timezone-free timestamps, timezone-aware timestamps, and
durations are not mutually interchangeable. “Date-times that represent
instants” does not identify which Arrow types do so. Compatibility pairs and
conversions should be defined separately.

**Response:**
Move this to future work

**Resolution:**
Removed the combined `date-time` normalized type from the initial design. The
MVP rejects all temporal and interval columns. Future support will separately
model calendar dates, local times, timestamps without time zones, instants,
durations, and calendar intervals; only instants are converted to UTC.

### 11. The “standard parser” does not exist without further choices

String conversion needs exact accepted grammars for booleans, integers, floats,
decimal values, dates, times, timestamps, timezone offsets, `NaN`, and
infinity. Locale, whitespace, leading signs, exponent notation, and date
formats must be fixed for deterministic behavior across libraries.

**Response:**
Define this briefly using standard rust parsers. Assume ISO8601.

**Resolution:**
String parsing is locale-independent, case-sensitive, and does not trim
whitespace. Booleans and doubles use Rust's standard `FromStr` parsers.
Integers use `i64::from_str` plus an exact checked parser for decimal/exponent
syntax whose mathematical value is integral. Unsupported formatting produces
a mismatch, not an error. Future temporal parsing will use explicitly selected
ISO 8601 profiles.

### 12. Key validation across differently typed columns is underspecified

The MVP requires key columns to have the same names, but not necessarily the
same source or normalized types. Should an integer key match a string key? If
so, the equality/hash problem above becomes immediately relevant. If not, the
MVP should explicitly require identical or narrowly compatible key types.

**Response:**
Yes, I think it should, since it's possible (if uncommon) for the key to be represented using the incorrect type, and the user needs to correct.

**Resolution:**
Key columns may have different compatible types. Construct one comparison plan
per old/new key-column pair and check uniqueness after canonicalization on each
side. Unparseable strings remain tagged string values that cannot match a typed
value but do not invalidate a key by themselves. Missing values and `NaN`
invalidate keys; incompatible declared key pairs fail validation.

## Full reconciliation blockers

### 13. Declared key naming after a rename hint is unclear

If `old.customer_id` becomes `new.id`, does a declared key contain
`customer_id`, `id`, an old/new name pair, or a resolved column identity?
“All columns still exist on both sides” is not literally true after a rename.
The engine API needs side-specific key references or identity-aware resolution.

**Response:**
An old/new name pair. e.g. --key old1/new1,repeated,old2/new2

**Resolution:**
`--key` accepts comma-separated components. A bare name refers to that column
on both sides; `old_name/new_name` identifies differently named columns and
establishes their identity before validation. Components may not reuse a
column. The JSON stores resolved key identities with the standard collapsed
coordinates. The MVP supports only bare components; paired components arrive
with rename-hint support.

### 14. Declared-key failure behavior conflicts with requiring keys

A broken declared key silently falls through to guessing and then row numbers.
That could conceal a serious data-integrity failure. Decide whether this is:

- an error;
- an unresolved issue requiring confirmation; or
- an automatic fallback accompanied by a prominent warning.

**Response:**
Should be flagged for the user in the same way as a failing hint. The UI will provide a way to resolve

**Resolution:**
Full reconciliation records an `invalid_key` issue with all validation reasons,
then continues with a guessed key or row-number matching to provide an initial
display. The fallback basis is explicit and the issue remains visible until the
user supplies a replacement, which reruns downstream stages. The MVP instead
terminates because it has neither guessing nor a resolution UI.

### 15. Many-to-one row changes have no representation

Duplication in `new` is fanout, but duplication in `old` makes the key
“unreliable.” A reverse fanout or aggregation can be just as meaningful. Even
if unsupported, it should be an explicit unresolved condition rather than
simply proceeding to a guessed key.

**Response:**
This is deliberate

**Resolution:**
Fanout remains intentionally one-directional. A unique old row can be compared
unambiguously with multiple new rows. Multiple old rows mapping to one new row
could represent aggregation, deduplication, or arbitrary pairing, so no
reverse-fanout event is inferred. The declared key fails with reason
`non_unique_old` and follows the normal invalid-key fallback behavior.

### 16. The fanout threshold needs a precise formula

“Fewer than 10% of the distinct key values in `new` are duplicated” could mean:

- duplicated distinct values divided by all distinct values;
- extra rows divided by all rows; or
- duplicated values common to both sides divided by common distinct values.

The strict boundary at exactly 10% also needs definition.

**Response:**
What do you recommend?

**Resolution:**
The fanout rate is the number of distinct shared key values duplicated in
`new`, divided by the number of distinct key values shared by both sides. Each
affected key counts once regardless of its number of new rows. Define the rate
as zero when there are no shared keys and retain the declared key when the rate
is at most 10%. New-only duplicates are additions and do not contribute.

### 17. Duplicated new-only keys should not be classified as fanout

The row-matching bullets say “keys duplicated in `new` → `row_fanout()`.” If
that key does not exist in `old`, there is no old row to fan out from; those
should presumably be added rows. Classification needs precedence rules.

**Response:**
Correct

**Resolution:**
Added and dropped rows are atomic events whose cells are not emitted or
summarized. Added and dropped columns likewise remain schema events rather than
generating cells across matched rows. Cell changes only compare identified
columns in matched or fanout-related rows.

**Resolution:**
Classify side presence before new-side multiplicity. An old-only key is a drop;
a new-only key produces additions regardless of duplication; a shared key with
one new row is a one-to-one match; and a shared key with multiple new rows is a
fanout group.

### 18. Fanout value comparisons conflict with canonical-dataset wording

Line 135 says old rows are aligned to every new fanout row so their values can
be compared. Line 139 then says the one-to-one datasets are used for “all
subsequent steps.” Are fanout changed cells included in the complete cell diff
and row/column summary, or only shown inside the fanout event?

**Response:**
Only in the fanout event

**Resolution:**
Each fanout event contains its old row, all corresponding new rows, and changed
cell pairs from comparing every identified non-key column. These cells remain
nested in the event and are excluded from the top-level changed-cell set,
rename inference, and row/column edit summarization. Schema adds/drops are not
expanded into cells, and an event remains even when its non-key values are
unchanged.

### 19. Added/dropped row cells need an explicit policy

Presumably cells in added and removed rows are not “changed cells,” since the
enclosing row operation accounts for them. This should be stated; otherwise an
implementor might emit both row additions and per-cell changes.

**Response:**
Correct

### 20. Key columns can apparently have schema changes but not value changes

Value comparison excludes key columns. If corresponding keys compare equal
through normalization but have different physical values or representations,
such as `"001"` and integer `1`, only a type change may be reported. Clarify
whether transformations of key values are deliberately suppressed or
represented separately.

**Response:**
Should be included with other col_edit() changes

**Resolution:**
Key columns do not generate changed cells because unequal keys identify
different rows. They do generate `col_edit()` events for source-type or other
representation changes when canonical keys remain equal. These edits sit
outside the minimum-cover graph, and may coexist with a rename. The initial
implementation only needs to detect source-type changes.

### 21. Exact rename inference can have very weak evidence

Any two removed/added columns that are identical over matched rows become
rename candidates. With very few matched rows, or columns that are entirely
null or constant, this provides weak evidence. Approximate inference corrects
for chance, but exact inference does not. Consider minimum evidence or an
ambiguity issue for low-information matches.

**Response:**
Suggest something

**Resolution:**
Keep exact rename inference simple for now: a sole exact pair is inferred
without a minimum row-count, cardinality, or information-content threshold.
Reconsider this only if practical fixtures reveal false positives.

### 22. Approximate rename sampling lacks minimum evidence requirements

With one sampled matched row, an unrelated pair can achieve 100% agreement.
Define a minimum number of comparable observations, and say whether null/null
observations count.

**Response:**
Suggest something. null always equals null for matching.

**Resolution:**
Require at least 20 aligned one-to-one row pairs before approximate rename
inference. Every pair counts, including null/null as an agreement and
null/present as a disagreement. With fewer rows, retain add/drop and report
`approximate_rename_insufficient_rows`. Keep the minimum tunable and retain the
existing agreement thresholds.

### 23. The expected-agreement calculation is underspecified

The frequency domains must use the same normalized equality relation, raising
the transitivity issue again. It also needs rules for missing values and values
failing cross-type parsing. Are those unique mismatch categories, omitted
observations, or ordinary values?

**Response:**
See above

**Resolution:**
The shared comparison-plan and missing-value rules already define the value
categories and parse-failure behavior. Clarified only that both frequency
distributions for expected agreement use the same deterministic sample as
observed agreement.

### 24. Swap detection does not fit cleanly into the identity model

A swap expressed as `old.a -> new.b` and `old.b -> new.a` means same-named
columns no longer preserve identity. That affects schema changes, key identity,
changed-cell coordinates, and order reporting. A concrete identity graph or
bijective mapping model is needed.

**Response:**
Yes, include a bijective map

**Resolution:**
Column identity is a partial bijection between old and new column coordinates.
Hints reserve endpoints, same-name identities are provisional, rename inference
adds unmatched pairs, and swap detection atomically replaces two pairs.
Unmatched endpoints are drops/adds; renames derive from differing endpoint
names. Any operation that reuses an endpoint is contradictory or ambiguous.

### 25. “Heavily edited” is undefined

Swap detection says it examines heavily edited columns, but gives no threshold
for entering the candidate set. It also says cross-comparisons must exceed 90%,
without stating whether the same chance correction, sampling, or missing-value
rules apply.

**Response:**
suggest something

**Resolution:**
A same-name pair is heavily edited when direct agreement is below 50%. Test a
swap only with at least 20 aligned rows; require both cross-pairs to have
greater than 90% observed and 80% chance-corrected agreement, using the
approximate-rename comparison rules. Accept only mutually unique candidate
swaps. All thresholds remain tunable.

### 26. Hints lack a formal contradiction model

Cases requiring explicit decisions include:

- both `col_drop(a)` and `col_rename(a, b)`;
- rename cycles or multiple old columns targeting one new column;
- `col_edit(a)` where `a` was renamed;
- add/drop hints for columns that also form an exact rename; and
- a hint that is valid structurally but disagrees with observed values.

“Contradictory” should be defined, ideally with stable issue codes.

**Response:**
Yes. Maybe change "Rename hints" to "Initial hint processing" where we reject contradictory hints, then apply the rename rules.

**Resolution:**
Initial hint processing normalizes all hints, rejects missing targets, and
detects endpoint conflicts across the full set before mutating identity.
Conflicting connected groups are rejected together so input order never picks
a winner; independent hints still apply. Renames precede keys, add/drop hints
reserve inference endpoints, and edit hints are validated after changes are
known. Issues use stable kinds for missing, contradictory, unchanged, and
unresolved hints.

### 27. `col_edit()` conflates schema and value events

The vocabulary says it means “values (or type) changed,” while the output also
preserves original schema type changes. That can produce two representations of
the same type change. Decide whether `col_edit` is a semantic summary that
references schema/value evidence, or whether schema differences and edit events
are independent layers.

**Response:**
What are the consequences of the two options?

**Resolution:**
`col_edit()` is one semantic column event with independent `type_changed` and
`values_changed` aspects. A type-only change produces no changed cells and is
displayed only in the schema section. Type changes force the column into the
summary; any genuinely unequal incident cells set `values_changed` and are
covered before optimizing the remaining graph. Schema and cell data remain
the underlying evidence, not duplicate semantic operations.

### 28. Minimum vertex-cover tie-breaking is not an algorithm yet

A standard maximum matching produces one minimum cover, not necessarily the
preferred one. “Prefer columns over rows, then original column order or
canonical row order” could mean:

- maximize the number of selected columns;
- lexicographically prefer earlier columns;
- prefer an all-column cover if one exists; or
- choose columns at each local ambiguity.

These give different answers. Define the optimization tuple explicitly.

**Response:**
Again, I don't understand the consequneces.

**Resolution:**
No semantic tie preference is needed. Any exact minimum cover is acceptable.
The implementation still uses stable traversal and iteration so repeated runs
choose the same cover, but tests generally assert minimum size and complete
edge coverage rather than a particular answer in tied cases.

### 29. Hints appear allowed to violate minimum summary size

Forced `col_edit()` events are selected before solving the remaining graph.
That is sensible, but the final summary may no longer be globally minimal. The
invariant should say “minimum subject to forced hints,” rather than simply
“minimal.”

**Response:**
Yes, that's right.

**Resolution:**
Coalesce columns forced by type changes or valid edit hints, remove all incident
changed-cell edges, and compute an exact minimum cover of the remainder. The
final summary is minimum subject to those forced events, not necessarily a
global minimum of the original graph, and must still cover every changed cell.

### 30. Budget exhaustion needs stage-specific result semantics

A partial diff after rename inference times out is materially different from
one after only summary construction times out. The result should say:

- which stage stopped;
- what was and was not attempted;
- whether downstream results are absent or provisional; and
- whether rerunning with user input can resume or must recompute.

**Response:**
Yes

**Resolution:**
Clarified that source-type and value changes are independent. A double-to-int
conversion whose normalized values remain equal produces a type-only
`col_edit()` and no changed cells.

**Resolution:**
Budgeted stages return the best valid partial result rather than suppressing
downstream work. Rename pairs are processed in endpoint groups and accepted
only when all candidates incident to both endpoints were examined. Minimum
cover always retains a valid cover and marks it `optimal: false` if exact
search stops. Incomplete stages identify unresolved work, and dependent
downstream results carry `incomplete_input`.

## Smaller mistakes and inconsistencies

### 31. There are five normalized types, not four

Line 96 says there are “only four possible target types,” and line 100 refers
to “these four normalized types,” but the table lists boolean, int64, double,
string, and date-time. If “four” means the four non-string parsing targets, say
that.

**Response:**
Update to five

**Resolution:**
Superseded by the later decision to defer temporal values. The initial design
now has four normalized types and three non-string parsing targets, so the
current counts are correct.

### 32. The type-change sentence is garbled

Line 72 says “we also don't want to display changes in type, but not type.”
The likely intended claim is that a physical type change should be shown
without also reporting unchanged logical values as cell edits.

**Response:**
Yes

### 33. `data-dict.yaml` appears without definition

The filename, discovery location, syntax, precedence relative to CLI keys, and
whether it belongs in the MVP are unspecified.

**Response:**
lets drop it for now. Move reading https://data-dict.tidyverse.org to future work.

**Resolution:**
Removed `data-dict.yaml` from declared-key resolution. Future work may
investigate data-dict metadata, with discovery, supported fields, validation,
and precedence designed when the integration is implemented.

### 34. Normalized overlap `r` is introduced late

Line 244 requires reporting `r`, but its definition only appears in the future
compound-key section. Move the definition into ordinary key guessing and define
its behavior for empty tables.

**Response:**
Good idea

**Resolution:**
Defined normalized overlap in ordinary key guessing as
`m / min(n_old, n_new)`, where `m` is the number of shared key values. It is
reported but does not alter candidate ranking. If either table is empty,
overlap is `null` and no guessed key is eligible. The HyUCC extension now
refers to this definition.

### 35. Empty-table behavior needs explicit cases

Questions include:

- Is an empty key column unique?
- Can a key be guessed with no shared values? Currently no.
- How are all rows classified when one side is empty?
- Is column order meaningful with zero rows?
- How are exact renames handled?

**Response:**
If old is empty, generate col_add() and row_add(). If new is empty, use col_drop() and row_drop(). But I don't think that speical case is particualrly interesting, we just need some default

**Resolution:**
Distinguish zero rows from zero columns and always compare schemas normally.
With an empty old/new side, all rows on the other side are additions/drops;
both empty means no row or cell changes. Empty-side key uniqueness is
vacuously true, no key can be guessed, value-based renames are skipped, and
column ordering may still be derived from schema identities.

### 36. Duplicate column names are not addressed

Arrow schemas can potentially contain repeated field names, while the CLI
identifies keys by name. Either reject duplicate names with a clear error or
define name-plus-occurrence addressing.

**Response:**
Reject it

**Resolution:**
Reject duplicate top-level column names before normalization. Equality is exact
and case-sensitive without Unicode normalization. Report every duplicate with
its side and one-based positions. This is a fatal input error because names
address keys, hints, schema identities, and results.

### 37. CLI parsing is ambiguous for unusual names

Comma-separated `--keys id,date` cannot represent a column name containing a
comma and may interact poorly with whitespace or empty names. Repeated flags
such as `--key id --key date` would avoid escaping rules.

**Response:**
yes, but that's a lot more typing and key names containing commas will be very rare.

**Resolution:**
Keep the concise comma-separated `--key`. The initial CLI does not escape `,`
or `/`; unusual names containing them require the structured engine or future
UI. Repeated flags can be reconsidered if this becomes a practical problem.

### 38. Error behavior is absent

The implementor needs exit codes and stderr/stdout rules for unreadable files,
invalid Parquet, invalid keys, unsupported types, and serialization failures.
In particular, invalid JSON must never be mixed with diagnostics on stdout.

**Response:**
Again, the this is just an initial sketch to work through reconcliation. These semantics don't matter yet.

**Resolution:**
Process-level exit codes, stdout/stderr conventions, and diagnostic formatting
are outside this reconciliation design. The experimental CLI may use
conventional behavior, but tests focus on engine results until the CLI becomes
a supported interface.

### 39. Coordinate-only output limits standalone inspectability

This is a defensible choice, but normalized schemas and key identities may
still require names, not only numeric positions. Also decide whether
coordinates alone remain valid when consumers no longer have access to the
exact original files.

**Response:**
Correct, again this is just for internal work.

**Resolution:**
Coordinate-only applies to operations, not schema metadata: the result retains
names and types in its old/new schemas. It is interpreted alongside the
original inputs during experimentation; standalone archival portability is not
a goal.

### 40. Determinism needs a stability boundary

“Repeated runs” might mean within one process, one binary version, or across
platforms and releases. Rust's default hashers are not suitable if cross-run
hash stability affects sampling or output. Specify a stable hash algorithm or
limit the determinism guarantee.

**Response:**
Yes, use stable hash.

**Resolution:**
Use XXH3-128 with seed 0 and a versioned, explicit canonical byte encoding.
Hashes are stable across runs and platforms. They only form candidate buckets;
the comparison plan always verifies equality, so collisions cannot affect
correctness. Deterministic samples use the smallest hashes with old-row
position as the tie-break.

## Suggested design additions

Before implementation, it would be useful to add four short normative
sections:

1. A comparison matrix defining nulls, cross-type equality, hashing, parsing,
   and supported Arrow types.
2. A concrete structured-diff JSON example with versioning, coordinates,
   identities, issues, and unresolved states.
3. Exact definitions and examples for row and column reorderings.
4. An MVP behavior table covering invalid keys, empty tables, unsupported
   types, duplicate names, and CLI errors.

**Response:**
1. Yes
2. sketch me something
3. sketch me something
4. sketch me something
