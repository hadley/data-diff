import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(cleanup);
import type { Page, SessionSummary } from "../types";
import { Sidebar } from "./Sidebar";
import { Toggle } from "./Toggle";
import { ValueText } from "./ValueText";
import { ROW_HEIGHT, usePages, VirtualTable } from "./VirtualTable";

describe("ValueText", () => {
  it("keeps null, NaN, and the empty string visibly distinct", () => {
    render(
      <>
        <ValueText value={{ kind: "null", text: "null" }} />
        <ValueText value={{ kind: "double", text: "NaN" }} />
        <ValueText value={{ kind: "string", text: "" }} />
      </>,
    );
    expect(document.querySelector(".value.null")).not.toBeNull();
    expect(document.querySelector(".value.nan")).not.toBeNull();
    expect(document.querySelector(".value.empty")).not.toBeNull();
  });
});

describe("Toggle", () => {
  it("renders both states as paired buttons and reports clicks", () => {
    const clicks: boolean[] = [];
    const { rerender } = render(
      <Toggle off="changed rows" on="all rows" checked={false} onChange={(v) => clicks.push(v)} />,
    );
    const [changed, all] = screen.getAllByRole("button");
    expect(changed.className).toBe("on");
    expect(all.className).toBe("");

    fireEvent.click(all);
    expect(clicks).toEqual([true]);

    rerender(
      <Toggle off="changed rows" on="all rows" checked={true} onChange={(v) => clicks.push(v)} />,
    );
    expect(screen.getAllByRole("button")[1].className).toBe("on");
  });
});

function summary(overrides: Partial<SessionSummary>): SessionSummary {
  return {
    old_path: "old.parquet",
    new_path: "new.parquet",
    cells: 0,
    optimal: true,
    edited_columns: 0,
    edited_rows: 0,
    cover_columns: 0,
    cover_rows: 0,
    added_rows: 0,
    dropped_rows: 0,
    moved_rows: 0,
    fanout_groups: 0,
    key_columns: ["id"],
    schema: [],
    ...overrides,
  };
}

describe("Sidebar", () => {
  it("lists categories with counts and hides empty ones", () => {
    render(
      <Sidebar
        summary={summary({ cells: 87, edited_columns: 3, added_rows: 12 })}
        selection={{ view: "cells" }}
        onSelect={() => {}}
      />,
    );
    expect(screen.getByText("Schema")).toBeTruthy();
    expect(screen.getByText("Columns").nextSibling?.textContent).toBe("3");
    expect(screen.getByText("Cells").nextSibling?.textContent).toBe("87");
    expect(screen.getByText("Rows added").nextSibling?.textContent).toBe("12");
    // Empty categories are hidden.
    expect(screen.queryByText("Rows dropped")).toBeNull();
    expect(screen.queryByText("Rows moved")).toBeNull();
    expect(screen.queryByText("Fanout")).toBeNull();
    expect(screen.queryByText("Rows edited")).toBeNull();
  });

  it("reports selections", () => {
    const selections: unknown[] = [];
    render(
      <Sidebar
        summary={summary({ cells: 87 })}
        selection={{ view: "schema" }}
        onSelect={(s) => selections.push(s)}
      />,
    );
    fireEvent.click(screen.getByText("Cells"));
    expect(selections).toEqual([{ view: "cells" }]);
  });

  it("expands rows edited into one sub-entry per group", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() =>
        Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              groups: [
                { columns: ["a", "b"], rows: 4 },
                { columns: ["c"], rows: 1 },
              ],
            }),
        }),
      ),
    );
    const selections: unknown[] = [];
    render(
      <Sidebar
        summary={summary({ cover_rows: 5, edited_rows: 5 })}
        selection={{ view: "schema" }}
        onSelect={(s) => selections.push(s)}
      />,
    );
    const entry = await screen.findByText("a, b");
    expect(screen.getByText("c")).toBeTruthy();
    fireEvent.click(entry);
    expect(selections).toEqual([{ view: "edited", group: 0 }]);
    vi.unstubAllGlobals();
  });
});

describe("VirtualTable", () => {
  it("renders only a window of the rows", () => {
    const ensured: [number, number][] = [];
    render(
      <VirtualTable
        total={1000}
        colSpan={1}
        ensure={(start, end) => ensured.push([start, end])}
        head={<tr><th>v</th></tr>}
        renderRow={(i) => (
          <tr key={i}>
            <td>row {i}</td>
          </tr>
        )}
      />,
    );
    // A small window around the top, not a thousand rows.
    expect(screen.getByText("row 0")).toBeTruthy();
    expect(screen.queryByText("row 999")).toBeNull();
    expect(document.querySelectorAll("tbody tr").length).toBeLessThan(50);
    expect(ensured[0]).toEqual([0, expect.any(Number)]);
  });

  it("moves the window on scroll", () => {
    const ensured: [number, number][] = [];
    render(
      <VirtualTable
        total={1000}
        colSpan={1}
        ensure={(start, end) => ensured.push([start, end])}
        head={<tr><th>v</th></tr>}
        renderRow={(i) => (
          <tr key={i}>
            <td>row {i}</td>
          </tr>
        )}
      />,
    );
    const scroller = document.querySelector(".table-scroll")!;
    Object.defineProperty(scroller, "scrollTop", { value: 100 * ROW_HEIGHT, configurable: true });
    Object.defineProperty(scroller, "clientHeight", { value: 10 * ROW_HEIGHT, configurable: true });
    fireEvent.scroll(scroller);
    expect(screen.getByText("row 100")).toBeTruthy();
    expect(ensured.at(-1)![0]).toBeGreaterThan(0);
  });
});

describe("usePages", () => {
  function Harness({
    fetcher,
    dep,
  }: {
    fetcher: (page: number, size: number) => Promise<Page<string>>;
    dep?: unknown;
  }) {
    const list = usePages(fetcher, [dep]);
    return (
      <VirtualTable
        total={list.total}
        colSpan={1}
        ensure={list.ensure}
        version={list.version}
        head={<tr><th>v</th></tr>}
        renderRow={(i) => (
          <tr key={i}>
            <td>{list.item(i) ?? ""}</td>
          </tr>
        )}
      />
    );
  }

  it("fetches pages as the window asks for them", async () => {
    const fetcher = vi.fn((page: number, size: number) =>
      Promise.resolve({
        items: Array.from({ length: size }, (_, i) => `value ${page * size + i}`),
        total: 120,
        page,
        page_size: size,
      }),
    );
    render(<Harness fetcher={fetcher} />);
    // The first page loads on mount and its rows render.
    expect(await screen.findByText("value 0")).toBeTruthy();
    expect(fetcher).toHaveBeenCalledWith(0, 50);
    expect(fetcher).toHaveBeenCalledTimes(1);

    // Scrolling into the next page fetches it.
    const scroller = document.querySelector(".table-scroll")!;
    Object.defineProperty(scroller, "scrollTop", { value: 60 * ROW_HEIGHT, configurable: true });
    Object.defineProperty(scroller, "clientHeight", { value: 10 * ROW_HEIGHT, configurable: true });
    fireEvent.scroll(scroller);
    expect(await screen.findByText("value 60")).toBeTruthy();
    expect(fetcher).toHaveBeenCalledWith(1, 50);
  });

  it("refetches the visible window when the deps change", async () => {
    // The by-key/by-column toggle's regression: the reset cache must reload
    // without waiting for a scroll.
    const fetcher = vi.fn((page: number, size: number) =>
      Promise.resolve({
        items: Array.from({ length: size }, (_, i) => `value ${page * size + i}`),
        total: 120,
        page,
        page_size: size,
      }),
    );
    const { rerender } = render(<Harness fetcher={fetcher} dep="key" />);
    expect(await screen.findByText("value 0")).toBeTruthy();
    const calls = fetcher.mock.calls.length;

    rerender(<Harness fetcher={fetcher} dep="column" />);
    await vi.waitFor(() => expect(fetcher.mock.calls.length).toBeGreaterThan(calls));
  });
});
