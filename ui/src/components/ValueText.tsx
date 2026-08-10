import type { ValueDto } from "../types";

/** A value in its kind's styling: null, NaN, and "" stay visibly distinct. */
export function ValueText({ value }: { value: ValueDto }) {
  if (value.kind === "null") return <span class="value null">null</span>;
  if (value.kind === "double" && value.text === "NaN")
    return <span class="value nan">NaN</span>;
  if (value.kind === "string" && value.text === "")
    return <span class="value empty">""</span>;
  return <span class={`value ${value.kind}`}>{value.text}</span>;
}
