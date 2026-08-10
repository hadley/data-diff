import { useEffect, useState } from "preact/hooks";
import { rowViewSection } from "../api";
import type { RowViewData, SessionSummary } from "../types";
import { Expando } from "./Expando";
import { PagedTable } from "./PagedTable";
import { Pager } from "./Pager";
import { Toggle } from "./Toggle";
import { ValueText } from "./ValueText";

const PAGE_SIZE = 50;

/** The transpose of the column view: the two-level structure on the rows. */
export function RowView({ summary }: { summary: SessionSummary }) {
  return (
    <div class="row-view">
      <h2>Value changes by row</h2>
      <Section kind="edited" title="EDITED" count={summary.edited_rows} toggles keyColumns={summary.key_columns} />
      <Section kind="added" title="ADDED" count={summary.added_rows} keyColumns={summary.key_columns} />
      <Section kind="dropped" title="DROPPED" count={summary.dropped_rows} keyColumns={summary.key_columns} />
      <Section kind="moved" title="MOVED" count={summary.moved_rows} keyColumns={summary.key_columns} />
      <Section kind="fanout" title="FANOUT" count={summary.fanout_groups} keyColumns={summary.key_columns} />
    </div>
  );
}

function Section({
  kind,
  title,
  count,
  keyColumns,
  toggles = false,
}: {
  kind: string;
  title: string;
  count: number;
  keyColumns: string[];
  toggles?: boolean;
}) {
  if (count === 0) return null;
  return (
    <Expando title={title} count={count}>
      <SectionBody kind={kind} toggles={toggles} keyColumns={keyColumns} />
    </Expando>
  );
}

function SectionBody({ kind, toggles, keyColumns }: { kind: string; toggles: boolean; keyColumns: string[] }) {
  const [allColumns, setAllColumns] = useState(false);
  const [page, setPage] = useState(0);
  const [data, setData] = useState<RowViewData | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    rowViewSection(kind, allColumns, page, PAGE_SIZE).then(setData, (e) =>
      setError(String(e)),
    );
  }, [kind, allColumns, page]);

  if (error) return <p class="error">{error}</p>;
  if (!data) return null;

  return (
    <div>
      {toggles && (
        <Toggle
          off="changed columns"
          on="all columns"
          checked={allColumns}
          onChange={(value) => {
            setAllColumns(value);
            setPage(0);
          }}
        />
      )}
      {data.rows && (
        <>
          <LinesTable data={data} lines={data.rows.items} keyColumns={keyColumns} />
          <Pager
            page={data.rows.page}
            pageSize={data.rows.page_size}
            total={data.rows.total}
            onPage={setPage}
          />
        </>
      )}
      {data.groups && (
        <>
          {data.groups.items.map((group, index) => (
            <Expando
              key={index}
              title={`${group.old_row}`}
              count={group.new_rows.length}
            >
              <LinesTable data={data} lines={group.lines} keyColumns={keyColumns} />
            </Expando>
          ))}
          <Pager
            page={data.groups.page}
            pageSize={data.groups.page_size}
            total={data.groups.total}
            onPage={setPage}
          />
        </>
      )}
    </div>
  );
}

function LinesTable({
  data,
  lines,
  keyColumns,
}: {
  data: RowViewData;
  lines: import("../types").RowLine[];
  keyColumns: string[];
}) {
  return (
    <PagedTable frozen={keyColumns.length}>
      <thead>
        <tr>
          <th />
          {keyColumns.map((name, i) => (
            <th class="frozen" style={{ left: `${i * 10}ch` }}>{name}</th>
          ))}
          {data.columns.map((column) => (
            <th>{column}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {lines.map((line, index) => (
          <tr key={index} class={line.label.startsWith("new") ? "new-line" : "old-line"}>
            <td class="line-label">{line.label}</td>
            {line.key.map((value, i) => (
              <td class="frozen" style={{ left: `${i * 10}ch` }}>
                <ValueText value={value} />
              </td>
            ))}
            {line.values.map((value, i) => (
              <td class={line.changed[i] ? "changed" : ""}>
                <ValueText value={value} />
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </PagedTable>
  );
}
