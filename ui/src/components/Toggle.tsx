interface ToggleProps {
  on: string;
  off: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

/** The views' changed/all control: a segmented control, one segment per state. */
export function Toggle({ on, off, checked, onChange }: ToggleProps) {
  // Either segment flips the state: clicking the active one switches too,
  // so the control behaves like a switch as well as a pair of buttons.
  const flip = () => onChange(!checked);
  return (
    <span class="paired-toggle" role="group" aria-label={`${off} / ${on}`}>
      {/* The chosen segment is raised rather than outlined, so it says so
          itself: colour and elevation alone would not reach a screen reader. */}
      <button class={checked ? "" : "on"} aria-pressed={!checked} onClick={flip}>
        {off}
      </button>
      <button class={checked ? "on" : ""} aria-pressed={checked} onClick={flip}>
        {on}
      </button>
    </span>
  );
}
