import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  api,
  isTauri,
  TELEGRAM_SYNC_ENABLED_SETTING,
  type CloudBackupInfo,
  type CloudTransport,
} from "../../lib/api";
import { Segmented, Toggle } from "../profile-form/controls";
import { UserbotPanel } from "./UserbotPanel";

/** Human-readable size for the "last backup" summary line. */
function fmtBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * (W51-B2) Settings section "Cloud backup (Telegram)" — primary-destination
 * style config: Bot Token (password field) + Chat ID, Test connection, enable
 * toggle, last backup summary. Credentials are stored encrypted at rest and
 * never read back into the form (placeholder indicates they are saved).
 */
export function TelegramSyncSection() {
  const { t, i18n } = useTranslation();
  const [enabled, setEnabled] = useState(false);
  const [configured, setConfigured] = useState(false);
  const [token, setToken] = useState("");
  const [chatId, setChatId] = useState("");
  const [status, setStatus] = useState<
    | { kind: "idle" }
    | { kind: "busy"; msg: string }
    | { kind: "ok"; msg: string }
    | { kind: "error"; msg: string }
  >({ kind: "idle" });
  const [lastBackup, setLastBackup] = useState<CloudBackupInfo | null>(null);
  // (W55b-UI) Selected transport. "userbot" only becomes active on the backend
  // once the userbot auth state is "ready" (the panel activates it then).
  const [transport, setTransport] = useState<CloudTransport>("bot_api");

  useEffect(() => {
    if (!isTauri()) return;
    api
      .getSettings()
      .then((s) => setEnabled(s[TELEGRAM_SYNC_ENABLED_SETTING] === "true"))
      .catch(() => {});
    api
      .telegramCredentialsStatus()
      .then(setConfigured)
      .catch(() => {});
    api
      .listCloudBackups()
      .then((list) => setLastBackup(list[0] ?? null))
      .catch(() => {});
    api
      .cloudGetTransport()
      .then(setTransport)
      .catch(() => {});
  }, []);

  const handleTransport = (next: CloudTransport) => {
    const prev = transport;
    setTransport(next);
    setStatus({ kind: "idle" });
    // Switching back to Bot API is always allowed; "userbot" is activated by
    // the panel itself once (and only when) the auth state reaches "ready".
    if (next === "bot_api") {
      api.cloudSetTransport("bot_api").catch((err) => {
        setTransport(prev);
        setStatus({
          kind: "error",
          msg: err instanceof Error ? err.message : String(err),
        });
      });
    }
  };

  const handleToggle = (next: boolean) => {
    setEnabled(next);
    api.setSetting(TELEGRAM_SYNC_ENABLED_SETTING, String(next)).catch(() => {
      setEnabled(!next);
      setStatus({ kind: "error", msg: t("settings.saveFailed") });
    });
  };

  const handleSave = async () => {
    setStatus({ kind: "busy", msg: t("telegram.saving") });
    try {
      await api.telegramSetCredentials(token.trim(), chatId.trim());
      const ok = await api.telegramCredentialsStatus();
      setConfigured(ok);
      setToken("");
      setChatId("");
      setStatus({ kind: "ok", msg: t("telegram.saved") });
    } catch (err) {
      setStatus({
        kind: "error",
        msg: err instanceof Error ? err.message : String(err),
      });
    }
  };

  const handleTest = async () => {
    setStatus({ kind: "busy", msg: t("telegram.testing") });
    try {
      const username = await api.telegramTestConnection();
      setStatus({
        kind: "ok",
        msg: t("telegram.testOk", { bot: username || "bot" }),
      });
    } catch (err) {
      setStatus({
        kind: "error",
        msg: err instanceof Error ? err.message : String(err),
      });
    }
  };

  const fmtDate = (iso: string) => {
    const d = new Date(iso);
    return isNaN(d.getTime())
      ? iso
      : new Intl.DateTimeFormat(i18n.language, {
          dateStyle: "short",
          timeStyle: "short",
        }).format(d);
  };

  const busy = status.kind === "busy";
  const canSave = token.trim() !== "" && chatId.trim() !== "";

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <p className="text-sm font-medium text-fg">
            {t("telegram.enableLabel")}
          </p>
          <p className="mt-0.5 text-xs text-fg-muted">
            {t("telegram.enableHint")}
          </p>
        </div>
        <div className="shrink-0">
          <Toggle
            checked={enabled}
            onChange={handleToggle}
            label={t("telegram.enableLabel")}
          />
        </div>
      </div>

      {/* (W55b-UI) Transport: Bot API (default) | Userbot (MTProto). */}
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <p className="text-sm font-medium text-fg">
            {t("telegram.transportLabel")}
          </p>
          <p className="mt-0.5 text-xs text-fg-muted">
            {t("telegram.transportHint")}
          </p>
        </div>
        <div className="shrink-0">
          <Segmented
            options={[
              { value: "bot_api", label: t("telegram.transportBotApi") },
              { value: "userbot", label: t("telegram.transportUserbot") },
            ]}
            value={transport}
            onChange={handleTransport}
            label={t("telegram.transportLabel")}
          />
        </div>
      </div>

      {transport === "userbot" ? (
        <>
          <p className="text-xs text-warning">{t("userbot.warning")}</p>
          <UserbotPanel />
        </>
      ) : (
        <>
          <div className="space-y-3">
            <label className="block">
              <span className="text-sm font-medium text-fg">
                {t("telegram.botToken")}
              </span>
              <input
                type="password"
                value={token}
                onChange={(e) => setToken(e.target.value)}
                autoComplete="off"
                placeholder={
                  configured
                    ? t("telegram.tokenSavedPlaceholder")
                    : "123456:ABC-…"
                }
                className="input mt-1 w-full py-1.5 text-sm"
              />
            </label>
            <label className="block">
              <span className="text-sm font-medium text-fg">
                {t("telegram.chatId")}
              </span>
              <input
                type="text"
                value={chatId}
                onChange={(e) => setChatId(e.target.value)}
                autoComplete="off"
                placeholder={
                  configured
                    ? t("telegram.tokenSavedPlaceholder")
                    : "-1001234567890"
                }
                className="input mt-1 w-full py-1.5 text-sm"
              />
            </label>
            <p className="text-xs text-fg-muted">
              {t("telegram.credentialsHint")}
            </p>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              disabled={!isTauri() || busy || !canSave}
              onClick={() => void handleSave()}
              className="btn-secondary h-9 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {t("telegram.saveButton")}
            </button>
            <button
              type="button"
              disabled={!isTauri() || busy || !configured}
              onClick={() => void handleTest()}
              className="btn-secondary h-9 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {t("telegram.testButton")}
            </button>
          </div>
        </>
      )}

      {status.kind !== "idle" && (
        <p
          role={status.kind === "error" ? "alert" : "status"}
          className={`text-xs ${
            status.kind === "error"
              ? "text-danger"
              : status.kind === "ok"
                ? "text-accent"
                : "text-fg-muted"
          }`}
        >
          {status.msg}
        </p>
      )}

      <p className="text-xs text-fg-muted">
        {lastBackup
          ? t("telegram.lastBackup", {
              date: fmtDate(lastBackup.uploaded_at),
              size: fmtBytes(lastBackup.size),
            })
          : t("telegram.noBackups")}
      </p>
    </div>
  );
}
