import { useEffect, useState } from "preact/hooks";
import { cellsPage } from "../api";
import type { CellRow, Page } from "../types";
import { FrozenTd, FrozenTh, PagedTable } from "./PagedTable";
import { Pager } from "./Pager";
import { ValueText } from "./ValueText";

const PAGE_SIZE = 50;

/** The flat evidence table: exactly Diff::cells, no more, no less. */
export function CellView({ total, keyColumns }: { total: number; keyColumns: string[] }) {
  const [page, setPage] = useState(0);
  const [data, setData] = useState<Page<CellRow> | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    cellsPage(null, null, "key", page, PAGE_SIZE).then(setData, (e) => setError(String(e)));
  }, [page]);

  return (
    <div class="cell-view">
      <header>
        <h2>All changed cells</h2>
        <span class="count">{data?.total ?? total} cells</span>
      </header>
      {error && <p class="error">{error}</p>}
      {data && (
        <>
          <PagedTable>
            <thead>
              <tr>
                {keyColumns.map((name, i) => (
                  <FrozenTh index={i}>{name}</FrozenTh>
                ))}
                <th class="col-name">column</th>
                <th>old</th>
                <th>new</th>
              </tr>
            </thead>
            <tbody>
              {data.items.map((item, index) => (
                <tr key={index}>
                  {item.key.map((value, i) => (
                    <FrozenTd index={i}>
                      <ValueText value={value} />
                    </FrozenTd>
                  ))}
                  <td class="col-name">{item.column}</td>
                  <td><ValueText value={item.old} /></td>
                  <td class="changed"><ValueText value={item.new} /></td>
                </tr>
              ))}
            </tbody>
          </PagedTable>
          <Pager page={data.page} pageSize={data.page_size} total={data.total} onPage={setPage} />
        </>
      )}
    </div>
  );
}
