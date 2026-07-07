import { Check, Minus } from "lucide-react";

interface CheckboxProps {
  checked: boolean;
  /** Header "some rows selected" state — renders a dash glyph. */
  indeterminate?: boolean;
  onChange: () => void;
  disabled?: boolean;
  ariaLabel: string;
}

/**
 * (W50H) MDC-style checkbox (MLX parity): 14×14 visible box inside a 16px
 * hitbox, 1px #817E7E border, 3px radius; accent fill when checked.
 */
export function Checkbox({
  checked,
  indeterminate = false,
  onChange,
  disabled,
  ariaLabel,
}: CheckboxProps) {
  const active = checked || indeterminate;
  return (
    <span className="relative inline-grid h-4 w-4 shrink-0 place-items-center align-middle">
      <input
        type="checkbox"
        aria-label={ariaLabel}
        checked={checked}
        ref={(el) => {
          if (el) el.indeterminate = indeterminate && !checked;
        }}
        onChange={onChange}
        disabled={disabled}
        className="peer absolute inset-0 z-10 m-0 h-4 w-4 cursor-pointer opacity-0 disabled:cursor-not-allowed"
      />
      <span
        aria-hidden="true"
        className={`grid h-3.5 w-3.5 place-items-center rounded-[3px] border transition-colors peer-focus-visible:ring-2 peer-focus-visible:ring-accent/60 peer-disabled:opacity-40 ${
          active
            ? "border-accent bg-accent text-white"
            : "border-[#817E7E] bg-transparent text-transparent"
        }`}
      >
        {indeterminate && !checked ? (
          <Minus className="h-3 w-3" strokeWidth={3} />
        ) : (
          checked && <Check className="h-3 w-3" strokeWidth={3} />
        )}
      </span>
    </span>
  );
}
