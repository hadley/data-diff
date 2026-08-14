---
title: data-diff UI design
---

# Introduction

This document sketches the interactive UI for data-diff. It assumes the reconciliation design in [design.md](design.md) and maps every view onto the existing `Diff` model; nothing here requires new reconciliation machinery. The one new library surface is a lazy, paginated lookup of cell and row values from the input tables, described at the end.

The layout is master-detail: a narrow sidebar on the left lists every component of the diff — schema, columns, rows edited (expanding to one sub-entry per group of rows sharing a changed-column set, the CLI's granularity), rows added, dropped, moved, fanout, and cells — each with its count, empty categories hidden. Selecting an entry switches the main panel on the right to that component's view. A common toolbar at the top of the main panel holds the active view's toggles, and the schema view is always the opening view.

Everything is lazily loaded. The model retains the complete cell set by invariant, but no view holds more than a window of it; old and new values are fetched from the input tables on demand, a page at a time, as the user scrolls. Each table view fills the main panel and scrolls in both directions, with the key columns pinned at the left edge and the column names pinned at the top. The schema view is the exception: it does not fill its space with a table, leaving room for a summary of the changes beside the alignment.

# Schema view

The opening view, and the hint surface. A two-sided alignment of old and new schemas, one row per column identity or unmatched column, ordered by new-file position with drops interleaved at their old position:

```
SCHEMA                                       [changed only | all columns]
──────────────────────────────────────────────────────────────────────────
key   old                      new
──────────────────────────────────────────────────────────────────────────
 🔑  1 id            ⇄        id          int64
     2 price         ⇄        price       int64 → double
     3 name          ⇄      4 label       string         rename (exact)
     4 qty           ✕                      int64          dropped
                          3   sku         string         added
```

* Key columns are marked in a dedicated first column (🔑), one mark per key component
* The old position always shows. The new position shows only when it differs from the old — an unmoved identity needs no second number, and a moved column's new position says everything a move badge would.
* Renames show both names with a basis badge (`exact`, `approximate`, `hinted`, `declared`, `swapped`). The badge matters because some bases are certainties and some are judgements — the same rationale as the human format printing `basis:`.
* Type changes render inline as `int64 → double`. A type-only edit (no changed cells) lives here and nowhere else; the value views are about values.
* The toggle fills in unchanged identities as plain rows, giving the full aligned schema. Changed-only is the default.
* This panel is where hints happen: split a rename into drop + add, join an add/drop pair into a rename, assert `col_edit()`.

# Value views

The value views show changes by column, by row, and by cell, selected from the sidebar. They are renderings of one underlying set — `Diff::cells` plus the row events — and each event is shown in exactly one place.

In every table the key columns are frozen at the left edge and the header row at the top: a cell's row identity and column name stay visible no matter how far the table scrolls. The flat tables (columns, each row section, cells) are windowed — only the visible rows are rendered, and scrolling fetches more — so the table always fills the panel and there is no pager.

## Column view

One table of the cover's column edits, with changed/all toggles in the toolbar — one per axis, so the columns shown and the rows shown each switch independently.

```
VALUE CHANGES BY COLUMN      [changed cols | all cols] [+ added/dropped]
                             [changed rows | all rows]
───────────────────────────────────────────────────────────────────────
      |  price                sku               discount
key   |  old      new         old      new      old      new
───────────────────────────────────────────────────────────────────────
1042  |  9.99     12.99       A-100    A-100X   0.00     0.00
1047  |  14.50    16.00       A-205    A-215    0.10     0.15
…                          (scrolls; more rows load as you go)
```

* The column name is a grouped header spanning its two sub-columns `old` and `new`. The whole EDITED section is one table: every edited column side by side, keyed rows aligned, so a row that changed in two columns shows both edits on one line.
* Highlighting marks the cells that actually differ.
* The rows toggle fills in unchanged rows for context, page by page. The columns toggle fills in unchanged columns, which join as single-span columns (no old/new split — there is nothing to compare). Both default to changed-only.
* A separate control adds the added and dropped columns, so their values are visible too. They join as single-span columns in schema order: an added column shows its values under `new` with the `old` side blank, a dropped column the reverse. They are not part of either changed/all toggle because they are not identities — there is no old/new pair to split — and because their values are context rather than changes: an added column's cells are not in `Diff::cells`, exactly as an added row's are not.

## Row views

The transpose of the column view: same grid, with the two-level structure on the rows instead of the columns. Each row category is its own sidebar entry — rows edited, added, dropped, moved, fanout — and "rows edited" expands to one sub-entry per group of rows sharing a changed-column set, so a rectangle is navigable directly. Selecting a sub-entry shows that group's rows with the columns narrowed to the group's changed set; the toolbar's columns toggle fills in the rest, defaulting to changed-only.

```
ROWS EDITED (41)               [changed columns | all columns]
  a, b (38)
  c (3)
ROWS ADDED (12)
ROWS DROPPED (8)
ROWS MOVED (3)
FANOUT (1)
```

* In the EDITED table, changed cells render as stacked rows

```
                                     [changed columns | all columns]
───────────────────────────────────────────────────────────────────────
 key |         price      sku         discount
───────────────────────────────────────────────────────────────────────
1042 | old     9.99       A-100       0.00
1042 | new     12.99      A-100X      0.00
```

* ADDED and DROPPED are tables of the rows' values from the one side that has them. Their cells are deliberately not in `Diff::cells`, so these tables read the input rows directly through the same lazy lookup.
* MOVED is the exception to the pattern: its table is `key | old position | new position`, with no values and no toggle.
* FANOUT lists its groups, each a small aligned table — the old row on top, each new row below, changed cells highlighted:

```
1055   1 row → 3 rows
      row      price    sku      discount
      old      4.50     B-010    0.00
      new 1    4.50     B-010    0.10
      new 2    4.50     B-010    0.20
      new 3    2.25     B-010    0.00
```

The fanout's cells live in `FanoutEvent.cells`, outside the top-level cell set, so this view is the one place they are visible. That is correct: a one-to-many comparison is not evidence of an ordinary edit.

## Cell view

The flat evidence table — the fallback view and the drill-down target of the other two:

```
ALL CHANGED CELLS               87 cells              [by key | by column]
──────────────────────────────────────────────────────────────────────────────
key   |  column      old          new
───────────────────────────────────────────────────────────────────────────────
1042  |  price       9.99         12.99
1042  |  sku         A-100        A-100X
1047  |  price       14.50        16.00
1047  |  sku         A-205        A-215
…                          (scrolls; more cells load as you go)
```

* Columns are `key | column | old | new`. Compound keys widen into one column per component.
* Column names come from the identity map, so a changed cell in a renamed column displays under its new name — the design's display rule, since the reader will find that name in the new data.
* Old and new values render in their own source types, so a type-changed column shows `"9.99" → 9.99` honestly rather than normalized into sameness. Null renders distinctly from `NaN` and from empty string, since the comparison semantics treat all three differently.
* Deliberately absent: added- and dropped-row cells (row events, shown in the row view), fanout cells (shown only in the fanout expansion), and key-column cells (unequal keys are different rows, not edits). The cell view is exactly `Diff::cells` — no more, no less. That fidelity is what makes it the evidence layer: every count elsewhere in the UI is a filter of this table.
* Sorting by key or column, from the toolbar. Column and row filters, with drill-down from the other views pre-setting them, are a future addition.

# Lazy lookup API

The consumers — the cell view, the row views, and the column view's table — all need values the `Diff` deliberately does not retain: `cells` holds only `CellCoordinate`s, and added/dropped rows are only positions. One lookup primitive serves them all: given a side and a set of row/column coordinates, return the values, page-sized. The `u32` coordinate ceiling already reflects the model's memory-consciousness, so values are looked up against the Parquet inputs rather than retained in `Diff`. The server keeps this page-sized contract; the windowed frontend simply asks for the pages the scroll position touches.
