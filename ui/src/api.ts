import type {
  CellRow,
  ColumnViewData,
  Page,
  RowViewData,
  SchemaRow,
  SessionSummary,
} from "./types";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, init);
  const body = await response.json();
  if (!response.ok) throw new Error(body.error ?? response.statusText);
  return body as T;
}

function query(params: Record<string, string | number | boolean | null>): string {
  const entries = Object.entries(params).filter(([, value]) => value != null && value !== "");
  return entries.length
    ? "?" + entries.map(([key, value]) => `${key}=${encodeURIComponent(String(value))}`).join("&")
    : "";
}

export function currentSession(): Promise<SessionSummary | null> {
  return request("/api/session");
}

export function openFiles(
  old: string,
  newPath: string,
  key: string[],
  hints: string[],
): Promise<SessionSummary> {
  return request("/api/open", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ old, new: newPath, key, hints }),
  });
}

export function applyHints(hints: string[]): Promise<SessionSummary> {
  return request("/api/hints", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ hints }),
  });
}

export function schemaPanel(changedOnly: boolean): Promise<SchemaRow[]> {
  return request(`/api/schema${query({ changed_only: changedOnly })}`);
}

export function cellsPage(
  column: number | null,
  row: number | null,
  sort: string,
  page: number,
  pageSize: number,
): Promise<Page<CellRow>> {
  return request(
    `/api/cells${query({ column, row, sort, page, page_size: pageSize })}`,
  );
}

export function columnView(
  allColumns: boolean,
  allRows: boolean,
  includeAddedDropped: boolean,
  page: number,
  pageSize: number,
): Promise<ColumnViewData> {
  return request(
    `/api/column-view${query({
      all_columns: allColumns,
      all_rows: allRows,
      added_dropped: includeAddedDropped,
      page,
      page_size: pageSize,
    })}`,
  );
}

export function rowViewSection(
  kind: string,
  allColumns: boolean,
  page: number,
  pageSize: number,
): Promise<RowViewData> {
  return request(
    `/api/row-view${query({ kind, all_columns: allColumns, page, page_size: pageSize })}`,
  );
}
