/**
 * The change vocabulary's marker: the soft fill as a square (the cell's
 * look) with the full hue as a small circle inside. Used by the header
 * legend and by the marker column in the schema and row views.
 */
export function Swatch({ kind }: { kind: "added" | "edited" | "deleted" }) {
  return <span class={`swatch ${kind}`} aria-hidden="true" />;
}
