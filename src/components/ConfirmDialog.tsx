import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

interface ConfirmDialogProps {
  /** Dialog heading — defaults to the generic t("confirm.title"). */
  title?: string;
  message: string;
  /** Confirm button label — defaults to the generic t("confirm.confirm"). */
  confirmLabel?: string;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * (W47) In-app confirmation dialog replacing window.confirm(), which is a
 * no-op inside the Tauri WKWebView (always returns falsy). Same overlay/card
 * pattern as QuitDialog/QuickStopDialog.
 */
export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  busy = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const { t } = useTranslation();
  const heading = title ?? t("confirm.title");

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={heading}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !busy) onCancel();
      }}
    >
      <div className="card w-full max-w-md p-5">
        <h2 className="text-base font-semibold text-fg">{heading}</h2>
        <p className="mt-2 text-sm text-fg-muted">{message}</p>
        <div className="mt-5 flex flex-wrap justify-end gap-2">
          <button
            type="button"
            className="btn-secondary px-3 py-1.5 text-sm"
            disabled={busy}
            onClick={onCancel}
          >
            {t("confirm.cancel")}
          </button>
          <button
            type="button"
            className="btn-danger inline-flex items-center gap-1.5 px-3 py-1.5 text-sm"
            disabled={busy}
            onClick={onConfirm}
          >
            {busy && (
              <Loader2
                className="h-3.5 w-3.5 animate-spin"
                aria-hidden="true"
              />
            )}
            {confirmLabel ?? t("confirm.confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}
