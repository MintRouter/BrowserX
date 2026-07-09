import { Loader2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, onBackupProgress, type BackupProgressEvent } from "../../lib/api";

interface BackupDialogProps {
  /** "create" encrypts ~/.browserx into a file; "restore" replaces it from one. */
  mode: "create" | "restore";
  onClose: () => void;
}

/** Success payload — create keeps the file path, restore the pre-restore dir. */
interface DoneState {
  path?: string;
  previousDataDir?: string | null;
}

/**
 * Backup/Restore dialog (W25a) — passphrase-encrypted (Argon2id + AES-256-GCM)
 * backup of the whole ~/.browserx. Restore requires an app restart afterwards.
 */
export function BackupDialog({ mode, onClose }: BackupDialogProps) {
  const { t } = useTranslation();
  const [passphrase, setPassphrase] = useState("");
  const [confirm, setConfirm] = useState("");
  const [filePath, setFilePath] = useState("");
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<BackupProgressEvent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<DoneState | null>(null);
  const busyRef = useRef(false);

  // Progress events only flow while a backup/restore command runs.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onBackupProgress((e) => {
      if (busyRef.current) setProgress(e);
    }).then((f) => {
      unlisten = f;
    });
    return () => unlisten?.();
  }, []);

  const mismatch =
    mode === "create" && confirm.length > 0 && confirm !== passphrase;
  const canSubmit =
    passphrase.length > 0 &&
    (mode === "create" ? passphrase === confirm : filePath.trim().length > 0);

  const handleSubmit = async () => {
    if (!canSubmit || busy) return;
    setBusy(true);
    busyRef.current = true;
    setError(null);
    setProgress(null);
    try {
      if (mode === "create") {
        const res = await api.createBackup(passphrase);
        setDone({ path: res.path });
      } else {
        const res = await api.restoreBackup(filePath.trim(), passphrase);
        setDone({ previousDataDir: res.previousDataDir });
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
      busyRef.current = false;
    }
  };

  const title =
    mode === "create" ? t("backup.createTitle") : t("backup.restoreTitle");

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !busy) onClose();
      }}
    >
      <div className="card w-full max-w-md p-5">
        <h2 className="text-base font-semibold text-fg">{title}</h2>
        <p className="mt-1.5 text-xs text-fg-muted">
          {mode === "create"
            ? t("backup.createWarning")
            : t("backup.restoreWarning")}
        </p>

        {done ? (
          <DoneView mode={mode} done={done} onClose={onClose} />
        ) : (
          <>
            <div className="mt-4 space-y-3">
              {mode === "restore" && (
                <label className="block">
                  <span className="label">{t("backup.filePath")}</span>
                  <input
                    type="text"
                    value={filePath}
                    onChange={(e) => setFilePath(e.target.value)}
                    placeholder={t("backup.filePathPlaceholder")}
                    disabled={busy}
                    className="input h-11 w-full"
                  />
                  <span className="mt-1 block text-xs text-fg-muted">
                    {t("backup.filePathHint")}
                  </span>
                </label>
              )}
              <label className="block">
                <span className="label">{t("backup.passphrase")}</span>
                <input
                  type="password"
                  value={passphrase}
                  onChange={(e) => setPassphrase(e.target.value)}
                  placeholder={t("backup.passphrasePlaceholder")}
                  disabled={busy}
                  autoFocus={mode === "create"}
                  className="input h-11 w-full"
                />
              </label>
              {mode === "create" && (
                <label className="block">
                  <span className="label">{t("backup.confirmPassphrase")}</span>
                  <input
                    type="password"
                    value={confirm}
                    onChange={(e) => setConfirm(e.target.value)}
                    disabled={busy}
                    className="input h-11 w-full"
                  />
                  {mismatch && (
                    <span
                      className="mt-1 block text-xs text-danger"
                      role="alert"
                    >
                      {t("backup.passphraseMismatch")}
                    </span>
                  )}
                </label>
              )}
            </div>

            {busy && (
              <div className="mt-4">
                <div className="flex items-center justify-between text-xs text-fg-muted">
                  <span>
                    {t(`backup.phase.${progress?.phase ?? "start"}`, {
                      defaultValue: progress?.phase ?? "…",
                    })}
                  </span>
                  <span>{progress?.pct ?? 0}%</span>
                </div>
                <div
                  className="mt-1 h-1.5 overflow-hidden rounded-full bg-surface-2"
                  role="progressbar"
                  aria-valuenow={progress?.pct ?? 0}
                  aria-valuemin={0}
                  aria-valuemax={100}
                >
                  <div
                    className="h-full rounded-full bg-accent transition-all"
                    style={{ width: `${progress?.pct ?? 0}%` }}
                  />
                </div>
              </div>
            )}

            {error && (
              <p className="mt-3 text-xs text-danger" role="alert">
                {error}
              </p>
            )}

            <div className="mt-5 flex justify-end gap-2">
              <button
                type="button"
                onClick={onClose}
                disabled={busy}
                className="btn-secondary h-9 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {t("backup.cancel")}
              </button>
              <button
                type="button"
                onClick={() => void handleSubmit()}
                disabled={!canSubmit || busy}
                className="btn-primary h-9 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {busy && (
                  <Loader2
                    className="h-4 w-4 animate-spin"
                    aria-hidden="true"
                  />
                )}
                {mode === "create"
                  ? t("backup.createButton")
                  : t("backup.restoreButton")}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

/** Success screen — create shows the file path, restore requires a restart. */
function DoneView({
  mode,
  done,
  onClose,
}: {
  mode: "create" | "restore";
  done: DoneState;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="mt-4">
      {mode === "create" ? (
        <>
          <p className="text-sm text-fg">{t("backup.createDone")}</p>
          <p className="mt-1 break-all text-xs text-fg-muted">{done.path}</p>
          <div className="mt-5 flex justify-end">
            <button type="button" onClick={onClose} className="btn-primary h-9">
              {t("backup.close")}
            </button>
          </div>
        </>
      ) : (
        <>
          <p className="text-sm text-fg">{t("backup.restoreDone")}</p>
          {done.previousDataDir && (
            <p className="mt-1 break-all text-xs text-fg-muted">
              {t("backup.restoredKept", { path: done.previousDataDir })}
            </p>
          )}
          <div className="mt-5 flex justify-end gap-2">
            <button
              type="button"
              onClick={onClose}
              className="btn-secondary h-9"
            >
              {t("backup.close")}
            </button>
            <button
              type="button"
              onClick={() => void api.restartApp().catch(() => {})}
              className="btn-primary h-9"
            >
              {t("backup.restartNow")}
            </button>
          </div>
        </>
      )}
    </div>
  );
}
