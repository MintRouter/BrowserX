import { type ReactNode, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SUPPORTED_LOCALES, setLocale } from "../i18n";
import { AUTO_CLEAR_CACHE_SETTING, api, isTauri } from "../lib/api";
import { useTheme } from "../lib/theme";
import { Toggle } from "./profile-form/controls";
import { ThemeCards } from "./settings/ThemeCards";

/** Fallback persistence when running outside Tauri (plain browser dev). */
const AUTO_CLEAR_STORAGE_KEY = "browserx.autoClearCache";

/** Section title 18px/500 without bottom border (audit R6 §5.1/§5.4). */
function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="space-y-4">
      <h2 className="text-lg font-medium text-fg">{title}</h2>
      {children}
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
    <div className="flex items-center justify-between gap-4">
      <div className="min-w-0">
        <p className="text-sm font-medium text-fg">{label}</p>
        {hint && <p className="mt-0.5 text-xs text-fg-muted">{hint}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

/**
 * Account/Settings screen — Multilogin-style 2-column layout (audit R6 màn 5):
 * account sidebar card (left) + white content card (right).
 *
 * Intentionally omitted vs Multilogin (n/a for a local-only app):
 * - Sidebar items Subscription / Pricing / Invoices / Buy ISP proxy /
 *   Buy proxy traffic / Buy mobile minutes (cloud billing).
 * - Email / Default workspace / Workspace ID + copy blocks (no cloud account).
 */
export function SettingsView() {
  const { t, i18n } = useTranslation();
  const { mode, setMode } = useTheme();
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

  const language = SUPPORTED_LOCALES.some((l) => l.code === i18n.language)
    ? i18n.language
    : "en";

  return (
    <div className="flex h-full min-h-0 gap-4 p-4">
      {/* Account sidebar: item h=36 r6, selected #F0F6FF/#055FF0 14/500 (audit R6 màn 5) */}
      <nav
        aria-label={t("settings.title")}
        className="card w-[270px] shrink-0 self-start p-3"
      >
        <button
          type="button"
          aria-current="page"
          className="flex h-9 w-full items-center rounded-md bg-[#F0F6FF] px-3 text-sm font-medium text-accent"
        >
          {t("settings.accountSettings")}
        </button>
      </nav>

      {/* Content card */}
      <div className="card min-h-0 flex-1 overflow-y-auto">
        <div className="max-w-2xl space-y-8 p-6">
          <Section title={t("settings.interfaceTheme")}>
            <ThemeCards value={mode} onChange={setMode} />
          </Section>

          <Section title={t("settings.profileBehavior")}>
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
          </Section>

          <Section title={t("settings.localization")}>
            <SettingRow
              label={t("settings.language")}
              hint={t("settings.languageHint")}
            >
              <select
                aria-label={t("settings.language")}
                className="input w-48 py-1.5 text-sm"
                value={language}
                onChange={(e) => setLocale(e.target.value)}
              >
                {SUPPORTED_LOCALES.map((l) => (
                  <option key={l.code} value={l.code}>
                    {l.label}
                  </option>
                ))}
              </select>
            </SettingRow>
            <SettingRow
              label={t("settings.dateFormat")}
              hint={t("settings.comingSoon")}
            >
              <select
                disabled
                className="input w-48 cursor-not-allowed py-1.5 text-sm opacity-50"
              >
                <option>DD/MM/YYYY</option>
              </select>
            </SettingRow>
          </Section>

          <Section title={t("settings.support")}>
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
          </Section>

          {saveError && (
            <p className="text-xs text-danger" role="alert">
              {t("settings.saveFailed")}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
