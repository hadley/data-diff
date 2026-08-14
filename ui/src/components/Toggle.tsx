interface ToggleProps {
  on: string;
  off: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

/** The views' changed/all control: a segmented control, one segment per state. */
export function Toggle({ on, off, checked, onChange }: ToggleProps) {
  return (
    <span class="paired-toggle" role="group" aria-label={`${off} / ${on}`}>
      {/* The chosen segment is raised rather than outlined, so it says so
          itself: colour and elevation alone would not reach a screen reader. */}
      <button class={checked ? "" : "on"} aria-pressed={!checked} onClick={() => onChange(false)}>
        {off}
      </button>
      <button class={checked ? "on" : ""} aria-pressed={checked} onClick={() => onChange(true)}>
        {on}
      </button>
    </span>
  );
}
