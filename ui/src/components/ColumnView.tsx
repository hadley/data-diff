import { useState } from "preact/hooks";
import { columnView } from "../api";
import type { ColumnHeader } from "../types";
import { FrozenTd, FrozenTh } from "./PagedTable";
import { ValueText } from "./ValueText";
import { usePages, VirtualTable } from "./VirtualTable";

/** The column view's three changed/all axes, lifted to the toolbar. */
export interface ColumnOptions {
  allColumns: boolean;
  allRows: boolean;
  addedDropped: boolean;
}

/**
 * Every edited column side by side, keyed rows aligned: a row that changed
 * in two columns shows both edits on one line.
 */
export function ColumnView({
  keyColumns,
  options,
}: {
  keyColumns: string[];
  options: ColumnOptions;
}) {
  const { allColumns, allRows, addedDropped } = options;
  const [columns, setColumns] = useState<ColumnHeader[]>([]);
  const list = usePages(
    (page, pageSize) =>
      columnView(allColumns, allRows, addedDropped, page, pageSize).then((data) => {
        setColumns(data.columns);
        return data.rows;
      }),
    [allColumns, allRows, addedDropped],
  );

  const colSpan =
    keyColumns.length +
    columns.reduce((span, column) => span + (column.span === "pair" ? 2 : 1), 0);

  return (
    <VirtualTable
      total={list.total}
      colSpan={colSpan}
      ensure={list.ensure}
      version={list.version}
      head={
        <>
          <tr>
            {keyColumns.map((name, i) => (
              <FrozenTh index={i} rowspan={2}>{name}</FrozenTh>
            ))}
            {columns.map((column) =>
              column.span === "pair" ? (
                <th colspan={2} class="group">{column.name}</th>
              ) : (
                <th rowspan={2} class={`single ${column.origin}`}>{column.name}</th>
              ),
            )}
          </tr>
          <tr class="sub-row">
            {columns.flatMap((column) =>
              column.span === "pair"
                ? [<th class="sub">old</th>, <th class="sub">new</th>]
                : [],
            )}
          </tr>
        </>
      }
      renderRow={(index) => {
        const row = list.item(index);
        if (!row) {
          return (
            <tr key={index} class="pending">
              <td colSpan={colSpan} />
            </tr>
          );
        }
        return (
          <tr key={index}>
            {row.key.map((value, i) => (
              <FrozenTd index={i}>
                <ValueText value={value} />
              </FrozenTd>
            ))}
            {row.cells.flatMap((cell, i) => {
              const column = columns[i];
              if (column.span === "single") {
                const value = column.side === "old" ? cell.old : cell.new;
                return [
                  <td class={`single ${column.origin}`}>
                    {value && <ValueText value={value} />}
                  </td>,
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
        );
      }}
    />
  );
}
