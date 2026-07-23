---
title: data-diff
---

# Introduction

This document lays out the design of a data-diff tool for tabular files, initially focusing on Parquet files. The motivating problem is that Positron has no way to display a diff of Parquet files, but seeing how an AI has changed your data is an important part of validating that it has done what you expect. We ultimately want to create a rich, interactive experience for comparing an `old` dataset with a `new` dataset.

A few principles:

* `data-diff` is a visual tool for humans: there's no goal to make patches or machine-readable output part of the final product. This gives us considerable freedom in the UI, including asking the user to resolve ambiguities and choose the display that they find most useful.

* We require row identifiers (keys) to produce the most semantically meaningful diffs. If no key is supplied at the command line, we guess and allow the user to override interactively.

* We display schema differences (e.g. column addition/removal or type change), then reorderings, then value differences. This orients the user at a high level before they dive into the details.

* We accept optional (possibly LLM-generated) hints to resolve ambiguity and circular problems, particularly when both rows and columns change.

* Computation is predictably bounded. Work in the common case should be linear in the size of the input, and any superlinear or heuristic search must operate within fixed limits. We prefer to request help from the user rather than spending unbounded time resolving an ambiguity.

The rest of this document is divided into three parts. First, we define a small vocabulary for describing semantic changes to rows and columns. Then we describe reconciliation: how we normalize values, establish column identity, match rows, and reduce the resulting cell-level changes to a concise set of row and column events. We finish up with some notes on the implementation.

# Vocabulary

We'll begin by establishing some vocabulary for the semantic changes we want to represent. Note that rows and columns are not exactly symmetrical. You can add, remove, and edit both rows and columns, but you can't rename a row or fan out a column.

| Operation | Meaning |
|---|---|
| `col_add([new1, new2, ...])` | Column added |
| `col_drop([old1, old2, ...])` | Column removed |
| `col_edit([old1, old2, ...])` | Values (or type) changed, preserving identity |
| `col_rename([old1, old2], [new1, new2])` | Column renamed, preserving identity |
| `col_order()` | Minimum set of identified columns that must move to explain a change in relative order |
| `row_add()` | Rows added |
| `row_drop()` | Rows removed |
| `row_edit()` | Existing rows' (non-key) values changed |
| `row_fanout()` | One old row corresponds to multiple new rows with the same key |
| `row_order()` | Minimum set of one-to-one matched rows that must move to explain a change in relative order |

This vocabulary allows a single physical change to be described by multiple possible semantic changes. Here are a few examples of such ambiguities:

* `row_edit()` vs `col_edit()`: You can represent a rectangular edit with either `row_edit()` or `col_edit()`. By default, we pick the most parsimonious description: does the change affect fewer rows or fewer columns?
* `col_drop(a)` + `col_add(b)` vs `col_rename(a, b)`: If column `a` is dropped, column `b` is added, and their values are identical, we can assume that `a` was renamed to `b`.
* `col_drop(a)` + `col_add(b)` vs `col_rename(a, b)` + `col_edit(b)`: If there's no perfect match between an old/new column pair, the change might be a deletion plus a genuinely new column, or a rename followed by modification. Because the latter is relatively rare, we prefer the former unless fewer than 10% of the values differ.
* `col_edit([a, b])` vs `col_rename([a, b], [b, a])`: If both `a` and `b` are heavily edited, it's possible that they were swapped.
* `row_add()` vs `row_fanout()`: If a key that identifies one row in `old` identifies multiple rows in `new`, the change is more usefully represented as a fanout than as a collection of additions. A common cause of this behaviour is a join gone wrong.

We resolve these ambiguities during reconciliation, described next, optionally accepting column hints (`col_add()`, `col_drop()`, `col_edit()`, or `col_rename()`) provided either on the command line or through the UI. Hints never manufacture a change. We ignore and report any hint whose target is missing, contradictory, or unchanged.

# Reconciliation

Reconciliation takes two datasets and produces a candidate set of semantic changes suitable for display to the user. This is reasonably straightforward when only rows or only columns have changed: a stable set of columns lets us match rows, and a stable set of rows lets us match columns. When both have changed, however, the two problems become interdependent. For example, imagine that we've renamed a key (initially observed as `col_drop()` and `col_add()`) and reordered the rows. We need the key to align the rows, but we need aligned rows to recognize the renamed key.

We can reduce this problem by encouraging the user to commit changes in smaller steps, but we can't rely on best practices. So instead of attempting to untangle the diff with a sufficiently expensive algorithm, we require the user to provide some hints. They work as follows:

* `col_rename()` is applied before resolving keys, so we can match rows even if key column names have changed.
* `col_add()` and `col_drop()` remove columns from rename inference, both improving performance and preventing false matches.
* `col_edit()` prevents us from interpreting the column as part of a swap and simplifies how we display value changes.

With those preliminaries in place, we can outline the full reconciliation process:

1. Normalize schemas.
2. Apply rename hints.
3. Resolve row keys.
4. Match rows, including duplicate-key groups.
5. Create aligned datasets for comparison.
6. Detect column renaming.
7. Determine column reordering.
8. Determine value changes.

## Schema normalization

We first compare the two schemas, recording column additions, removals, reorderings, and type changes. Schema additions and removals recorded at this stage are provisional. If later reconciliation establishes an identity between a removed and added column, the structured diff replaces those provisional operations with the semantic rename. It does not retain redundant drop/add operations. The original schemas remain available if a consumer needs to reconstruct the initial syntactic comparison.

We preserve type differences for display: normalization does not erase a type change just because values can be compared across it. But we also don't want to display changes in type, but not type, e.g. when integers in a double-typed column are converted to integers in an integer-typed column.

For value comparison, we normalize the source types to a smaller set of flexible types:

| Normalized type | Source types |
|---|---|
| `boolean` | Booleans |
| `int64` | Integers that fit in a signed 64-bit integer, and fixed-precision numbers with at most 18 significant digits |
| `double` | Floating-point numbers and other real numbers that can be represented as doubles |
| `string` | Strings, factors, categoricals, dictionaries, and enums, using their logical values rather than their underlying codes |
| `date-time` | Dates, times, and date-times |

Fixed-precision values are represented by an `int64` coefficient and a scale. When comparing two such columns, we rescale them to a common scale if we can do so without overflow. This allows values such as `1.0` and `1.00` to compare equal without passing through floating point.

Missing values compare equal to missing values, including when the columns have different but compatible types. A missing value does not equal any present value. Floating-point `NaN` is distinct from a missing value: all `NaN` values compare equal to one another, but do not compare equal to null.

Both null and floating-point `NaN` are considered missing for key validation and therefore invalidate a declared or guessed key. Outside key validation, null and `NaN` participate in value comparisons, hashing, agreement proportions, and value-frequency calculations as two distinct value categories.

Columns with the same normalized type are compared as follows:

* `boolean` values are compared exactly.
* `int64` values are compared exactly after resolving any fixed-precision scale.
* `double` values are compared exactly, and positive and negative zero compare equal.
* `string` values are compared byte-for-byte. We do not silently trim, fold case, or apply Unicode normalization.
* `date-time` values are converted to a common resolution. Date-times that represent instants are converted to UTC; changes to source units and time zones remain visible as schema differences.

Numeric comparisons use an exact comparison domain. Integers and fixed-precision decimals are represented by an integer coefficient and a decimal scale. A floating-point value compares equal to an integer or decimal only when it represents exactly the same mathematical value. This avoids introducing matches through rounding.

`string` is compatible with every other normalized type. When comparing a string column with a numeric column, we parse the strings into the numeric comparison domain. Integer-like strings may contain a fractional part or exponent provided their exact mathematical value is integral: for example, `"1"`, `"1.0"`, and `"1e0"` compare equal to integer `1`, while `"1.5"` does not. Parsing must not truncate fractional values or pass exact integers through floating point. Numeric canonicalization removes insignificant decimal zeros, so `1`, `1.0`, and `1.00` have the same canonical representation.

For other normalized types, we parse strings using the standard parser for the other column's type and compare the parsed values. A value that cannot be parsed is a mismatch. We don't format the typed value as a string, because formatting choices should not determine equality. Parsed representations are cached, and there are only four possible non-string target types, so this adds only linear work per string column.

This rule allows a column to retain its identity through a transformation such as parsing a character date or number. We still report the string-to-typed transition as a type change.

Source types that cannot be represented by these four normalized types, such as binary or nested values, are compared only when their source types are identical. They are not candidates for inferred cross-type renames; the user can supply a rename hint if needed.

## Rename hints

We next apply any rename hints, provided that both the old and new columns exist and neither has already been assigned to a different rename. Otherwise, we ignore the hint and add it to a list of issues that we surface to the user. A valid hint establishes column identity for key resolution and all subsequent comparisons; it does not assert that the column's type or values are unchanged.

## Key resolution

Next we look for a key that provides a stable row identifier. We proceed in three steps:

1. **Declared key** — If the user supplies a key set, either directly or through `data-dict.yaml`, we use it provided that all of its columns still exist on both sides. We validate uniqueness in both `old` and `new` before trusting it:

   | Unique in `old` | Unique in `new` | Resolution |
   |---|---|---|
   | yes | yes | use the key as declared |
   | yes | no | `new` has fanned out relative to `old` |
   | no | --- | key is unreliable |

   For the fanout case, we retain the key if fewer than 10% of the distinct key values in `new` are duplicated, treating them as isolated `row_fanout()` groups. Otherwise, we treat the key as broken and continue to the next step.

2. **Guessed key** — If no declared key was provided or survives validation, we search for one. For each compatible column with the same identity on both sides, we compute uniqueness and the overlap between its sets of non-missing values. A candidate must contain no missing values, be unique in both `old` and `new`, and have at least one value in common. We do not infer fanout from a guessed key. We select the candidate with the largest number of shared values, breaking ties by column order because we assume key columns are more likely to occur early in the data.

3. **Row number** — If we can't find a candidate key, we use row number. This means we can't distinguish a `row_edit()` from `row_drop()` + `row_add()`, and we display a reordering as many edits. But it allows the rest of the process to continue, and will generate an initial display that the user can refine.

If we reach step 2 or 3, we expose the selected matching basis in the UI so that the user can override it. An override reruns the remainder of the reconciliation process.

## Row matching

With a key in hand, we hash each row's key value on both sides:

* Keys present only in `old` → `row_drop()`.
* Keys present only in `new` → `row_add()`.
* Keys duplicated in `new` → `row_fanout()`.
* Keys present in both → matched rows, carried forward for cell comparison.

Uniqueness is required for one-to-one row matching, but not for grouping. If the key is unique in `old` but a key value occurs multiple times in `new`, all of the new rows belong to a `row_fanout()` group for that value. We align the old row with each new row in the group so that we can compare their values. These one-to-many alignments are kept separate from the one-to-one matches used to infer column renames.

## Aligned matched rows

We create `old_matching` and `new_matching` from the one-to-one common rows. Matched pairs are ordered by their original position in `old`: `old_matching` contains each old row in that order, and `new_matching` contains its corresponding new row in the same position. The two tables are therefore aligned for column hashing and cell comparison without requiring a total ordering over key values. Each entry retains both rows' original positions for output coordinates. Added, dropped, and fanout rows remain outside these tables.

Before aligning the matched rows, we compare their identities in the original old and new input orders. Added, dropped, and fanout rows are excluded. We find a longest common subsequence (LCS) of these identities. Rows in the LCS retained their relative order; rows outside it are the minimum set of rows that must move to explain the reordering. Because matched-row identities are unique, we use the same linearithmic longest-increasing-subsequence algorithm as for column ordering.

We break ties by retaining the LCS whose sequence of original old-row positions is lexicographically earliest. The structured diff records each moved row using its collapsed old/new coordinate. An empty list means that relative order did not change.

For example, if old rows `[a, b, c]` become `[x, c, a, b]`, `x` is handled as an addition, the LCS retains `[a, b]`, and `c` is the sole moved row. Fanout rows are excluded because a one-to-many relationship does not have a single position on the new side; their ordering remains part of the fanout event.

## Rename inference

Next we resolve column identity by interpreting addition/removal or edit pairs as renames. Rename inference uses only the aligned, one-to-one matched rows; fanout groups are excluded. We compare only columns with compatible types. If there are no matched rows, we cannot infer renames from values, so we skip this step and leave the columns as additions and removals.

There are four steps for rename inference:

1. Apply hints.
2. Look for exact renames.
3. Look for approximate renames.
4. Look for swaps.

We first generate candidate lists of adds, drops, and edits, then apply any hints by removing those columns from the lists. (Plausible rename hints have already been applied before key matching). If a hint is not applicable --- for example, because the column does not exist or is unchanged --- we ignore it and report that to the user.

We first look for exact renames. We hash each remaining removed and added column over the matched rows, then compare columns with equal hashes to verify that their values are identical. If an old column and a new column match only each other, they become a `col_rename()`. If multiple pairings are possible because columns have identical hashes/values, we match them in column order and allow the user to override the result in the UI.

Next we look for approximate renames among the remaining unmatched removed and added columns. We expect rename-and-modify to be relatively rare, so this is a small, bounded search. We impose fixed limits on both the number of candidate pairs and the number of matched rows examined. If there are too many candidate pairs, we skip approximate inference and ask the user to identify any renames. If there are too many matched rows, we take a deterministic sample based on the key so that repeated runs produce the same result.

For each compatible pair in the sample, let $p_o$ be the observed proportion of equal values. Raw agreement is less informative for low-cardinality columns, where unrelated columns may often agree by chance, so we also calculate the expected agreement from the two columns' value frequencies:

$$
p_e = \sum_v p_{old}(v) p_{new}(v)
$$

We then calculate chance-corrected agreement:

$$
\kappa = \frac{p_o - p_e}{1 - p_e}
$$

A pair is an approximate-rename candidate if $p_o > 0.9$ and $\kappa > 0.8$. If $p_e = 1$, $\kappa$ is undefined, and the pair is not a candidate. These initial thresholds are deliberately conservative and can be tuned with experience.

We accept a candidate only when it is the sole candidate for both the old and new columns. If candidates overlap --- for example, if one old column plausibly matches two new columns ---  we leave resolution up to the user. We deliberately avoid more complex assignment algorithms: ambiguity here is unusual, and user input is more valuable than a sophisticated guess.

Finally, we check whether pairs of heavily edited, same-named columns might actually have been swapped. For columns `a` and `b`, we compare `old.a` with `new.b` and `old.b` with `new.a`. If both cross-column comparisons have greater than 90% agreement and there is only one possible swap, we replace the two `col_edit()` interpretations with `col_rename([a, b], [b, a])`. As above, we ask the user to resolve competing interpretations.

## Column ordering

Once column identities have been resolved, we compare the relative order of the identified columns. We remove dropped columns from the old sequence and added columns from the new sequence, then replace every remaining column with its resolved identity. Renaming a column without moving it therefore does not count as a reordering, and inserting or removing a column does not by itself make the surrounding columns appear to move.

We find a longest common subsequence (LCS) of the two identity sequences. Columns in the LCS retained their relative order; columns outside it are the minimum set of columns that must move to explain the reordering. Because column identities are unique, this can be implemented as a longest-increasing-subsequence problem in linearithmic time rather than with a general quadratic LCS algorithm.

There may be multiple longest common subsequences. We deterministically retain the one whose sequence of original old-column positions is lexicographically earliest. This tends to treat an earlier part of the old schema as stable and describe later columns as moving around it.

For example, inserting `x` to transform `[a, b]` into `[x, a, b]` is only a column addition: after removing `x`, the two identity sequences are identical. Transforming `[a, b, c]` into `[c, a, b]` retains the subsequence `[a, b]` and reports `c` as the single moved column. Transforming `[a, b, c]` into `[c, a]` first removes the dropped column `b`, then compares `[a, c]` with `[c, a]`; the tie-break retains `a` and reports `c` as moved.

The structured diff records each moved column using its collapsed old/new coordinate. An empty list means that relative order did not change.

## Value changes

Now that we have row keys and consistent column identities, we compare non-key cell values in `old` and `new`. This produces a set of changed cells, `[(row1, col1), (row2, col2), ...]`, scattered across rows and columns. We retain the complete cell-level change set for display and later summarization.

A valid `col_edit()` hint forces a column to be represented as a column event if it contains at least one changed cell. We first select the hinted columns and remove their incident cells from the change set. We then summarize the remaining cells normally. This preserves all observed changes while preventing a hinted column edit from being reinterpreted as a collection of row edits. A type-only edit has no incident changed cells to remove and is already represented by the schema comparison. We ignore and report a `col_edit()` hint for an absent column or one with neither value nor type changes.

From the cell-level change set, we need to decide whether to report `row_edit()`, `col_edit()`, or both, reducing it to the minimal set of row and column events that accounts for every changed cell.

We model this as a bipartite graph with one vertex per affected row, one vertex per affected column, and an edge for every changed cell. Choosing rows to mark `row_edit()` and columns to mark `col_edit()` so that every changed cell is covered by at least one marked row or column is then a **minimum vertex-cover problem on a bipartite graph**. Unlike vertex cover on a general graph, which is NP-hard, the bipartite case can be solved exactly in polynomial time. König's theorem tells us that the size of the minimum vertex cover equals the size of the maximum matching, and the cover can be recovered from the matching by the standard alternating-path construction.

This objective depends only on the number of events, not on the proportion of values changed within each row or column. For example, if three changed cells all belong to one column, we report one `col_edit()` rather than three `row_edit()` events, even if most values in that column are unchanged.

There can be multiple covers with the same number of events. We resolve these deterministically, preferring columns over rows, then original column order or aligned row order. These preferences are only tie-breakers and must never increase the number of events.

Maximum matching is superlinear in the worst case, so we apply it only within fixed budgets for vertices, edges, and elapsed work. If the changed-cell graph exceeds any budget, we retain the complete cell-level diff but do not guess a row/column summary. Instead, we ask the user whether to summarize primarily by rows or by columns. The concrete budgets are implementation parameters that should be chosen through benchmarking.

# Implementation

The first implementation will be written in Rust, with the reconciliation engine implemented as a library and a small command-line binary used to exercise it. The engine takes two typed tables plus optional keys and column hints, and returns a structured diff. Eventually, the UI will render that diff, report any issues, and rerun reconciliation when the user changes a key or hint. Keeping this boundary narrow will make the reconciliation logic easy to test without involving the UI.

The goal of the first implementation pass is to work out the reconciliation process, not to build the UI. It should therefore write the structured diff as JSON, including any issues or unresolved ambiguities. This gives us an inspectable output for developing fixtures and refining the algorithms while postponing presentation decisions until the underlying model is stable.

The initial command-line interface should be:

```
data-diff old.parquet new.parquet --keys id,date
```

`--keys` takes a comma-separated list of columns, allowing the user to supply either a single-column or compound key. We still need to decide how hints will be supplied. That decision is not required for the MVP, which does not support hints; later, the engine should accept them independently of whatever command-line or UI syntax we choose.

The structured diff should preserve information rather than prematurely reducing it. At a minimum, it needs to contain:

* the original and normalized schemas, plus the resolved semantic schema differences;
* the resolved column identities and row key;
* added, removed, matched, and fanout rows;
* row- and column-order changes;
* the complete set of changed cells;
* the chosen row/column summary; and
* ignored hints, unresolved ambiguities, and exhausted computation budgets.

The JSON output only needs to identify operations and their coordinates; it does not need to include old or new cell values. Coordinates refer to one-based row and column positions in the original inputs. We use the same collapsing convention for every old/new relationship:

* A one-sided coordinate is an integer.
* An old/new position is a single integer when the positions are the same, and `[old, new]` otherwise.
* A cell is `[row, column]` when both positions are the same, and `[[old_row, old_column], [new_row, new_column]]` otherwise.

Additions and removals are stored separately, so they do not require a sentinel coordinate. Arrays are emitted in deterministic input/coordinate order. An empty array means that a stage ran and found no corresponding changes; `null` means that the stage was not run or did not produce a resolved result, with the reason recorded as an issue.

The complete resolved mapping between old and new columns is stored as `identities`. A rename is derived when the names at the two ends of an identity differ, so it is not also stored as a separate operation. Additions and removals must be stored explicitly because they have no identity on the other side. Edits are also stored explicitly because identity alone does not imply that a column's type or values changed.

For example, an experimental result might look like:

```json
{
  "schemas": {
    "old": [
      {"name": "id", "source_type": "INT64", "normalized_type": "int64"},
      {"name": "label", "source_type": "UTF8", "normalized_type": "string"},
      {"name": "value", "source_type": "DOUBLE", "normalized_type": "double"}
    ],
    "new": [
      {"name": "id", "source_type": "INT64", "normalized_type": "int64"},
      {"name": "amount", "source_type": "DOUBLE", "normalized_type": "double"},
      {"name": "note", "source_type": "UTF8", "normalized_type": "string"}
    ]
  },
  "columns": {
    "identities": [1, [3, 2]],
    "added": [3],
    "dropped": [2],
    "edited": [[3, 2]]
  },
  "key": [1],
  "rows": {
    "added": [4],
    "dropped": [2],
    "matched": [1, [3, 2]],
    "fanout": []
  },
  "order": {
    "columns": null,
    "rows": null
  },
  "cells": [
    [[3, 3], [2, 2]]
  ],
  "summary": null,
  "issues": []
}
```

Here old column 3 and new column 2 have the same identity; because their schema names differ, this represents a rename from `value` to `amount`. Their values also changed, so the same coordinate pair occurs in `edited`. The example is illustrative rather than a stable public contract: the representation can evolve as we use it for testing and experimentation.

Coordinate-only output avoids defining JSON encodings for values such as large integers, decimals, `NaN`, infinities, dates, timestamps, binary data, or nested values.

Each comparison should use a comparison plan derived from the two column types. Hashing and equality within that plan must use the same canonicalization, so values that compare equal always have equal hashes. Key matching, rename inference, and cell comparison should all use this shared comparison layer. Operations should be deterministic: samples, ambiguous exact matches, and tie-breaks must depend only on the input data and its original ordering. Expensive stages should accept explicit budgets and return an unresolved result when a budget is exhausted, rather than silently falling back to a weaker heuristic.

## MVP

The MVP should exercise the complete path from two Parquet files to a JSON description of their differences while avoiding inference. It should require the user to supply a key whose columns have the same names in both datasets, and it should reject keys that are missing, contain missing values, or are not unique on either side. It does not need to support fanout, guessed keys, column hints, rename inference, or an interactive UI.

The MVP can assume that both datasets are small enough to fit comfortably in memory. Its purpose is to work out the reconciliation model and produce correct results, so it does not need computation budgets, sampling, streaming, or other safeguards for large inputs.

The MVP supports:

* booleans;
* signed and unsigned integers, provided every value fits in `int64`;
* `float32` and `float64`, normalized to `double`;
* UTF-8 strings, including dictionary-encoded strings after decoding their logical values;
* decimals with at most 18 significant digits; and
* nulls within any supported typed column.

The MVP rejects binary and fixed-size binary values; lists, structs, maps, and other nested values; dates, times, timestamps, durations, and intervals; decimals exceeding the supported precision; and any other Arrow or Parquet logical type not listed above. If either input contains an unsupported column, the MVP rejects the entire comparison and identifies the column and its source type. It does not silently omit the column or return a partial diff. Support for these types can be added later.

For this restricted case, the engine should:

1. Read the two files and compare their schemas.
2. Normalize the supported scalar types and implement the comparison rules above.
3. Validate the supplied key and use it to identify added, removed, and matched rows.
4. Record row and column reorderings.
5. Compare same-named, non-key columns over matched rows and retain the changed cells.
6. Serialize schema changes, row additions and removals, ordering changes, and cell-level value changes as coordinate-only JSON.

Initially, the JSON can list affected rows, columns, and cells without trying to find the optimal `row_edit()`/`col_edit()` summary. This makes the reconciliation result easy to inspect while establishing the data structures needed by every later stage.

It's also important to build a solid testing system so that it's easy to generate variable inputs and compactly assert that the results are correct. It's very important that the tests be easy to read and understand in isolation.

## Implementation order

Once the end-to-end MVP works, additional reconciliation features should be added in dependency order:

1. **Value-change summarization.** Add minimum bipartite vertex cover and deterministic tie-breaking. This improves the presentation without changing row or column identity.
2. **Key guessing.** Find eligible single-column keys, rank them by the number of matched rows, and report $r$ as a normalized overlap summary. Let the user override the result, rerunning row matching and all downstream stages after an override.
3. **Rename hints.** Add `col_rename()` before key resolution, then `col_add()`, `col_drop()`, and `col_edit()` at their respective reconciliation stages. Surface ignored or contradictory hints in the UI.
4. **Exact rename inference.** Use the aligned matched rows to detect identical removed/added columns. This depends on stable row matching but requires no heuristic thresholds.
5. **Declared-key fanout.** Permit limited duplication in `new` for a declared key, keeping fanout groups separate from the one-to-one rows used for rename inference.
6. **Approximate rename inference.** Add chance-corrected agreement and resolution of ambiguous mappings. At this stage, it can examine all candidate pairs and matched rows.
7. **Swap detection.** Compare heavily edited same-named columns for the relatively rare case where two columns have exchanged identities.
8. **Computation budgets.** Benchmark the completed reconciliation stages, then add explicit limits on candidate pairs, sampled rows, graph vertices and edges, memory, and elapsed work. When a limit is reached, return the partial diff together with an unresolved issue instead of silently changing algorithms or implying that no match exists.

Each step should be introduced with small fixtures that isolate the new behaviour, plus end-to-end fixtures that combine it with all earlier stages. Particularly important invariants are that reconciliation is deterministic, hints never override the data, every changed cell remains available for display, and every inferred row or column event can be traced back to the underlying schema or cell-level diff.

# Future extensions

## Compound key guessing with HyUCC

The initial key search considers only individual columns, but many datasets require a combination of columns to identify a row. [HyUCC](https://www.btw2017.informatik.uni-stuttgart.de/slidesandpapers/F3-13-21-short/paper_web.pdf) is a promising basis for extending the search to compound keys. It discovers unique column combinations by alternating between evidence gathered from rows and validation over combinations of columns, allowing each strategy to prune the other's search space.

We would need to adapt HyUCC to our two-table setting. A candidate combination must contain no missing values and be unique in both `old` and `new`; satisfying either table alone is not sufficient. For every eligible combination, we would form its key tuples and let $m$ be the number shared by the two tables. We would select the candidate with the largest $m$, breaking ties first in favour of fewer columns and then by column order.

As with single-column keys, we would report normalized overlap as

$$
r = \frac{m}{\min(n_o, n_n)},
$$

where $n_o$ and $n_n$ are the row counts of `old` and `new`. Because the denominator is constant across candidates, $r$ summarizes the match but does not affect candidate selection. The inferred compound key and its overlap must be visible and overrideable by the user, and we should not infer fanout from it.

Unique-column-combination discovery has an exponential worst-case search space, particularly for wide tables. Any HyUCC-based search must therefore have explicit limits on candidate width, number of candidates, memory, and elapsed work. If it exhausts a budget, it should report that the search was incomplete rather than imply that no compound key exists.

## Tolerant double comparison

Exact comparison can produce noisy diffs when floating-point values differ only because of rounding error. In the future, we should allow the user to supply non-negative relative and absolute tolerances, $t_{rel}$ and $t_{abs}$. Two finite values would compare equal when

$$
|x - y| \le \max\left(t_{abs},\ t_{rel}\max(|x|, |y|)\right).
$$

The relative tolerance scales with the magnitude of the values, reflecting how floating-point precision behaves, while the absolute tolerance handles values near zero. Using the larger magnitude makes the comparison symmetric. Both tolerances would default to zero, preserving exact comparison unless the user opts in.

Tolerance should apply only when reporting cell-level value changes. Key matching, hashing, and rename inference should remain exact because approximate equality is not transitive and therefore cannot safely define row or column identity. The UI should display the active tolerances so that the user can tell why small numerical changes have been omitted.
