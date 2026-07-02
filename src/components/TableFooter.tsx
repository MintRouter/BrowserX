import { ChevronLeft, ChevronRight, Info } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Popover } from "./Popover";

export const ROWS_PER_PAGE_OPTIONS = [25, 50, 100] as const;

interface TableFooterProps {
  total: number;
  page: number;
  rowsPerPage: number;
  onPageChange: (page: number) => void;
  onRowsPerPageChange: (rows: number) => void;
  profileCount: number;
  settings: Record<string, string> | null;
}

export function TableFooter({
  total,
  page,
  rowsPerPage,
  onPageChange,
  onRowsPerPageChange,
  profileCount,
  settings,
}: TableFooterProps) {
  const { t } = useTranslation();
  const [infoOpen, setInfoOpen] = useState(false);

  const totalPages = Math.max(1, Math.ceil(total / rowsPerPage));
  const from = total === 0 ? 0 : page * rowsPerPage + 1;
  const to = Math.min(total, (page + 1) * rowsPerPage);

  const version =
    settings?.app_version ?? settings?.version ?? "0.1.0";
  const cap =
    settings?.max_concurrent_profiles ??
    settings?.max_concurrent ??
    settings?.concurrent_cap ??
    null;

  const navBtn =
    "grid h-7 w-7 place-items-center rounded-lg text-fg-muted transition-colors hover:bg-surface-2 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent disabled:hover:text-fg-muted";

  return (
    <div className="flex items-center justify-between gap-3 border-t border-border px-3 py-2 text-sm text-fg">
      <Popover
        open={infoOpen}
        onClose={() => setInfoOpen(false)}
        label={t("footer.info")}
        panelClassName="bottom-full top-auto mb-1 mt-0"
        trigger={
          <button
            type="button"
            aria-label={t("footer.info")}
            aria-haspopup="dialog"
            aria-expanded={infoOpen}
            onClick={() => setInfoOpen((v) => !v)}
            className="inline-flex items-center gap-1.5 rounded-md bg-[#F0F6FF] px-2.5 py-1.5 text-xs font-medium text-accent transition-colors hover:bg-[#E0EDFF] focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
          >
            <Info className="h-3.5 w-3.5" aria-hidden="true" />
            <span>{t("footer.info")}</span>
          </button>
        }
      >
        <dl className="w-52 space-y-1.5 p-2 text-xs">
          <div className="flex justify-between gap-3">
            <dt className="text-fg-muted">{t("footer.version")}</dt>
            <dd className="font-medium text-fg">{version}</dd>
          </div>
          <div className="flex justify-between gap-3">
            <dt className="text-fg-muted">{t("footer.profiles")}</dt>
            <dd className="font-medium text-fg tabular-nums">{profileCount}</dd>
          </div>
          {cap !== null && (
            <div className="flex justify-between gap-3">
              <dt className="text-fg-muted">{t("footer.concurrentCap")}</dt>
              <dd className="font-medium text-fg tabular-nums">{cap}</dd>
            </div>
          )}
        </dl>
      </Popover>

      <div className="flex items-center gap-4">
        <label className="flex items-center gap-2">
          <span>{t("footer.rowsPerPage")}</span>
          <select
            value={rowsPerPage}
            onChange={(e) => onRowsPerPageChange(Number(e.target.value))}
            className="rounded-md border border-border bg-surface-1 px-1.5 py-1 text-sm text-fg focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/50"
          >
            {ROWS_PER_PAGE_OPTIONS.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </label>
        <span className="tabular-nums" aria-live="polite">
          {t("footer.range", { from, to, total })}
        </span>
        <div className="flex items-center gap-1">
          <button
            type="button"
            aria-label={t("footer.prevPage")}
            disabled={page === 0}
            onClick={() => onPageChange(page - 1)}
            className={navBtn}
          >
            <ChevronLeft className="h-4 w-4" aria-hidden="true" />
          </button>
          <button
            type="button"
            aria-label={t("footer.nextPage")}
            disabled={page >= totalPages - 1}
            onClick={() => onPageChange(page + 1)}
            className={navBtn}
          >
            <ChevronRight className="h-4 w-4" aria-hidden="true" />
          </button>
        </div>
      </div>
    </div>
  );
}
