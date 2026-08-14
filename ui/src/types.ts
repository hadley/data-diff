/** The file a value comes from; the toolbar's old/new toggle picks one. */
export type Side = "old" | "new";

export interface ValueDto {
  kind: string;
  text: string;
}

export interface SchemaRow {
  status: "identity" | "added" | "dropped";
  key: boolean;
  old_pos: number | null;
  old_name: string | null;
  new_pos: number | null;
  new_name: string | null;
  moved: boolean;
  basis: string | null;
  type_change: [string, string] | null;
  source_type: string | null;
}

export interface SessionSummary {
  old_path: string;
  new_path: string;
  cells: number;
  optimal: boolean;
  cover_columns: number;
  cover_rows: number;
  added_rows: number;
  dropped_rows: number;
  moved_rows: number;
  fanout_groups: number;
  key_columns: string[];
  schema: SchemaRow[];
}

export interface Page<T> {
  items: T[];
  total: number;
  page: number;
  page_size: number;
}

export interface CellRow {
  key: ValueDto[];
  column: string;
  old: ValueDto;
  new: ValueDto;
  delta: ValueDto | null;
}

export interface ColumnHeader {
  name: string;
  span: "pair" | "single";
  side: "old" | "new" | null;
  origin: "edited" | "context" | "added" | "dropped";
}

export interface ColumnCell {
  old: ValueDto | null;
  new: ValueDto | null;
  changed: boolean;
}

export interface ColumnRow {
  key: ValueDto[];
  cells: ColumnCell[];
}

export interface ColumnViewData {
  columns: ColumnHeader[];
  rows: Page<ColumnRow>;
}

export interface RowLine {
  label: string;
  key: ValueDto[];
  values: ValueDto[];
  changed: boolean[];
  /** The hidden side's values on a single-side edited line; null otherwise. */
  alt: ValueDto[] | null;
}

export interface FanoutGroup {
  old_row: number;
  new_rows: number[];
  lines: RowLine[];
}

/** One "rows edited" sub-entry in the sidebar: its changed columns and row count. */
export interface EditedGroupSummary {
  columns: string[];
  rows: number;
}

/** One "columns" sub-entry in the sidebar: the group's columns and shared row count. */
export interface ColumnGroupSummary {
  columns: string[];
  rows: number;
}

export interface RowViewData {
  columns: string[];
  rows: Page<RowLine> | null;
  groups: Page<FanoutGroup> | null;
}

/** The sidebar's selection: which component the main panel shows. */
export type Selection =
  | { view: "schema" }
  | { view: "columns"; group: number | null }
  | { view: "edited"; group: number | null }
  | { view: "added" }
  | { view: "dropped" }
  | { view: "moved" }
  | { view: "fanout" }
  | { view: "cells" };
