import type { ComponentChildren } from "preact";

/**
 * The table every value view shares. Key columns stay pinned during
 * horizontal scrolling — rendered with `FrozenTh`/`FrozenTd` — so a cell's
 * row identity is always visible.
 */
export function PagedTable({ children }: { children: ComponentChildren }) {
  return (
    <div class="table-scroll">
      <table class="paged-table">{children}</table>
    </div>
  );
}

/** The sticky offset of the i-th frozen column. */
const left = (index: number) => ({ left: `${index * 10}ch` });

export function FrozenTh({
  index,
  rowspan,
  children,
}: {
  index: number;
  rowspan?: number;
  children: ComponentChildren;
}) {
  return (
    <th class="frozen" rowspan={rowspan} style={left(index)}>
      {children}
    </th>
  );
}

export function FrozenTd({
  index,
  children,
}: {
  index: number;
  children: ComponentChildren;
}) {
  return (
    <td class="frozen" style={left(index)}>
      {children}
    </td>
  );
}
