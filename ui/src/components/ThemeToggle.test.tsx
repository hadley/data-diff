import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ThemeToggle } from "./ThemeToggle";

afterEach(() => {
  cleanup();
  localStorage.clear();
  delete document.documentElement.dataset.theme;
});

describe("ThemeToggle", () => {
  it("follows the system with nothing chosen, then forces a scheme on click", () => {
    render(<ThemeToggle />);
    expect(document.documentElement.dataset.theme).toBeUndefined();

    fireEvent.click(screen.getByRole("button"));
    const first = document.documentElement.dataset.theme;
    expect(["light", "dark"]).toContain(first);
    expect(localStorage.getItem("dd-theme")).toBe(first);

    fireEvent.click(screen.getByRole("button"));
    expect(document.documentElement.dataset.theme).toBe(
      first === "dark" ? "light" : "dark",
    );
  });

  it("respects a stored choice", () => {
    localStorage.setItem("dd-theme", "dark");
    render(<ThemeToggle />);
    // The choice applies on the first click's effect; render alone leaves
    // the pre-paint script's attribute untouched.
    fireEvent.click(screen.getByRole("button"));
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});
