import { useState } from "preact/hooks";
import type { ComponentChildren } from "preact";

interface ExpandoProps {
  title: string;
  count: number;
  children: ComponentChildren;
}

/** A section that minimized shows only kind and count, per the design. */
export function Expando({ title, count, children }: ExpandoProps) {
  const [open, setOpen] = useState(false);
  return (
    <section class="expando">
      <button class="expando-header" onClick={() => setOpen(!open)}>
        {open ? "▾" : "▸"} {title} ({count})
      </button>
      {open && <div class="expando-body">{children}</div>}
    </section>
  );
}
