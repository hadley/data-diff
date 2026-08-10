import type { ComponentChildren } from "preact";

interface PagedTableProps {
  /** How many leading columns are key columns, frozen at the left edge. */
  frozen: number;
  children: ComponentChildren;
}

/**
 * The table every value view shares. Key columns stay pinned during
 * horizontal scrolling, so a cell's row identity is always visible.
 */
export function PagedTable({ frozen, children }: PagedTableProps) {
  return (
    <div class="table-scroll">
      <table class="paged-table" style={{ "--frozen": frozen }}>
        {children}
      </table>
    </div>
  );
}
