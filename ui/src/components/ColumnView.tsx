import { useEffect, useState } from "preact/hooks";
import { columnView } from "../api";
import type { ColumnViewData } from "../types";
import { PagedTable } from "./PagedTable";
import { Pager } from "./Pager";
import { Toggle } from "./Toggle";
import { ValueText } from "./ValueText";

const PAGE_SIZE = 50;

/**
 * Every edited column side by side, keyed rows aligned: a row that changed
 * in two columns shows both edits on one line.
 */
export function ColumnView({ keyColumns }: { keyColumns: string[] }) {
  const [allColumns, setAllColumns] = useState(false);
  const [allRows, setAllRows] = useState(false);
  const [addedDropped, setAddedDropped] = useState(false);
  const [page, setPage] = useState(0);
  const [data, setData] = useState<ColumnViewData | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    columnView(allColumns, allRows, addedDropped, page, PAGE_SIZE).then(
      setData,
      (e) => setError(String(e)),
    );
  }, [allColumns, allRows, addedDropped, page]);

  const reset = (set: (value: boolean) => void) => (value: boolean) => {
    set(value);
    setPage(0);
  };

  const keyWidth = keyColumns.length;

  return (
    <div class="column-view">
      <header>
        <h2>Value changes by column</h2>
        <Toggle off="changed cols" on="all cols" checked={allColumns} onChange={reset(setAllColumns)} />
        <Toggle off="changed rows" on="all rows" checked={allRows} onChange={reset(setAllRows)} />
        <Toggle off="without added/dropped" on="+ added/dropped" checked={addedDropped} onChange={reset(setAddedDropped)} />
      </header>
      {error && <p class="error">{error}</p>}
      {data && (
        <>
          <PagedTable frozen={keyWidth}>
            <thead>
              <tr>
                {keyColumns.map((name, i) => (
                  <th class="frozen" rowspan={2} style={{ left: `${i * 10}ch` }}>{name}</th>
                ))}
                {data.columns.map((column) =>
                  column.span === "pair" ? (
                    <th colspan={2} class="group">{column.name}</th>
                  ) : (
                    <th rowspan={2} class={`single ${column.side}`}>{column.name}</th>
                  ),
                )}
              </tr>
              <tr>
                {data.columns.flatMap((column) =>
                  column.span === "pair"
                    ? [<th class="sub">old</th>, <th class="sub">new</th>]
                    : [],
                )}
              </tr>
            </thead>
            <tbody>
              {data.rows.items.map((row, index) => (
                <tr key={index}>
                  {row.key.map((value, i) => (
                    <td class="frozen" style={{ left: `${i * 10}ch` }}>
                      <ValueText value={value} />
                    </td>
                  ))}
                  {row.cells.flatMap((cell, i) => {
                    const column = data.columns[i];
                    if (column.span === "single") {
                      const value = column.side === "old" ? cell.old : cell.new;
                      return [
                        <td class="single">{value && <ValueText value={value} />}</td>,
                      ];
                    }
                    return [
                      <td class={cell.changed ? "changed" : ""}>
                        {cell.old && <ValueText value={cell.old} />}
                      </td>,
                      <td class={cell.changed ? "changed" : ""}>
                        {cell.new && <ValueText value={cell.new} />}
                      </td>,
                    ];
                  })}
                </tr>
              ))}
            </tbody>
          </PagedTable>
          <Pager
            page={data.rows.page}
            pageSize={data.rows.page_size}
            total={data.rows.total}
            onPage={setPage}
          />
        </>
      )}
    </div>
  );
}
