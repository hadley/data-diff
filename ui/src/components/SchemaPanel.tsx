import { useEffect, useState } from "preact/hooks";
import { schemaPanel } from "../api";
import type { SchemaRow, SessionSummary, Side } from "../types";
import { Swatch } from "./Swatch";

interface SchemaPanelProps {
  summary: SessionSummary;
  /** The toolbar's changed-only/all-columns toggle. */
  all: boolean;
  /** The toolbar's old/new toggle: which file's position and name show. */
  side: Side;
}

/** The schema alignment: identities, additions, and drops, one side at a time. */
export function SchemaPanel({ summary, all, side }: SchemaPanelProps) {
  const [rows, setRows] = useState<SchemaRow[]>(summary.schema);

  useEffect(() => {
    if (all) {
      // The summary carries the changed-only rows; the full alignment needs
      // a fetch with the toggle off.
      schemaPanel(false).then(setRows, () => {});
    } else {
      setRows(summary.schema);
    }
  }, [all, summary]);

  return (
    <div class="schema-panel">
      <table>
        <thead>
          <tr>
            <th class="marker" />
            <th aria-label="key" />
            <th>#</th>
            <th>{side}</th>
            <th>type</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr key={index} class={row.status}>
              <td class="marker">
                {row.status === "added" ? (
                  <Swatch kind="added" />
                ) : row.status === "dropped" ? (
                  <Swatch kind="deleted" />
                ) : row.moved || row.type_change !== null || row.basis !== null ? (
                  <Swatch kind="edited" />
                ) : (
                  ""
                )}
              </td>
              <td>{row.key && <span class="key-badge">PK</span>}</td>
              {/* A one-sided row has a position and name only on its own
                  side; an identity shows the chosen side. */}
              <td>{(side === "old" ? row.old_pos : row.new_pos) ?? ""}</td>
              <td>{(side === "old" ? row.old_name : row.new_name) ?? ""}</td>
              <td class="type">
                {row.type_change ? (
                  <span class="type-change">
                    {row.type_change[0]} → {row.type_change[1]}
                  </span>
                ) : (
                  (row.source_type ?? "")
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
