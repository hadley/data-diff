import { describe, expect, it } from "vitest";
import { openingView } from "./opening";
import type { SessionSummary } from "./types";

function summary(overrides: Partial<SessionSummary>): SessionSummary {
  return {
    old_path: "old.parquet",
    new_path: "new.parquet",
    cells: 0,
    optimal: true,
    edited_columns: 0,
    edited_rows: 0,
    cover_columns: 0,
    cover_rows: 0,
    added_rows: 0,
    dropped_rows: 0,
    moved_rows: 0,
    fanout_groups: 0,
    key_columns: ["id"],
    schema: [],
    ...overrides,
  };
}

describe("openingView", () => {
  it("opens the column view when the cover is column-dominated", () => {
    expect(
      openingView(summary({ cover_columns: 3, cover_rows: 1, cells: 100 })),
    ).toBe("column");
  });

  it("opens the row view when the cover is row-dominated", () => {
    expect(
      openingView(summary({ cover_columns: 1, cover_rows: 5, cells: 100 })),
    ).toBe("row");
  });

  it("opens the cell view for a diffuse cover", () => {
    // Four events covering six cells is nearly one event per cell.
    expect(
      openingView(summary({ cover_columns: 2, cover_rows: 2, cells: 6 })),
    ).toBe("cell");
  });

  it("opens the cell view when the cover is not optimal", () => {
    expect(
      openingView(
        summary({ optimal: false, cover_columns: 10, cover_rows: 0, cells: 1000 }),
      ),
    ).toBe("cell");
  });

  it("opens the cell view when nothing changed", () => {
    expect(openingView(summary({}))).toBe("cell");
  });
});
