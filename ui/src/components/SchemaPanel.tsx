import { useEffect, useState } from "preact/hooks";
import { schemaPanel } from "../api";
import type { SchemaRow, SessionSummary } from "../types";
import { Toggle } from "./Toggle";

interface SchemaPanelProps {
  summary: SessionSummary;
}

/** The two-sided schema alignment: identities, additions, and drops. */
export function SchemaPanel({ summary }: SchemaPanelProps) {
  const [all, setAll] = useState(false);
  const [rows, setRows] = useState<SchemaRow[]>(summary.schema);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (all) {
      // The summary carries the changed-only rows; the full alignment needs
      // a fetch with the toggle off.
      schemaPanel(false).then(setRows, (e) => setError(String(e)));
    } else {
      setRows(summary.schema);
    }
  }, [all, summary]);

  return (
    <div class="schema-panel">
      <header>
        <h2>Schema</h2>
        <Toggle off="changed only" on="all columns" checked={all} onChange={setAll} />
      </header>
      {error && <p class="error">{error}</p>}
      <table>
        <thead>
          <tr>
            <th aria-label="key" />
            <th>#</th>
            <th>old</th>
            <th />
            <th>#</th>
            <th>new</th>
            <th>type</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr key={index} class={row.status}>
              <td>{row.key && <span class="key-badge">PK</span>}</td>
              <td>{row.old_pos ?? ""}</td>
              <td>{row.old_name ?? ""}</td>
              <td>{row.status === "identity" ? "⇄" : row.status === "dropped" ? "✕" : ""}</td>
              <td>{row.status === "identity" ? (row.moved ? row.new_pos : "") : (row.new_pos ?? "")}</td>
              <td>{row.new_name ?? ""}</td>
              <td>
                {row.type_change && (
                  <span class="type-change">
                    {row.type_change[0]} → {row.type_change[1]}
                  </span>
                )}
              </td>
              <td>
                {row.basis && <span class="badge rename">rename ({row.basis})</span>}
                {row.status === "dropped" && <span class="badge dropped">dropped</span>}
                {row.status === "added" && <span class="badge added">added</span>}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
