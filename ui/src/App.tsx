import { useEffect, useRef, useState } from "preact/hooks";
import { currentSession, onRequestError, openFiles } from "./api";
import { CellView } from "./components/CellView";
import { ColumnView, type ColumnOptions } from "./components/ColumnView";
import { EditedView, FanoutView, RowsKindView } from "./components/RowView";
import { SchemaPanel } from "./components/SchemaPanel";
import { Sidebar } from "./components/Sidebar";
import { ThemeToggle } from "./components/ThemeToggle";
import { Toast, Toasts } from "./components/Toasts";
import { Toggle } from "./components/Toggle";
import type { Selection, SessionSummary } from "./types";

export function App() {
  const [summary, setSummary] = useState<SessionSummary | null>(null);
  // The schema view is always the opening view.
  const [selection, setSelection] = useState<Selection>({ view: "schema" });
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextToast = useRef(1);

  // Per-view toolbar state, held here so it survives view switches.
  const [schemaAll, setSchemaAll] = useState(true);
  const [columnOptions, setColumnOptions] = useState<ColumnOptions>({
    allColumns: false,
    allRows: false,
    addedDropped: false,
  });
  const [editedAllColumns, setEditedAllColumns] = useState(false);
  const [cellByColumn, setCellByColumn] = useState(false);

  // Every failed request surfaces as a toast, wherever it came from.
  useEffect(() => {
    onRequestError((message) =>
      setToasts((current) => [...current, { id: nextToast.current++, message }]),
    );
  }, []);

  // A session launched with paths is already open; pick it up once.
  useEffect(() => {
    currentSession().then((existing) => {
      if (existing) setSummary(existing);
    });
  }, []);

  if (!summary) {
    return <OpenForm onOpen={setSummary} />;
  }

  return (
    <main class="app">
      <header class="app-header">
        <div class="head-title">
          <h1>data-diff</h1>
          <span class="paths">
            {summary.old_path} → {summary.new_path}
          </span>
        </div>
        <ThemeToggle />
      </header>
      {/* The toolbar sits above the whole body, its controls left-aligned
          with the main panel's edge. It stays visible even when the active
          view has no controls, so the layout never shifts. */}
      <div class="toolbar">
        {selection.view === "schema" && (
          <Toggle off="changed only" on="all columns" checked={schemaAll} onChange={setSchemaAll} />
        )}
        {selection.view === "columns" && (
          <>
            <Toggle
              off="changed cols"
              on="all cols"
              checked={columnOptions.allColumns}
              onChange={(value) => setColumnOptions((o) => ({ ...o, allColumns: value }))}
            />
            <Toggle
              off="changed rows"
              on="all rows"
              checked={columnOptions.allRows}
              onChange={(value) => setColumnOptions((o) => ({ ...o, allRows: value }))}
            />
            <Toggle
              off="without added/dropped"
              on="+ added/dropped"
              checked={columnOptions.addedDropped}
              onChange={(value) => setColumnOptions((o) => ({ ...o, addedDropped: value }))}
            />
          </>
        )}
        {selection.view === "edited" && (
          <Toggle
            off="changed columns"
            on="all columns"
            checked={editedAllColumns}
            onChange={setEditedAllColumns}
          />
        )}
        {selection.view === "cells" && (
          <Toggle off="by key" on="by column" checked={cellByColumn} onChange={setCellByColumn} />
        )}
      </div>
      <div class="app-body">
        <Sidebar summary={summary} selection={selection} onSelect={setSelection} />
        <section class="main-panel">
          <div class="view-body">
            {selection.view === "schema" && <SchemaPanel summary={summary} all={schemaAll} />}
            {selection.view === "columns" && (
              <ColumnView keyColumns={summary.key_columns} options={columnOptions} />
            )}
            {selection.view === "edited" && (
              <EditedView
                keyColumns={summary.key_columns}
                group={selection.group}
                allColumns={editedAllColumns}
              />
            )}
            {(selection.view === "added" || selection.view === "dropped" || selection.view === "moved") && (
              <RowsKindView kind={selection.view} keyColumns={summary.key_columns} />
            )}
            {selection.view === "fanout" && <FanoutView keyColumns={summary.key_columns} />}
            {selection.view === "cells" && (
              <CellView total={summary.cells} keyColumns={summary.key_columns} byColumn={cellByColumn} />
            )}
          </div>
        </section>
      </div>
      <Toasts
        toasts={toasts}
        onDismiss={(id) => setToasts((current) => current.filter((t) => t.id !== id))}
      />
    </main>
  );
}

function OpenForm({ onOpen }: { onOpen: (summary: SessionSummary) => void }) {
  const [oldPath, setOldPath] = useState("");
  const [newPath, setNewPath] = useState("");
  const [key, setKey] = useState("");
  const [error, setError] = useState<string | null>(null);

  const submit = () => {
    openFiles(
      oldPath,
      newPath,
      key ? key.split(",").map((name) => name.trim()) : [],
      [],
    ).then(onOpen, (e) => setError(String(e)));
  };

  return (
    <main class="open-form">
      <h1>data-diff</h1>
      <p>Enter the paths of the two Parquet files to compare.</p>
      <label>
        old{" "}
        <input
          value={oldPath}
          placeholder="/path/to/old.parquet"
          onInput={(e) => setOldPath((e.target as HTMLInputElement).value)}
        />
      </label>
      <label>
        new{" "}
        <input
          value={newPath}
          placeholder="/path/to/new.parquet"
          onInput={(e) => setNewPath((e.target as HTMLInputElement).value)}
        />
      </label>
      <label>
        key columns (comma-separated, optional){" "}
        <input value={key} onInput={(e) => setKey((e.target as HTMLInputElement).value)} />
      </label>
      <button disabled={!oldPath || !newPath} onClick={submit}>
        Compare
      </button>
      {error && <p class="error">{error}</p>}
    </main>
  );
}
