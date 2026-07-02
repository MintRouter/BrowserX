import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

interface QuitDialogProps {
  /** Number of sessions still running when quit was requested. */
  count: number;
  busy: boolean;
  onStopAllQuit: () => void;
  onCancel: () => void;
}

/**
 * (W23a) Confirmation shown when the user tries to quit while sessions are
 * still running: Stop all & quit (full per-profile cleanup, then exit) / Cancel.
 */
export function QuitDialog({
  count,
  busy,
  onStopAllQuit,
  onCancel,
}: QuitDialogProps) {
  const { t } = useTranslation();

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t("quit.title")}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !busy) onCancel();
      }}
    >
      <div className="card w-full max-w-md p-5">
        <h2 className="text-base font-semibold text-fg">{t("quit.title")}</h2>
        <p className="mt-2 text-sm text-fg-muted">
          {t("quit.message", { count })}
        </p>
        <div className="mt-5 flex flex-wrap justify-end gap-2">
          <button
            type="button"
            className="btn-secondary px-3 py-1.5 text-sm"
            disabled={busy}
            onClick={onCancel}
          >
            {t("quit.cancel")}
          </button>
          <button
            type="button"
            className="btn-danger inline-flex items-center gap-1.5 px-3 py-1.5 text-sm"
            disabled={busy}
            onClick={onStopAllQuit}
          >
            {busy && (
              <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
            )}
            {t("quit.stopAllQuit")}
          </button>
        </div>
      </div>
    </div>
  );
}
