/** Shared primitives for the profile form: toggle switch + segmented control. */

interface ToggleProps {
  checked: boolean;
  onChange?: (next: boolean) => void;
  disabled?: boolean;
  /** Accessible name (required — the visible label lives outside). */
  label: string;
  id?: string;
  title?: string;
}

/** Accessible switch (role="switch") in the W12 theme: blue track when on. */
export function Toggle({ checked, onChange, disabled, label, id, title }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      id={id}
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      title={title}
      onClick={() => onChange?.(!checked)}
      className={[
        "relative inline-flex h-5 w-11 shrink-0 items-center rounded-full",
        "transition-colors motion-reduce:transition-none",
        "focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:ring-offset-1",
        checked ? "bg-accent" : "bg-surface-4",
        disabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer",
      ].join(" ")}
    >
      <span
        aria-hidden="true"
        className={[
          "inline-block h-3.5 w-3.5 rounded-full bg-white shadow",
          "transition-transform motion-reduce:transition-none",
          checked ? "translate-x-[27px]" : "translate-x-[3px]",
        ].join(" ")}
      />
    </button>
  );
}

interface SegmentedProps<T extends string> {
  options: ReadonlyArray<{ value: T; label: string }>;
  value: T;
  onChange?: (value: T) => void;
  disabled?: boolean;
  /** Accessible name for the group. */
  label: string;
  title?: string;
}

/** Segmented control (toggle-button group) matching the W12 pill style. */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  disabled,
  label,
  title,
}: SegmentedProps<T>) {
  return (
    <div
      role="group"
      aria-label={label}
      title={title}
      className={[
        "inline-flex rounded-lg bg-[#F1F2F4] p-0.5 dark:bg-surface-2",
        disabled ? "opacity-50" : "",
      ].join(" ")}
    >
      {options.map((opt) => {
        const active = opt.value === value;
        return (
          <button
            key={opt.value}
            type="button"
            aria-pressed={active}
            disabled={disabled}
            onClick={() => onChange?.(opt.value)}
            className={[
              "rounded-md px-3 py-1 text-sm font-medium",
              "transition-colors motion-reduce:transition-none",
              "focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60",
              disabled ? "cursor-not-allowed" : "",
              active
                ? "bg-white text-accent shadow-sm dark:bg-surface-1"
                : "text-fg-muted hover:text-fg",
            ].join(" ")}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}
