import { useState } from "react";
import { useTranslation } from "react-i18next";
import { exportRecoveryKey, importRecoveryKey, isTauri } from "../../lib/api";

/**
 * (W52-E1) Settings section "Recovery Key" — export the master key as a
 * one-time recovery string (stored OFFLINE by the user) and import it on a
 * new machine to decrypt cloud .bxa backups. The key is shown exactly once,
 * never logged, never persisted and never sent over the network.
 */
export function RecoveryKeySection() {
  const { t } = useTranslation();
  const [revealed, setRevealed] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [importValue, setImportValue] = useState("");
  const [status, setStatus] = useState<
    | { kind: "idle" }
    | { kind: "busy"; msg: string }
    | { kind: "ok"; msg: string }
    | { kind: "error"; msg: string }
  >({ kind: "idle" });

  const busy = status.kind === "busy";

  const handleExport = async () => {
    try {
      setRevealed(await exportRecoveryKey());
      setCopied(false);
      setStatus({ kind: "idle" });
    } catch (err) {
      setStatus({
        kind: "error",
        msg: err instanceof Error ? err.message : String(err),
      });
    }
  };

  const handleCopy = () => {
    if (!revealed) return;
    navigator.clipboard
      .writeText(revealed)
      .then(() => setCopied(true))
      .catch(() => {});
  };

  const handleHide = () => {
    setRevealed(null);
    setCopied(false);
  };

  const handleImport = async () => {
    setStatus({ kind: "busy", msg: t("recoveryKey.importing") });
    try {
      const result = await importRecoveryKey(importValue.trim());
      setImportValue("");
      setStatus({
        kind: "ok",
        msg: result.changed
          ? t("recoveryKey.importedChanged")
          : t("recoveryKey.imported"),
      });
    } catch (err) {
      setStatus({
        kind: "error",
        msg: err instanceof Error ? err.message : String(err),
      });
    }
  };

  return (
    <div className="space-y-4">
      <p className="text-xs text-fg-muted">{t("recoveryKey.hint")}</p>

      {/* Export — key revealed once, with a strong warning + copy/hide. */}
      {revealed === null ? (
        <button
          type="button"
          disabled={!isTauri() || busy}
          onClick={() => void handleExport()}
          className="btn-secondary h-9 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {t("recoveryKey.exportButton")}
        </button>
      ) : (
        <div className="space-y-2 rounded-md border border-red-300 bg-red-50 p-3">
          <p className="text-xs font-medium text-red-700">
            {t("recoveryKey.exportWarning")}
          </p>
          <p className="select-all break-all rounded border border-border bg-white p-2 font-mono text-xs">
            {revealed}
          </p>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handleCopy}
              className="btn-secondary h-9"
            >
              {copied ? t("recoveryKey.copied") : t("recoveryKey.copyButton")}
            </button>
            <button
              type="button"
              onClick={handleHide}
              className="btn-secondary h-9"
            >
              {t("recoveryKey.hideButton")}
            </button>
          </div>
        </div>
      )}

      {/* Import — validate + install the key on a new machine. */}
      <div className="space-y-1.5">
        <label
          className="text-xs font-medium text-fg-muted"
          htmlFor="recovery-key-import"
        >
          {t("recoveryKey.importLabel")}
        </label>
        <div className="flex items-center gap-2">
          <input
            id="recovery-key-import"
            type="text"
            autoComplete="off"
            spellCheck={false}
            className="input h-9 flex-1 font-mono text-xs"
            placeholder={t("recoveryKey.importPlaceholder")}
            value={importValue}
            onChange={(e) => setImportValue(e.target.value)}
          />
          <button
            type="button"
            disabled={!isTauri() || busy || importValue.trim().length === 0}
            onClick={() => void handleImport()}
            className="btn-secondary h-9 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {t("recoveryKey.importButton")}
          </button>
        </div>
        <p className="text-xs text-fg-muted">{t("recoveryKey.importHint")}</p>
      </div>

      {status.kind !== "idle" && (
        <p
          className={`text-xs ${
            status.kind === "error"
              ? "text-red-600"
              : status.kind === "ok"
                ? "text-green-600"
                : "text-fg-muted"
          }`}
        >
          {status.msg}
        </p>
      )}
    </div>
  );
}
