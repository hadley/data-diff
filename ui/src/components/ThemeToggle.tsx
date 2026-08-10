import { useEffect, useState } from "preact/hooks";

type Theme = "light" | "dark" | null;

function chosen(): Theme {
  try {
    const value = localStorage.getItem("dd-theme");
    return value === "light" || value === "dark" ? value : null;
  } catch {
    return null;
  }
}

function isDark(theme: Theme): boolean {
  if (theme) return theme === "dark";
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
}

/** The one toggle, light or dark; with nothing chosen the page follows the system. */
export function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>(chosen);

  useEffect(() => {
    const root = document.documentElement;
    if (theme) {
      root.dataset.theme = theme;
      try {
        localStorage.setItem("dd-theme", theme);
      } catch {}
    } else {
      delete root.dataset.theme;
      try {
        localStorage.removeItem("dd-theme");
      } catch {}
    }
  }, [theme]);

  const dark = isDark(theme);
  const next = dark ? "light" : "dark";
  return (
    <button
      class={`icon-btn theme-toggle${dark ? " is-dark" : ""}`}
      title={`Switch to ${next} mode`}
      aria-label={`Switch to ${next} mode`}
      onClick={() => setTheme(next)}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true">
        <circle cx="10" cy="10" r="8" fill="none" stroke="currentColor" stroke-width="1.6" />
        <path d="M10 2a8 8 0 0 1 0 16z" fill="currentColor" />
      </svg>
    </button>
  );
}
