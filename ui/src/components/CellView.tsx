import { useEffect, useState } from "preact/hooks";
import { cellsPage } from "../api";
import { FrozenTd, FrozenTh } from "./PagedTable";
import { ValueText } from "./ValueText";
import { usePages, VirtualTable } from "./VirtualTable";

/** The flat evidence table: exactly Diff::cells, no more, no less. */
export function CellView({
  total,
  keyColumns,
  byColumn,
}: {
  total: number;
  keyColumns: string[];
  byColumn: boolean;
}) {
  const [hasDelta, setHasDelta] = useState(false);
  const list = usePages(
    (page, pageSize) =>
      cellsPage(byColumn ? "column" : "key", page, pageSize).then((data) => {
        if (data.items.some((item) => item.delta != null)) setHasDelta(true);
        return data;
      }),
    [byColumn],
  );

  useEffect(() => setHasDelta(false), [byColumn]);

  const colSpan = keyColumns.length + 3 + (hasDelta ? 1 : 0);

  return (
    <VirtualTable
      total={list.total ?? total}
      colSpan={colSpan}
      ensure={list.ensure}
      version={list.version}
      head={
        <tr>
          {keyColumns.map((name, i) => (
            <FrozenTh index={i}>{name}</FrozenTh>
          ))}
          <th class="col-name">column</th>
          <th>old</th>
          <th>new</th>
          {hasDelta && <th class="delta">Δ</th>}
        </tr>
      }
      renderRow={(index) => {
        const item = list.item(index);
        if (!item) {
          return (
            <tr key={index} class="pending">
              <td colSpan={colSpan} />
            </tr>
          );
        }
        return (
          <tr key={index}>
            {item.key.map((value, i) => (
              <FrozenTd index={i}>
                <ValueText value={value} />
              </FrozenTd>
            ))}
            <td class="col-name">{item.column}</td>
            <td><ValueText value={item.old} /></td>
            <td class="changed"><ValueText value={item.new} /></td>
            {hasDelta && (
              <td class="delta">{item.delta && <ValueText value={item.delta} />}</td>
            )}
          </tr>
        );
      }}
    />
  );
}
