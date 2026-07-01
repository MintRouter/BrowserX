import { Globe, Moon, Plus, Search, Sun } from "lucide-react";
import { useTranslation } from "react-i18next";
import { SUPPORTED_LOCALES, setLocale } from "../i18n";
import { useTheme } from "../lib/theme";

interface TopBarProps {
  search: string;
  onSearchChange: (value: string) => void;
  onNewProfile: () => void;
}

export function TopBar({ search, onSearchChange, onNewProfile }: TopBarProps) {
  const { t, i18n } = useTranslation();
  const { theme, setTheme } = useTheme();

  const nextLocale = () => {
    const idx = SUPPORTED_LOCALES.findIndex((l) => l.code === i18n.language);
    const next = SUPPORTED_LOCALES[(idx + 1) % SUPPORTED_LOCALES.length];
    if (next) setLocale(next.code);
  };

  const currentLocale =
    SUPPORTED_LOCALES.find((l) => l.code === i18n.language) ?? SUPPORTED_LOCALES[0];

  return (
    <header className="flex items-center gap-3 h-12 px-3 border-b border-border bg-surface-1 shrink-0">
      <div className="flex items-center gap-2">
        <img src="/brand-icon-180.png" alt="" className="h-6 w-6 rounded" />
        <span className="text-sm font-semibold tracking-tight">{t("app.name")}</span>
      </div>

      <div className="relative flex-1 max-w-xl mx-auto">
        <Search
          className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-fg-muted"
          aria-hidden="true"
        />
        <input
          type="search"
          value={search}
          onChange={(e) => onSearchChange(e.target.value)}
          placeholder={t("topbar.search")}
          aria-label={t("topbar.search")}
          className="input pl-8 py-1.5 text-xs"
        />
      </div>

      <div className="flex items-center gap-1.5">
        <button onClick={onNewProfile} className="btn-primary text-xs">
          <Plus className="h-3.5 w-3.5" aria-hidden="true" />
          <span>{t("topbar.newProfile")}</span>
        </button>
        <button
          onClick={nextLocale}
          className="btn-secondary px-2 text-xs"
          aria-label={t("topbar.changeLanguage")}
          title={t("topbar.changeLanguage")}
        >
          <Globe className="h-3.5 w-3.5" aria-hidden="true" />
          <span className="uppercase">{currentLocale.code}</span>
        </button>
        <button
          onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
          className="btn-secondary px-2"
          aria-label={t("topbar.toggleTheme")}
          title={t("topbar.toggleTheme")}
          aria-pressed={theme === "dark"}
        >
          {theme === "dark" ? (
            <Sun className="h-3.5 w-3.5" aria-hidden="true" />
          ) : (
            <Moon className="h-3.5 w-3.5" aria-hidden="true" />
          )}
        </button>
      </div>
    </header>
  );
}
