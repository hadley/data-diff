---
title: data-diff implementation plan
---

# Todo

Development now proceeds at a slower, review-first pace. Treat each new plan as
a separate PR-sized change: branch, implement it, present it for careful review, and
leave it uncommitted. The project owner will decide when to commit after review.
Do not begin the next plan until that review is complete.

- [ ] **Establish compact summary test infrastructure.** Scaffold an internal
  summary module with small graph and cover types. Add test-only edge-list
  construction, cover-validity assertions, and a brute-force optimum oracle.
  Test those helpers directly so this commit remains green before the production
  algorithm exists.
- [ ] **Compute an exact minimum bipartite vertex cover.** Find a maximum
  matching with stable Hopcroft–Karp traversal and recover a cover with the
  standard alternating-path construction. Use the compact fixtures for focused
  shapes, then exhaustively compare every graph up to 3 × 3 with the brute-force
  oracle. Do not add budgets, a new dependency, or an approximate fallback.
- [ ] **Define the edit-summary result model.** Add a separate `summary` to
  `Diff`, containing `optimal`, selected column edits, and selected row edits.
  Reuse the existing collapsed coordinates and column-edit aspects. Preserve
  `columns.edited` and `cells` as complete evidence rather than changing their
  current meaning.
- [ ] **Prepare forced column edits and the residual graph.** Force every
  source-type edit into the summary, mark it as value-edited when it has an
  incident changed cell, and remove its incident cells from optimization.
  Convert the remaining cells into deterministic dense row and column vertices
  without losing their original old/new coordinates.
- [ ] **Integrate summarization into reconciliation.** Run it after complete
  cell comparison, emit selected edits in original old-side order, and verify
  that every changed cell is covered by a forced or selected event. Empty cell
  sets, type-only changes, moved identities, and added or dropped rows and
  columns must retain their existing behavior.
- [ ] **Expose the summary at output boundaries.** Serialize the new summary in
  deterministic JSON. Update human output to use summary `col_edit` and
  `row_edit` operations instead of the redundant evidence-level value edits and
  individual `cell_edit` operations; JSON continues to retain all underlying
  cells and evidence-level column edits.
- [ ] **Complete the acceptance pass.** Add focused library and CLI coverage,
  confirm byte-identical repeated output, update the README and demo, run tests,
  strict Clippy, formatting, and diff checks, and manually inspect one
  row-dominant and one column-dominant summary.

# Goal

Turn the complete changed-cell evidence into the smallest exact set of semantic
`row_edit()` and `col_edit()` events. A user should see one column edit when many
rows changed in one column, one row edit when many columns changed in one row,
and a minimum combination for irregular patterns.

The summary is an additional interpretation of the diff:

```text
complete changed cells
    → forced type-edited columns
    → residual bipartite graph
    → exact minimum vertex cover
    → row/column edit summary
```

It must not discard or rewrite evidence. The complete schemas, identities,
evidence-level column edits, row matches, and changed cells remain available in
the structured result.

# Scope

This step implements exact summarization only:

* One graph vertex represents one affected matched-row identity.
* One graph vertex represents one affected identified-column identity.
* One edge represents one changed cell.
* Selecting a vertex emits one row or column edit.
* Every edge must be incident to at least one selected vertex.
* The number of non-forced selected vertices must be minimal.

Columns with source-type changes are forced into the edit set before
optimization, as required by `design.md`. A changed cell incident to a forced
column sets that summary event's `values_changed` aspect and is removed from the
residual graph. The resulting cover is minimum subject to those forced choices;
it need not be a global minimum of the original graph.

There are no edit hints yet, so source-type changes are the only forced events.
Added and dropped rows or columns do not produce changed cells and therefore do
not enter the graph. Key columns can contribute forced type-only edits, but key
cells remain excluded.

Do not add computation budgets, timeouts, partial results, or approximate
covers. The summary always reports `optimal: true`. Bounded fallback is a later
step that may report `optimal: false`.

# Result model

Add a summary alongside the existing evidence:

```rust
pub struct EditSummary {
    pub optimal: bool,
    pub columns: Vec<ColumnEdit>,
    pub rows: Vec<Coordinate>,
}
```

`Diff.summary.columns` contains forced type edits and value-edited columns
selected by the cover. `Diff.summary.rows` contains rows selected by the cover.
A forced type-edited column coalesces its independent aspects:

* `type_changed: true, values_changed: false` for a type-only edit;
* `type_changed: true, values_changed: true` when it also covers changed cells;
* `type_changed: false, values_changed: true` for a value-only column selected
  by the minimum cover.

`Diff.columns.edited` keeps its current evidence-level meaning: it includes
every identified column with a type change or at least one changed cell.
`Diff.cells` remains the complete changed-cell set. Consumers can therefore
inspect or render the evidence even when the summary chooses `row_edit()`.

Row and column edits use the same collapsed one-based coordinates as their
underlying identities. Emit summary columns in old-column order and rows in
old-row order, regardless of graph traversal order.

The JSON shape is:

```json
{
  "summary": {
    "optimal": true,
    "columns": [
      {
        "column": 2,
        "type_changed": false,
        "values_changed": true
      }
    ],
    "rows": [[3, 1]]
  }
}
```

This is a fragment; the existing evidence fields remain present.

# Exact-cover algorithm

## Graph construction

Build the residual graph from changed cells not already covered by forced
columns. Rows are the left partition and columns are the right partition.
Assign dense internal vertex IDs in old-side coordinate order, and keep
adjacency lists in old-column order. Deduplicate defensively even though the
cell comparer should produce at most one edge for each row/column identity.

Keep output coordinates outside the graph algorithm. The graph should operate
on small integer IDs so its unit tests do not need Arrow tables, schemas, or
`Diff` construction.

## Maximum matching

Use Hopcroft–Karp to compute an exact maximum-cardinality matching:

1. Breadth-first search layers all augmenting paths from unmatched row vertices.
2. Depth-first search augments along those layers.
3. Repeat until no augmenting path remains.

Visit row vertices and each adjacency list in ascending stable order. This does
not establish a semantic preference among tied minimum covers, but it makes the
chosen result repeatable.

## Cover recovery

Recover a minimum cover using König's theorem. Starting from unmatched left
vertices, traverse alternating paths:

* left to right only across unmatched edges;
* right to left only across matched edges.

If the reachable sets are `Z_left` and `Z_right`, the minimum cover is:

```text
(left - Z_left) ∪ (right ∩ Z_right)
```

Return both the cover and maximum-matching cardinality internally. Assert in
debug builds and tests that the cover has the same size as the matching and
that every residual edge is covered.

# Test strategy

Keep algorithm tests independent of Arrow and express graphs as short edge
lists. Cover these shapes directly:

* no vertices and no edges;
* one changed cell;
* many rows incident to one column, selecting the column;
* many columns incident to one row, selecting the row;
* disconnected components requiring a mixture of rows and columns;
* a complete rectangle with tied minimum covers;
* isolated vertices, which must never be selected; and
* repeated runs of a tied graph, which must return the same cover.

For every graph with at most three rows and three columns, enumerate all edge
subsets. Compute the true optimum by enumerating all possible vertex subsets,
then assert that the production result:

1. covers every edge;
2. has the optimum cardinality; and
3. is byte-for-byte stable across repeated calls.

Use small in-memory Arrow tables for summarization integration tests:

* a column-dominant edit selects one `col_edit`;
* a row-dominant edit selects one `row_edit`;
* an irregular edit selects a mixed minimum cover;
* a type-only column is forced without manufacturing a cell;
* a type-and-value column is one coalesced forced edit;
* a forced column removes its incident edges before optimization;
* moved rows and columns retain paired coordinates;
* unchanged and empty inputs produce an empty optimal summary; and
* additions and drops do not enter the summary.

At the output boundary, use one compact JSON assertion and one human-output
snapshot. Avoid duplicating the graph truth table in CLI tests.

# Human output

The human format should present the semantic summary rather than both the
summary and its redundant cell evidence. For example, three changed cells in
one column become:

```text
col_edit("price", values)
```

and three changed cells in one row become:

```text
row_edit(2)
```

A moved selected row uses its collapsed identity:

```text
row_edit(3 -> 1)
```

Schema, addition, drop, and ordering operations retain their current forms.
Individual `cell_edit` lines are omitted from human output once the summary is
available; the JSON `cells` field remains the complete drill-down evidence.
`no_changes()` still represents a diff with no structural, ordering, type, or
value operations.

# Definition of done

This step is complete when:

* every checklist item is checked and committed;
* every complete small graph passes brute-force optimality comparison;
* every changed cell is covered by at least one summary event;
* forced type edits are coalesced and excluded from residual optimization;
* summary events use original one-based old/new coordinates;
* JSON retains the complete evidence and adds an optimal summary;
* human output uses the minimum semantic edit summary;
* repeated comparisons produce byte-identical output; and
* the full test, Clippy, formatting, and diff checks pass.

# Next steps

Add later reconciliation features in dependency order, giving each isolated
fixtures, integration coverage, and determinism checks:

1. Guess eligible single-column keys and allow users to override the guess.
2. Add paired key components and validated rename/add/drop/edit hints.
3. Infer exact renames from aligned matched rows.
4. Support bounded declared-key fanout while keeping fanout cells separate.
5. Add approximate rename inference and then swap detection, initially
   examining all matched rows.
6. Benchmark the complete pipeline and introduce deterministic sampling,
   computation budgets, valid partial results, and incomplete-stage reporting.
   This is also when edit summarization gains a bounded valid-cover fallback and
   may emit `optimal: false`.
7. Expand scalar type support, then design a bounded large-data execution model
   and interactive UI.

Defer decisions about hint syntax, UI presentation, thresholds, and concrete
budgets until the prerequisite behavior exists and can be benchmarked. Preserve
the central invariants: deterministic reconciliation, no inferred event without
underlying evidence, and continued access to the complete cell-level diff.
