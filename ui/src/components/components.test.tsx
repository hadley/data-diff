import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, describe, expect, it } from "vitest";

afterEach(cleanup);
import { Expando } from "./Expando";
import { Pager } from "./Pager";
import { Toggle } from "./Toggle";
import { ValueText } from "./ValueText";

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

describe("Expando", () => {
  it("shows kind and count minimized and expands on click", () => {
    render(
      <Expando title="EDITED" count={41}>
        <p>body</p>
      </Expando>,
    );
    expect(screen.getByText(/EDITED \(41\)/)).toBeTruthy();
    expect(screen.queryByText("body")).toBeNull();
    fireEvent.click(screen.getByRole("button"));
    expect(screen.getByText("body")).toBeTruthy();
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

describe("Pager", () => {
  it("paginates within bounds", () => {
    const pages: number[] = [];
    render(<Pager page={1} pageSize={50} total={200} onPage={(p) => pages.push(p)} />);
    const [prev, next] = screen.getAllByRole("button");
    fireEvent.click(prev);
    fireEvent.click(next);
    expect(pages).toEqual([0, 2]);
  });

  it("renders nothing for a single page", () => {
    render(<Pager page={0} pageSize={50} total={30} onPage={() => {}} />);
    expect(screen.queryByRole("navigation")).toBeNull();
  });
});
