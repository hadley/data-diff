import { useEffect, useState } from "preact/hooks";
import { columnGroups, editedGroups } from "../api";
import type { ColumnGroupSummary, EditedGroupSummary, Selection, SessionSummary } from "../types";

interface SidebarProps {
  summary: SessionSummary;
  selection: Selection;
  onSelect: (selection: Selection) => void;
}

/**
 * The component list: every category of change with its count, empty
 * categories hidden. "Rows edited" expands to one sub-entry per edited
 * group — the shared changed-column sets — and "Columns edited" to one per
 * multi-column group of a shared changed-row set, the CLI's granularity.
 */
export function Sidebar({ summary, selection, onSelect }: SidebarProps) {
  const [groups, setGroups] = useState<EditedGroupSummary[] | null>(null);
  const [columnGroupList, setColumnGroups] = useState<ColumnGroupSummary[] | null>(null);

  useEffect(() => {
    if (summary.cover_rows > 0) {
      editedGroups().then((data) => setGroups(data.groups), () => {});
    }
    if (summary.cover_columns > 0) {
      columnGroups().then((data) => setColumnGroups(data.groups), () => {});
    }
  }, [summary]);

  // The summary's schema rows are the changed-only alignment, but key
  // columns show even unchanged — they are not changes and don't count.
  const schemaChanges = summary.schema.filter(
    (row) => row.status !== "identity" || row.moved || row.type_change !== null || row.basis !== null,
  ).length;

  return (
    <nav class="sidebar" aria-label="diff components">
      <Entry
        label="Schema"
        count={schemaChanges}
        active={selection.view === "schema"}
        onClick={() => onSelect({ view: "schema" })}
      />
      {summary.cover_columns > 0 && (
        <>
          <Entry
            label="Columns edited"
            count={summary.cover_columns}
            active={selection.view === "columns" && selection.group === null}
            onClick={() => onSelect({ view: "columns", group: null })}
          />
          {columnGroupList?.map((group, index) => (
            <button
              key={index}
              class={`entry sub ${selection.view === "columns" && selection.group === index ? "active" : ""}`}
              onClick={() => onSelect({ view: "columns", group: index })}
            >
              <span class="entry-label">{group.columns.join(", ")}</span>
              <span class="count">{group.rows}</span>
            </button>
          ))}
        </>
      )}
      {summary.cover_rows > 0 && (
        <>
          <Entry
            label="Rows edited"
            count={summary.cover_rows}
            active={selection.view === "edited" && selection.group === null}
            onClick={() => onSelect({ view: "edited", group: null })}
          />
          {groups?.map((group, index) => (
            <button
              key={index}
              class={`entry sub ${selection.view === "edited" && selection.group === index ? "active" : ""}`}
              onClick={() => onSelect({ view: "edited", group: index })}
            >
              <span class="entry-label">{group.columns.join(", ")}</span>
              <span class="count">{group.rows}</span>
            </button>
          ))}
        </>
      )}
      {summary.added_rows > 0 && (
        <Entry
          label="Rows added"
          count={summary.added_rows}
          active={selection.view === "added"}
          onClick={() => onSelect({ view: "added" })}
        />
      )}
      {summary.dropped_rows > 0 && (
        <Entry
          label="Rows dropped"
          count={summary.dropped_rows}
          active={selection.view === "dropped"}
          onClick={() => onSelect({ view: "dropped" })}
        />
      )}
      {summary.moved_rows > 0 && (
        <Entry
          label="Rows moved"
          count={summary.moved_rows}
          active={selection.view === "moved"}
          onClick={() => onSelect({ view: "moved" })}
        />
      )}
      {summary.fanout_groups > 0 && (
        <Entry
          label="Fanout"
          count={summary.fanout_groups}
          active={selection.view === "fanout"}
          onClick={() => onSelect({ view: "fanout" })}
        />
      )}
      {summary.cells > 0 && (
        <Entry
          label="Cells edited"
          count={summary.cells}
          active={selection.view === "cells"}
          onClick={() => onSelect({ view: "cells" })}
        />
      )}
    </nav>
  );
}

function Entry({
  label,
  count,
  active,
  onClick,
}: {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button class={`entry ${active ? "active" : ""}`} onClick={onClick}>
      <span class="entry-label">{label}</span>
      <span class="count">{count}</span>
    </button>
  );
}
