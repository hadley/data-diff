import type { ComponentChildren } from "preact";
import type { ValueDto } from "../types";

const NUMERIC = new Set(["int64", "double", "decimal"]);

/**
 * The difference between two displayed values, formatted with its sign, or
 * null unless both sides are finite numbers. Computed from the displayed
 * texts, so it matches what the reader sees.
 */
export function changeDelta(old: ValueDto, newValue: ValueDto): string | null {
  if (!NUMERIC.has(old.kind) || !NUMERIC.has(newValue.kind)) return null;
  const a = Number(old.text);
  const b = Number(newValue.text);
  if (!Number.isFinite(a) || !Number.isFinite(b)) return null;
  // toPrecision sheds the binary float tail (0.30000000000000004).
  const delta = Number((b - a).toPrecision(12));
  return `${delta >= 0 ? "+" : ""}${delta}`;
}

/**
 * A changed cell's hover tooltip: both sides styled as values, with the
 * difference on a second line for numerics. Wraps the cell's content, so
 * the hover target is the value itself.
 */
export function ChangeTooltip({
  old,
  newValue,
  children,
}: {
  old: ValueDto;
  newValue: ValueDto;
  children: ComponentChildren;
}) {
  const delta = changeDelta(old, newValue);
  return (
    <span class="tip-host">
      {children}
      <span class="tip" role="tooltip">
        <span class="tip-values">
          <ValueText value={old} />
          <span class="tip-arrow">→</span>
          <ValueText value={newValue} />
        </span>
        {delta && <span class="tip-delta">Δ {delta}</span>}
      </span>
    </span>
  );
}

/** A value in its kind's styling: null, NaN, and "" stay visibly distinct. */
export function ValueText({ value }: { value: ValueDto }) {
  if (value.kind === "null") return <span class="value null">null</span>;
  if (value.kind === "double" && value.text === "NaN")
    return <span class="value nan">NaN</span>;
  if (value.kind === "string" && value.text === "")
    return <span class="value empty">""</span>;
  return <span class={`value ${value.kind}`}>{value.text}</span>;
}
