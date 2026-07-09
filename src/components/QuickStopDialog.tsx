import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";

interface QuickStopDialogProps {
  /** Names of the quick profiles being stopped. */
  names: string[];
  busy: boolean;
  onSaveAsRegular: () => void;
  onCloseDelete: () => void;
  onCancel: () => void;
}

/**
 * Confirmation shown when stopping a quick (use-and-discard) profile:
 * Close & delete (purge data) / Save as regular (keep data) / Cancel.
 */
export function QuickStopDialog({
  names,
  busy,
  onSaveAsRegular,
  onCloseDelete,
  onCancel,
}: QuickStopDialogProps) {
  const { t } = useTranslation();
  const message =
    names.length === 1
      ? t("quickStop.message", { name: names[0] })
      : t("quickStop.messageMany", { count: names.length });

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t("quickStop.title")}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !busy) onCancel();
      }}
    >
      <div className="card w-full max-w-md p-5">
        <h2 className="text-base font-semibold text-fg">
          {t("quickStop.title")}
        </h2>
        <p className="mt-2 text-sm text-fg-muted">{message}</p>
        <div className="mt-5 flex flex-wrap justify-end gap-2">
          <button
            type="button"
            className="btn-secondary px-3 py-1.5 text-sm"
            disabled={busy}
            onClick={onCancel}
          >
            {t("quickStop.cancel")}
          </button>
          <button
            type="button"
            className="btn-secondary px-3 py-1.5 text-sm"
            disabled={busy}
            onClick={onSaveAsRegular}
          >
            {t("quickStop.saveAsRegular")}
          </button>
          <button
            type="button"
            className="btn-danger inline-flex items-center gap-1.5 px-3 py-1.5 text-sm"
            disabled={busy}
            onClick={onCloseDelete}
          >
            {busy && (
              <Loader2
                className="h-3.5 w-3.5 animate-spin"
                aria-hidden="true"
              />
            )}
            {t("quickStop.closeDelete")}
          </button>
        </div>
      </div>
    </div>
  );
}
