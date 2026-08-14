import { useEffect, useState } from "preact/hooks";
import { cellsPage } from "../api";
import { FrozenTd, FrozenTh } from "./PagedTable";
import { ValueText } from "./ValueText";
import { usePages, VirtualTable } from "./VirtualTable";

/**
 * The flat evidence table: exactly Diff::cells, no more, no less — unless
 * `addedDropped` asks every non-key cell of each added and dropped row to
 * join, one side of the line then staying absent.
 */
export function CellView({
  total,
  keyColumns,
  byColumn,
  addedDropped,
}: {
  total: number;
  keyColumns: string[];
  byColumn: boolean;
  addedDropped: boolean;
}) {
  const [hasDelta, setHasDelta] = useState(false);
  const list = usePages(
    (page, pageSize) =>
      cellsPage(byColumn ? "column" : "key", addedDropped, page, pageSize).then((data) => {
        if (data.items.some((item) => item.delta != null)) setHasDelta(true);
        return data;
      }),
    [byColumn, addedDropped],
  );

  useEffect(() => setHasDelta(false), [byColumn, addedDropped]);

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
            {/* A one-sided line (an added or dropped row's cell) marks the
                side the row exists on and leaves the other absent. */}
            <td class={item.old ? (item.new ? "" : "deleted") : "absent"}>
              {item.old ? <ValueText value={item.old} /> : "—"}
            </td>
            <td class={item.new ? (item.old ? "changed" : "added") : "absent"}>
              {item.new ? <ValueText value={item.new} /> : "—"}
            </td>
            {hasDelta && (
              <td class="delta">{item.delta && <ValueText value={item.delta} />}</td>
            )}
          </tr>
        );
      }}
    />
  );
}
