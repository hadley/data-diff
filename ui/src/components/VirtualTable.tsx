import { useEffect, useRef, useState } from "preact/hooks";
import type { ComponentChildren } from "preact";
import type { Page } from "../types";

/**
 * The row height the windowed tables and the CSS agree on; the scroll
 * math below is meaningless if `.virtual-table` rows render at any other
 * height.
 */
export const ROW_HEIGHT = 29;
const PAGE_SIZE = 50;
const OVERSCAN = 12;

/**
 * A page cache over a paginated endpoint. Pages load on demand as the
 * windowed table asks for ranges; changing `deps` (a toggle, a selected
 * group) resets the cache. State lives in refs so `ensure` never acts on
 * a stale closure; `version` exists only to trigger rendering.
 */
export function usePages<T>(
  fetcher: (page: number, pageSize: number) => Promise<Page<T>>,
  deps: unknown[],
) {
  const pages = useRef(new Map<number, T[]>());
  const total = useRef<number | null>(null);
  const loading = useRef(new Set<number>());
  const fetcherRef = useRef(fetcher);
  fetcherRef.current = fetcher;
  const [version, setVersion] = useState(0);

  // Reset on a deps change, but not on mount: effects run child-first, so
  // the table's first `ensure` fires before this effect, and clearing the
  // in-flight set here would double-fetch the first page.
  const mounted = useRef(false);
  useEffect(() => {
    if (!mounted.current) {
      mounted.current = true;
      return;
    }
    pages.current = new Map();
    total.current = null;
    loading.current = new Set();
    setVersion((v) => v + 1);
  }, deps);

  const ensure = (start: number, end: number) => {
    // Every page the window [start, end) touches.
    const firstPage = Math.floor(start / PAGE_SIZE);
    const lastPage = Math.floor(Math.max(start, end - 1) / PAGE_SIZE);
    for (let page = firstPage; page <= lastPage; page++) {
      if (pages.current.has(page) || loading.current.has(page)) continue;
      if (total.current !== null && page * PAGE_SIZE >= total.current) continue;
      loading.current.add(page);
      fetcherRef.current(page, PAGE_SIZE).then(
        (result) => {
          loading.current.delete(page);
          pages.current.set(page, result.items);
          total.current = result.total;
          setVersion((v) => v + 1);
        },
        () => loading.current.delete(page),
      );
    }
  };

  return {
    version,
    get total() {
      return total.current;
    },
    item: (index: number) =>
      pages.current.get(Math.floor(index / PAGE_SIZE))?.[index % PAGE_SIZE],
    ensure,
  };
}

interface VirtualTableProps {
  /** The full row count; null until the first page arrives. */
  total: number | null;
  /** The table's column count, for the spacer rows. */
  colSpan: number;
  /** Ask for the rows in [start, end) to be loaded. */
  ensure: (start: number, end: number) => void;
  /**
   * The page cache's version: bumping it (a toggle reset the cache)
   * re-requests the visible window even though the range is unchanged.
   */
  version?: number;
  /** One `<tr>` for the row at index, loaded or not. */
  renderRow: (index: number) => ComponentChildren;
  /** The thead content. */
  head: ComponentChildren;
  /** The table has a pinned marker column before the key columns. */
  marker?: boolean;
}

/**
 * The full-height scrolling table every flat view shares. Only the
 * visible window of rows (plus overscan) is rendered; spacer rows above
 * and below hold the scroll position, and the header and key columns
 * stay pinned while scrolling.
 */
export function VirtualTable({
  total,
  colSpan,
  ensure,
  version = 0,
  renderRow,
  head,
  marker = false,
}: VirtualTableProps) {
  const scroll = useRef<HTMLDivElement>(null);
  const [range, setRange] = useState({ start: 0, end: OVERSCAN });

  const update = () => {
    const el = scroll.current;
    if (!el || total === null) return;
    const first = Math.max(0, Math.floor(el.scrollTop / ROW_HEIGHT) - OVERSCAN);
    const visible = Math.ceil(el.clientHeight / ROW_HEIGHT) + 2 * OVERSCAN;
    const start = Math.min(first, total);
    const end = Math.min(first + visible, total);
    setRange((current) =>
      current.start === start && current.end === end ? current : { start, end },
    );
  };

  useEffect(update, [total]);
  useEffect(() => {
    ensure(range.start, range.end);
  }, [range, total, version]);

  const rows = [];
  if (total !== null) {
    for (let index = range.start; index < range.end; index++) {
      rows.push(renderRow(index));
    }
  }

  return (
    <div class="table-scroll" ref={scroll} onScroll={update}>
      <table class={`paged-table virtual-table ${marker ? "with-marker" : ""}`}>
        <thead>{head}</thead>
        <tbody>
          {range.start > 0 && (
            <tr class="spacer">
              <td colSpan={colSpan} style={{ height: range.start * ROW_HEIGHT }} />
            </tr>
          )}
          {rows}
          {total !== null && range.end < total && (
            <tr class="spacer">
              <td colSpan={colSpan} style={{ height: (total - range.end) * ROW_HEIGHT }} />
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
