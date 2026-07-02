import { RotateCcw, Trash2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { Profile } from "../lib/api";

interface TrashViewProps {
  items: Profile[];
  onRestore: (ids: string[]) => Promise<void>;
  onPurge: (ids: string[]) => Promise<void>;
}

export function TrashView({ items, onRestore, onPurge }: TrashViewProps) {
  const { t, i18n } = useTranslation();
  const [busyId, setBusyId] = useState<string | null>(null);

  const deletedAt = (p: Profile) =>
    (p as unknown as { deleted_at?: string }).deleted_at ?? p.updated_at;

  const fmtDate = (iso: string) => {
    const d = new Date(iso);
    return isNaN(d.getTime())
      ? iso
      : new Intl.DateTimeFormat(i18n.language, {
          dateStyle: "short",
          timeStyle: "short",
        }).format(d);
  };

  const run = async (id: string, fn: () => Promise<void>) => {
    setBusyId(id);
    try {
      await fn();
    } catch (err) {
      console.error("Trash action failed:", err);
    } finally {
      setBusyId(null);
    }
  };

  const handlePurge = (p: Profile) => {
    if (!confirm(t("trash.confirmPurge", { name: p.name }))) return;
    void run(p.id, () => onPurge([p.id]));
  };

  return (
    <div className="flex h-full flex-col gap-3 p-3">
      <div className="card flex min-h-0 flex-1 flex-col overflow-hidden">
        <h2 className="border-b border-border px-4 py-3 text-base font-semibold text-fg">
          {t("trash.title")}
        </h2>
        {items.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 p-12 text-center">
            <Trash2 className="h-8 w-8 text-fg-muted/50" aria-hidden="true" />
            <p className="text-sm text-fg-muted">{t("trash.empty")}</p>
          </div>
        ) : (
          <div className="flex-1 overflow-auto">
            <table className="w-full text-sm">
              <thead className="sticky top-0 border-b border-border bg-surface-1">
                <tr>
                  <th scope="col" className="px-4 py-2.5 text-left text-xs font-medium uppercase tracking-wider text-fg-muted">
                    {t("table.profileName")}
                  </th>
                  <th scope="col" className="px-4 py-2.5 text-left text-xs font-medium uppercase tracking-wider text-fg-muted">
                    {t("trash.deletedAt")}
                  </th>
                  <th scope="col" className="px-4 py-2.5 text-right text-xs font-medium uppercase tracking-wider text-fg-muted">
                    {t("table.actions")}
                  </th>
                </tr>
              </thead>
              <tbody>
                {items.map((p) => (
                  <tr key={p.id} className="border-b border-border hover:bg-accent/[0.03]">
                    <td className="px-4 py-2 font-medium text-fg">{p.name}</td>
                    <td className="px-4 py-2 text-fg-muted">{fmtDate(deletedAt(p))}</td>
                    <td className="px-4 py-2">
                      <span className="flex items-center justify-end gap-2">
                        <button
                          type="button"
                          disabled={busyId === p.id}
                          onClick={() => void run(p.id, () => onRestore([p.id]))}
                          className="btn-secondary px-2.5 py-1 text-xs"
                        >
                          <RotateCcw className="h-3.5 w-3.5" aria-hidden="true" />
                          <span>{t("trash.restore")}</span>
                        </button>
                        <button
                          type="button"
                          disabled={busyId === p.id}
                          onClick={() => handlePurge(p)}
                          className="btn-danger px-2.5 py-1 text-xs"
                        >
                          <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
                          <span>{t("trash.deleteForever")}</span>
                        </button>
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
