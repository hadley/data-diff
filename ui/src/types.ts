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
  edited_columns: number;
  edited_rows: number;
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
}

export interface FanoutGroup {
  old_row: number;
  new_rows: number[];
  lines: RowLine[];
}

export interface EditedGroup {
  rows: number[];
  key: ValueDto[];
  changed: boolean[];
  lines: RowLine[];
}

export interface RowViewData {
  columns: string[];
  rows: Page<RowLine> | null;
  groups: Page<FanoutGroup> | null;
  edited: Page<EditedGroup> | null;
}

export type ViewKind = "column" | "row" | "cell";
