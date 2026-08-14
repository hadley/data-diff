import type {
  CellRow,
  ColumnViewData,
  EditedGroupSummary,
  Page,
  RowViewData,
  SchemaRow,
  SessionSummary,
} from "./types";

// Request failures surface as toasts; the App registers the reporter.
let report: (message: string) => void = () => {};
export function onRequestError(fn: (message: string) => void) {
  report = fn;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  try {
    const response = await fetch(path, init);
    const body = await response.json();
    if (!response.ok) throw new Error(body.error ?? response.statusText);
    return body as T;
  } catch (error) {
    const message =
      error instanceof TypeError
        ? "Cannot reach the data-diff server — is it still running?"
        : String(error);
    report(message);
    throw error;
  }
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

// The hint surface returns to the frontend with its UI; the server route
// (POST /api/hints) is already in place.
export function schemaPanel(changedOnly: boolean): Promise<SchemaRow[]> {
  return request(`/api/schema${query({ changed_only: changedOnly })}`);
}

export function cellsPage(
  sort: string,
  page: number,
  pageSize: number,
): Promise<Page<CellRow>> {
  return request(`/api/cells${query({ sort, page, page_size: pageSize })}`);
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

/** The sidebar's "rows edited" sub-entries: one per shared changed-column set. */
export function editedGroups(): Promise<{ groups: EditedGroupSummary[] }> {
  return request("/api/edited-groups");
}

export function rowViewSection(
  kind: string,
  allColumns: boolean,
  group: number | null,
  page: number,
  pageSize: number,
): Promise<RowViewData> {
  return request(
    `/api/row-view${query({ kind, all_columns: allColumns, group, page, page_size: pageSize })}`,
  );
}
