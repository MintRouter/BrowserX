import { useTranslation } from "react-i18next";
import type { ThemeMode } from "../../lib/theme";

const MODES: ThemeMode[] = ["light", "dark", "system"];

/** Fixed palettes so each card always previews its own scheme (independent of the active theme). */
const PALETTE = {
  light: { bg: "#EAEDF3", card: "#FFFFFF", line: "#E5E1E1" },
  dark: { bg: "#0B0D10", card: "#14171C", line: "#2C323B" },
} as const;

const ACCENT = "#2563EB";

/** Mini app mock (sidebar + content card with text bars) in the given scheme. */
function Pane({ dark }: { dark: boolean }) {
  const p = dark ? PALETTE.dark : PALETTE.light;
  return (
    <div
      aria-hidden="true"
      className="flex h-full w-full gap-1 p-1.5"
      style={{ backgroundColor: p.bg }}
    >
      <div className="w-1/4 rounded-sm" style={{ backgroundColor: p.card }} />
      <div
        className="flex flex-1 flex-col gap-1 rounded-sm p-1.5"
        style={{ backgroundColor: p.card }}
      >
        <div
          className="h-1 w-1/2 rounded-full"
          style={{ backgroundColor: ACCENT }}
        />
        <div
          className="h-1 w-full rounded-full"
          style={{ backgroundColor: p.line }}
        />
        <div
          className="h-1 w-3/4 rounded-full"
          style={{ backgroundColor: p.line }}
        />
      </div>
    </div>
  );
}

function Preview({ mode }: { mode: ThemeMode }) {
  if (mode === "system") {
    // Half light / half dark to signal "follows the OS".
    return (
      <div className="flex h-16 w-full overflow-hidden">
        <div className="h-full w-1/2">
          <Pane dark={false} />
        </div>
        <div className="h-full w-1/2">
          <Pane dark />
        </div>
      </div>
    );
  }
  return (
    <div className="h-16 w-full">
      <Pane dark={mode === "dark"} />
    </div>
  );
}

interface ThemeCardsProps {
  value: ThemeMode;
  onChange: (mode: ThemeMode) => void;
}

/**
 * Multilogin-style interface-theme picker: 3 preview cards (Light / Dark /
 * System) with an accent border on the selected one (audit R6 §5.3).
 */
export function ThemeCards({ value, onChange }: ThemeCardsProps) {
  const { t } = useTranslation();
  const labels: Record<ThemeMode, string> = {
    light: t("settings.themeLight"),
    dark: t("settings.themeDark"),
    system: t("settings.themeSystem"),
  };

  return (
    <div
      role="radiogroup"
      aria-label={t("settings.theme")}
      className="grid max-w-md grid-cols-3 gap-3"
    >
      {MODES.map((mode) => {
        const selected = value === mode;
        return (
          <button
            key={mode}
            type="button"
            role="radio"
            aria-checked={selected}
            onClick={() => onChange(mode)}
            className={`overflow-hidden rounded-lg border-2 text-left transition-colors motion-reduce:transition-none focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 ${
              selected
                ? "border-accent"
                : "border-border hover:border-border-hover"
            }`}
          >
            <Preview mode={mode} />
            <span
              className={`block border-t border-border px-3 py-2 text-sm font-medium ${
                selected ? "text-accent" : "text-fg"
              }`}
            >
              {labels[mode]}
            </span>
          </button>
        );
      })}
    </div>
  );
}
