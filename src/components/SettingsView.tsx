import { getVersion } from "@tauri-apps/api/app";
import { Check, Copy } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SUPPORTED_LOCALES, setLocale } from "../i18n";
import { AUTO_CLEAR_CACHE_SETTING, api, isTauri } from "../lib/api";
import { useTheme } from "../lib/theme";
import { Toggle } from "./profile-form/controls";
import { AuditLog } from "./settings/AuditLog";
import { BackupDialog } from "./settings/BackupDialog";
import { RecoveryKeySection } from "./settings/RecoveryKeySection";
import { SystemPanel } from "./settings/SystemPanel";
import { TelegramSyncSection } from "./settings/TelegramSyncSection";
import { ThemeCards } from "./settings/ThemeCards";

/** Fallback persistence when running outside Tauri (plain browser dev). */
const AUTO_CLEAR_STORAGE_KEY = "browserx.autoClearCache";

/** Shown when getVersion() is unavailable (plain browser dev). */
const FALLBACK_APP_VERSION = "0.1.0";

/** All app data lives here — shown in the account info block (copyable). */
const DATA_FOLDER = "~/.browserx";

type SettingsPage = "account" | "backup" | "cloudSync" | "system" | "audit";

/** Functional sidebar entries — each maps to one content page. */
const NAV_ITEMS: { page: SettingsPage; labelKey: string }[] = [
  { page: "account", labelKey: "settings.accountSettings" },
  { page: "backup", labelKey: "settings.nav.backupSecurity" },
  { page: "cloudSync", labelKey: "settings.nav.cloudSync" },
  { page: "system", labelKey: "system.title" },
  { page: "audit", labelKey: "audit.title" },
];

/** Billing entries kept disabled for Multilogin visual parity (n/a locally). */
const BILLING_ITEMS: { labelKey: string; beta?: boolean }[] = [
  { labelKey: "settings.nav.subscription" },
  { labelKey: "settings.nav.pricing" },
  { labelKey: "settings.nav.invoices" },
  { labelKey: "settings.nav.buyIspProxy", beta: true },
  { labelKey: "settings.nav.buyProxyTraffic" },
  { labelKey: "settings.nav.buyMobileMinutes" },
];

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
 * ML-style account info block (audit R6 màn 5): App version as a filled
 * disabled input + Data folder value with a copy button — replaces
 * Email / Workspace ID (no cloud account in a local-only app).
 */
function AccountInfoBlock() {
  const { t } = useTranslation();
  const [version, setVersion] = useState(FALLBACK_APP_VERSION);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (isTauri()) {
      getVersion()
        .then(setVersion)
        .catch(() => {});
    }
  }, []);

  // (W59e) Auto-reset the copied indicator after ~2s.
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 2000);
    return () => clearTimeout(timer);
  }, [copied]);

  const handleCopy = () => {
    // (W59e) navigator.clipboard is undefined in insecure contexts —
    // guard it and fall back to the legacy execCommand path.
    if (navigator.clipboard?.writeText) {
      navigator.clipboard
        .writeText(DATA_FOLDER)
        .then(() => setCopied(true))
        .catch(() => {});
      return;
    }
    try {
      const textarea = document.createElement("textarea");
      textarea.value = DATA_FOLDER;
      textarea.style.position = "fixed";
      textarea.style.opacity = "0";
      document.body.appendChild(textarea);
      textarea.select();
      const ok = document.execCommand("copy");
      textarea.remove();
      if (ok) {
        setCopied(true);
      } else {
        console.warn("Clipboard copy is not supported in this context");
      }
    } catch {
      console.warn("Clipboard copy is not supported in this context");
    }
  };

  return (
    <div className="space-y-4">
      <div>
        <p className="text-sm font-medium text-fg">
          {t("settings.appVersion")}
        </p>
        <input
          disabled
          value={version}
          aria-label={t("settings.appVersion")}
          className="input mt-1.5 w-64 cursor-default py-1.5"
        />
      </div>
      <div>
        <p className="text-sm font-medium text-fg">
          {t("settings.dataFolder")}
        </p>
        <div className="mt-1.5 flex items-center gap-1.5">
          <span className="font-mono text-sm text-fg-muted">{DATA_FOLDER}</span>
          <button
            type="button"
            onClick={handleCopy}
            aria-label={copied ? t("settings.copied") : t("settings.copy")}
            title={copied ? t("settings.copied") : t("settings.copy")}
            className="rounded p-1 text-fg-muted hover:bg-surface-2 hover:text-fg"
          >
            {copied ? (
              <Check className="h-4 w-4 text-success" aria-hidden="true" />
            ) : (
              <Copy className="h-4 w-4" aria-hidden="true" />
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Account/Settings screen — Multilogin-style 2-column layout (audit R6 màn 5):
 * account sidebar card (left, multi-item nav) + white content card (right,
 * one page per nav item). Billing items are rendered disabled for visual
 * parity only (cloud billing is n/a in a local-only app).
 */
export function SettingsView() {
  const { t, i18n } = useTranslation();
  const { mode, setMode } = useTheme();
  const [autoClear, setAutoClear] = useState(false);
  const [saveError, setSaveError] = useState(false);
  // (W25a) Encrypted backup/restore dialog of the whole ~/.browserx.
  const [backupDialog, setBackupDialog] = useState<"create" | "restore" | null>(
    null,
  );
  // (W59c) Sidebar routing — local state, Account settings by default.
  const [page, setPage] = useState<SettingsPage>("account");

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
    <div className="flex h-full min-h-0 gap-2 pb-4 pl-2 pr-4 pt-0">
      {/* Account sidebar: item h=36 r6, selected #F0F6FF/#055FF0 14/500,
          hover #F1EDED (audit R6 màn 5) */}
      <nav
        aria-label={t("settings.title")}
        className="card w-[270px] shrink-0 self-start p-3"
      >
        <ul className="space-y-0.5">
          {NAV_ITEMS.map((item) => (
            <li key={item.page}>
              <button
                type="button"
                onClick={() => setPage(item.page)}
                aria-current={page === item.page ? "page" : undefined}
                className={`flex h-9 w-full items-center rounded-md px-3 text-sm ${
                  page === item.page
                    ? "bg-[#F0F6FF] font-medium text-accent"
                    : "text-fg hover:bg-[#F1EDED]"
                }`}
              >
                {t(item.labelKey)}
              </button>
            </li>
          ))}
          {BILLING_ITEMS.map((item) => (
            <li key={item.labelKey}>
              <button
                type="button"
                disabled
                aria-disabled="true"
                className="flex h-9 w-full cursor-not-allowed items-center gap-2 rounded-md px-3 text-sm text-fg opacity-50"
              >
                {t(item.labelKey)}
                {item.beta && (
                  <span className="rounded bg-[#FEEFC7] px-1.5 py-0.5 text-[10px] font-semibold uppercase leading-none text-[#B54708]">
                    {t("settings.nav.beta")}
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
      </nav>

      {/* Content card — one page per nav item */}
      <div className="card min-h-0 flex-1 overflow-y-auto">
        <div className="max-w-2xl space-y-8 p-6">
          {page === "account" && (
            <>
              <AccountInfoBlock />

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

              {saveError && (
                <p className="text-xs text-danger" role="alert">
                  {t("settings.saveFailed")}
                </p>
              )}
            </>
          )}

          {page === "backup" && (
            <>
              <Section title={t("backup.sectionTitle")}>
                <SettingRow
                  label={t("backup.createLabel")}
                  hint={t("backup.createHint")}
                >
                  <button
                    type="button"
                    disabled={!isTauri()}
                    onClick={() => setBackupDialog("create")}
                    className="btn-secondary h-9 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    {t("backup.createButton")}
                  </button>
                </SettingRow>
                <SettingRow
                  label={t("backup.restoreLabel")}
                  hint={t("backup.restoreHint")}
                >
                  <button
                    type="button"
                    disabled={!isTauri()}
                    onClick={() => setBackupDialog("restore")}
                    className="btn-secondary h-9 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    {t("backup.restoreButton")}
                  </button>
                </SettingRow>
              </Section>

              {/* (W52-E1) Recovery Key — export/import master key để khôi phục
                  backup .bxa trên máy mới. */}
              <Section title={t("recoveryKey.sectionTitle")}>
                <RecoveryKeySection />
              </Section>
            </>
          )}

          {/* (W51-B2) Cloud backup (Telegram) — primary destination config. */}
          {page === "cloudSync" && (
            <Section title={t("telegram.sectionTitle")}>
              <TelegramSyncSection />
            </Section>
          )}

          {page === "system" && (
            <>
              <Section title={t("system.title")}>
                <p className="text-xs text-fg-muted">{t("system.hint")}</p>
                <SystemPanel />
              </Section>

              <Section title={t("settings.support")}>
                <SettingRow
                  label={t("settings.logs")}
                  hint={t("settings.logsHint")}
                >
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
            </>
          )}

          {page === "audit" && (
            <Section title={t("audit.title")}>
              <p className="text-xs text-fg-muted">{t("audit.hint")}</p>
              <AuditLog />
            </Section>
          )}
        </div>
      </div>

      {backupDialog && (
        <BackupDialog
          mode={backupDialog}
          onClose={() => setBackupDialog(null)}
        />
      )}
    </div>
  );
}
