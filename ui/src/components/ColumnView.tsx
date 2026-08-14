import { useState } from "preact/hooks";
import { columnView } from "../api";
import type { ColumnHeader, Side } from "../types";
import { FrozenTd, FrozenTh } from "./PagedTable";
import { ChangeTooltip, ValueText } from "./ValueText";
import { usePages, VirtualTable } from "./VirtualTable";

/** The column view's three changed/all axes, lifted to the toolbar. */
export interface ColumnOptions {
  allColumns: boolean;
  allRows: boolean;
  addedDropped: boolean;
}

/**
 * Every edited column, keyed rows aligned: a row that changed in two
 * columns shows both edits on one line. `side` picks which file's values
 * the changed columns show.
 */
export function ColumnView({
  keyColumns,
  options,
  group,
  side,
}: {
  keyColumns: string[];
  options: ColumnOptions;
  group: number | null;
  side: Side;
}) {
  const { allColumns, allRows, addedDropped } = options;
  const [columns, setColumns] = useState<ColumnHeader[]>([]);
  const list = usePages(
    (page, pageSize) =>
      columnView(allColumns, allRows, addedDropped, group, page, pageSize).then((data) => {
        setColumns(data.columns);
        return data.rows;
      }),
    [allColumns, allRows, addedDropped, group],
  );

  const colSpan = keyColumns.length + columns.length;

  return (
    <VirtualTable
      total={list.total}
      colSpan={colSpan}
      ensure={list.ensure}
      version={list.version}
      head={
        <tr>
          {keyColumns.map((name, i) => (
            <FrozenTh index={i}>{name}</FrozenTh>
          ))}
          {columns.map((column) => (
            <th class={`single ${column.origin}`}>{column.name}</th>
          ))}
        </tr>
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
            {row.cells.map((cell, i) => {
              const column = columns[i];
              // A one-sided (added/dropped) column has a value only on its
              // own side; a changed column follows the toolbar's side.
              const value =
                column.span === "single"
                  ? column.side === "old"
                    ? cell.old
                    : cell.new
                  : side === "old"
                    ? cell.old
                    : cell.new;
              const tip = column.span !== "single" && cell.changed && cell.old && cell.new;
              return (
                <td
                  class={
                    column.span === "single"
                      ? `single ${column.origin}`
                      : cell.changed
                        ? "changed"
                        : ""
                  }
                >
                  {tip ? (
                    <ChangeTooltip old={cell.old!} newValue={cell.new!}>
                      {value && <ValueText value={value} />}
                    </ChangeTooltip>
                  ) : (
                    value && <ValueText value={value} />
                  )}
                </td>
              );
            })}
          </tr>
        );
      }}
    />
  );
}
