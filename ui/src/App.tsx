import { useEffect, useRef, useState } from "preact/hooks";
import { currentSession, onRequestError, openFiles } from "./api";
import { CellView } from "./components/CellView";
import { ColumnView } from "./components/ColumnView";
import { RowView } from "./components/RowView";
import { SchemaPanel } from "./components/SchemaPanel";
import { ThemeToggle } from "./components/ThemeToggle";
import { Toast, Toasts } from "./components/Toasts";
import { openingView } from "./opening";
import type { SessionSummary, ViewKind } from "./types";

export function App() {
  const [summary, setSummary] = useState<SessionSummary | null>(null);
  const [view, setView] = useState<ViewKind>("cell");
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextToast = useRef(1);

  // Every failed request surfaces as a toast, wherever it came from.
  useEffect(() => {
    onRequestError((message) =>
      setToasts((current) => [...current, { id: nextToast.current++, message }]),
    );
  }, []);

  // A session launched with paths is already open; pick it up once.
  useEffect(() => {
    currentSession().then((existing) => {
      if (existing) {
        setSummary(existing);
        setView(openingView(existing));
      }
    });
  }, []);

  if (!summary) {
    return <OpenForm onOpen={(s) => { setSummary(s); setView(openingView(s)); }} />;
  }

  return (
    <main>
      <header class="app-header">
        <div class="head-title">
          <h1>data-diff</h1>
          <span class="paths">
            {summary.old_path} → {summary.new_path}
          </span>
        </div>
        <ThemeToggle />
      </header>
      <SchemaPanel summary={summary} />
      <section class="value-views">
        <nav class="tabs" role="tablist">
          {(["column", "row", "cell"] as ViewKind[]).map((kind) => (
            <button
              role="tab"
              aria-selected={view === kind}
              class={view === kind ? "tab active" : "tab"}
              onClick={() => setView(kind)}
            >
              {kind}
            </button>
          ))}
        </nav>
        <div class="view-body" role="tabpanel">
          {view === "column" && <ColumnView keyColumns={summary.key_columns} />}
          {view === "row" && <RowView summary={summary} />}
          {view === "cell" && <CellView total={summary.cells} keyColumns={summary.key_columns} />}
        </div>
      </section>
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
