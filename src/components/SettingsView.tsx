import { type ReactNode, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AUTO_CLEAR_CACHE_SETTING, api, isTauri } from "../lib/api";
import { type Theme, useTheme } from "../lib/theme";
import { LanguageSwitcher } from "./LanguageSwitcher";
import { Toggle } from "./profile-form/controls";

/** Fallback persistence when running outside Tauri (plain browser dev). */
const AUTO_CLEAR_STORAGE_KEY = "browserx.autoClearCache";

function SectionCard({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="card overflow-hidden">
      <h2 className="border-b border-border px-5 py-3 text-sm font-semibold text-fg">
        {title}
      </h2>
      <div className="divide-y divide-border">{children}</div>
    </section>
  );
}

function SettingRow({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 px-5 py-4">
      <div className="min-w-0">
        <p className="text-sm font-medium text-fg">{label}</p>
        {hint && <p className="mt-0.5 text-xs text-fg-muted">{hint}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

const pillBase =
  "rounded-md px-3 py-1 text-sm font-medium transition-colors motion-reduce:transition-none focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60";

/** Centralized settings page (avatar menu → Settings): language, theme, auto-clear cache. */
export function SettingsView() {
  const { t } = useTranslation();
  const { theme, setTheme } = useTheme();
  const [autoClear, setAutoClear] = useState(false);
  const [saveError, setSaveError] = useState(false);

  useEffect(() => {
    if (isTauri()) {
      api
        .getSettings()
        .then((s) => setAutoClear(s[AUTO_CLEAR_CACHE_SETTING] === "true"))
        .catch(() => {});
    } else {
      setAutoClear(localStorage.getItem(AUTO_CLEAR_STORAGE_KEY) === "true");
    }
  }, []);

  const handleAutoClear = (next: boolean) => {
    setAutoClear(next);
    setSaveError(false);
    if (isTauri()) {
      api.setSetting(AUTO_CLEAR_CACHE_SETTING, String(next)).catch(() => {
        setAutoClear(!next);
        setSaveError(true);
      });
    } else {
      localStorage.setItem(AUTO_CLEAR_STORAGE_KEY, String(next));
    }
  };

  const themeOptions: { value: Theme; label: string }[] = [
    { value: "light", label: t("settings.themeLight") },
    { value: "dark", label: t("settings.themeDark") },
  ];

  return (
    <div className="mx-auto max-w-3xl space-y-4 p-6">
      <h1 className="text-lg font-semibold text-fg">{t("settings.title")}</h1>

      <SectionCard title={t("settings.general")}>
        <SettingRow
          label={t("settings.language")}
          hint={t("settings.languageHint")}
        >
          <LanguageSwitcher />
        </SettingRow>
        <SettingRow label={t("settings.theme")} hint={t("settings.themeHint")}>
          <div
            role="group"
            aria-label={t("settings.theme")}
            className="inline-flex rounded-lg bg-surface-2 p-0.5"
          >
            {themeOptions.map((opt) => (
              <button
                key={opt.value}
                type="button"
                aria-pressed={theme === opt.value}
                onClick={() => setTheme(opt.value)}
                className={`${pillBase} ${
                  theme === opt.value
                    ? "bg-surface-1 text-fg shadow-sm"
                    : "text-fg-muted hover:text-fg"
                }`}
              >
                {opt.label}
              </button>
            ))}
            <button
              type="button"
              disabled
              title={t("settings.comingSoon")}
              className={`${pillBase} cursor-not-allowed text-fg-muted opacity-50`}
            >
              {t("settings.themeSystem")}
            </button>
          </div>
        </SettingRow>
        <SettingRow
          label={t("settings.dateFormat")}
          hint={t("settings.comingSoon")}
        >
          <select disabled className="input w-40 cursor-not-allowed py-1.5 text-sm opacity-50">
            <option>DD/MM/YYYY</option>
          </select>
        </SettingRow>
      </SectionCard>

      <SectionCard title={t("settings.profiles")}>
        <SettingRow
          label={t("settings.autoClearCache")}
          hint={t("settings.autoClearCacheHint")}
        >
          <Toggle
            checked={autoClear}
            onChange={handleAutoClear}
            label={t("settings.autoClearCache")}
          />
        </SettingRow>
      </SectionCard>

      <SectionCard title={t("settings.support")}>
        <SettingRow label={t("settings.logs")} hint={t("settings.logsHint")}>
          <button
            type="button"
            disabled={!isTauri()}
            onClick={() => {
              api.openLogsFolder().catch(() => {});
            }}
            className="btn-secondary disabled:cursor-not-allowed disabled:opacity-50"
          >
            {t("settings.openLogsFolder")}
          </button>
        </SettingRow>
      </SectionCard>

      {saveError && (
        <p className="text-xs text-danger" role="alert">
          {t("settings.saveFailed")}
        </p>
      )}
    </div>
  );
}
