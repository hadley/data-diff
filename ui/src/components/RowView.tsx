import { useEffect, useRef, useState } from "preact/hooks";
import { rowViewSection } from "../api";
import type { FanoutGroup, RowLine, Side } from "../types";
import { FrozenTd, FrozenTh, PagedTable } from "./PagedTable";
import { Swatch } from "./Swatch";
import { ChangeTooltip, ValueText } from "./ValueText";
import { usePages, VirtualTable } from "./VirtualTable";

/**
 * The edited rows, one line per row on the toolbar's chosen side. `group`
 * selects one of the sidebar's sub-entries — rows sharing a changed-column
 * set — and narrows the default columns to that set; null shows every
 * edited row.
 */
export function EditedView({
  keyColumns,
  group,
  allColumns,
  side,
}: {
  keyColumns: string[];
  group: number | null;
  allColumns: boolean;
  side: Side;
}) {
  const [columns, setColumns] = useState<string[]>([]);
  const list = usePages(
    (page, pageSize) =>
      rowViewSection("edited", allColumns, group, side, page, pageSize).then((data) => {
        setColumns(data.columns);
        return data.rows!;
      }),
    [allColumns, group, side],
  );

  return (
    // No label column: every line is the toolbar's chosen side, so the
    // old/new label would repeat what the toggle already says.
    <LinesVirtualTable
      keyColumns={keyColumns}
      columns={columns}
      list={list}
      showLabel={false}
      marker="edited"
      singleSide
    />
  );
}

/** The one-sided sections: added and dropped rows, and moved rows. */
export function RowsKindView({
  kind,
  keyColumns,
}: {
  kind: "added" | "dropped" | "moved";
  keyColumns: string[];
}) {
  const [columns, setColumns] = useState<string[]>([]);
  const list = usePages(
    (page, pageSize) =>
      rowViewSection(kind, false, null, null, page, pageSize).then((data) => {
        setColumns(data.columns);
        return data.rows!;
      }),
    [kind],
  );

  return (
    <LinesVirtualTable
      keyColumns={keyColumns}
      columns={columns}
      list={list}
      showLabel={kind === "moved"}
      marker={kind === "moved" ? null : kind === "added" ? "added" : "deleted"}
    />
  );
}

function LinesVirtualTable({
  keyColumns,
  columns,
  list,
  showLabel,
  marker,
  singleSide = false,
}: {
  keyColumns: string[];
  columns: string[];
  list: ReturnType<typeof usePages<RowLine>>;
  showLabel: boolean;
  /** The swatch the marker column shows; null for no marker column. */
  marker: "edited" | "added" | "deleted" | null;
  /** One line per row rather than stacked old/new pairs. */
  singleSide?: boolean;
}) {
  const colSpan = (marker ? 1 : 0) + (showLabel ? 1 : 0) + keyColumns.length + columns.length;
  return (
    <VirtualTable
      total={list.total}
      colSpan={colSpan}
      ensure={list.ensure}
      version={list.version}
      marker={marker !== null}
      head={
        <tr>
          {marker && <th class="marker" />}
          {showLabel && <th />}
          {keyColumns.map((name, i) => (
            <FrozenTh index={i}>{name}</FrozenTh>
          ))}
          {columns.map((column) => (
            <th>{column}</th>
          ))}
        </tr>
      }
      renderRow={(index) => {
        const line = list.item(index);
        if (!line) {
          return (
            <tr key={index} class="pending">
              <td colSpan={colSpan} />
            </tr>
          );
        }
        return (
          <tr
            key={index}
            class={
              line.label === "added" || line.label === "dropped"
                ? line.label
                : line.label.startsWith("new")
                  ? "new-line"
                  : "old-line"
            }
          >
            {/* One marker per row: on the old line of an edited pair, on
                every line of a one-sided row or a single-side edited line. */}
            {marker && (
              <td class="marker">
                {(singleSide || marker !== "edited" || line.label === "old") && (
                  <Swatch kind={marker} />
                )}
              </td>
            )}
            {showLabel && <td class="line-label">{line.label}</td>}
            {line.key.map((value, i) => (
              <FrozenTd index={i}>
                <ValueText value={value} />
              </FrozenTd>
            ))}
            {line.values.map((value, i) => {
              // A single-side edited line carries the hidden side in `alt`;
              // the tooltip orders them old → new whichever side is shown.
              const alt = line.changed[i] ? line.alt?.[i] : undefined;
              return (
                <td class={line.changed[i] ? "changed" : ""}>
                  {alt ? (
                    <ChangeTooltip
                      old={line.label === "old" ? value : alt}
                      newValue={line.label === "old" ? alt : value}
                    >
                      <ValueText value={value} />
                    </ChangeTooltip>
                  ) : (
                    <ValueText value={value} />
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

/**
 * The fanout groups, each an aligned table — the old row on top, each new
 * row below, changed cells highlighted. Groups are few and unevenly sized,
 * so this view appends on scroll rather than windowing.
 */
export function FanoutView({ keyColumns }: { keyColumns: string[] }) {
  const [groups, setGroups] = useState<FanoutGroup[]>([]);
  const [columns, setColumns] = useState<string[]>([]);
  const [total, setTotal] = useState<number | null>(null);
  const nextPage = useRef(0);
  const loading = useRef(false);

  const loadMore = () => {
    if (loading.current || (total !== null && groups.length >= total)) return;
    loading.current = true;
    rowViewSection("fanout", false, null, null, nextPage.current, 20).then((data) => {
      const page = data.groups!;
      nextPage.current += 1;
      loading.current = false;
      setColumns(data.columns);
      setTotal(page.total);
      setGroups((current) => [...current, ...page.items]);
    }, () => {
      loading.current = false;
    });
  };

  useEffect(loadMore, []);

  return (
    <div
      class="fanout-view"
      onScroll={(event) => {
        const el = event.target as HTMLDivElement;
        if (el.scrollTop + el.clientHeight >= el.scrollHeight - 200) loadMore();
      }}
    >
      {groups.map((group, index) => (
        <section class="fanout-group" key={index}>
          <h3>
            old row {group.old_row} → {group.new_rows.length} rows
          </h3>
          <PagedTable>
            <thead>
              <tr>
                <th />
                {keyColumns.map((name, i) => (
                  <FrozenTh index={i}>{name}</FrozenTh>
                ))}
                {columns.map((column) => (
                  <th>{column}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {group.lines.map((line, lineIndex) => (
                <tr
                  key={lineIndex}
                  class={line.label.startsWith("new") ? "new-line" : "old-line"}
                >
                  <td class="line-label">{line.label}</td>
                  {line.key.map((value, i) => (
                    <FrozenTd index={i}>
                      <ValueText value={value} />
                    </FrozenTd>
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
        </section>
      ))}
    </div>
  );
}
