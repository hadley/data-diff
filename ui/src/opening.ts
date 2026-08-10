import type { SessionSummary, ViewKind } from "./types";

/**
 * The opening view, picked from the edit summary per ui-design.md: a cover
 * dominated by column edits opens the column view, one dominated by row
 * edits the row view, and a diffuse cover — large relative to the cell
 * count, or not optimal — the cell view, the evidence layer.
 */
export function openingView(summary: SessionSummary): ViewKind {
  if (!summary.optimal) return "cell";
  const events = summary.cover_columns + summary.cover_rows;
  if (summary.cells > 0 && events * 2 >= summary.cells) return "cell";
  if (summary.cover_columns > summary.cover_rows) return "column";
  if (summary.cover_rows > 0) return "row";
  if (summary.cover_columns > 0) return "column";
  return "cell";
}
