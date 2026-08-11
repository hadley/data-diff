interface ToggleProps {
  on: string;
  off: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

/** The views' changed/all control: one paired button per state. */
export function Toggle({ on, off, checked, onChange }: ToggleProps) {
  return (
    <span class="paired-toggle" role="group" aria-label={`${off} / ${on}`}>
      <button class={checked ? "" : "on"} onClick={() => onChange(false)}>
        {off}
      </button>
      <button class={checked ? "on" : ""} onClick={() => onChange(true)}>
        {on}
      </button>
    </span>
  );
}
