import {
  ChevronDown,
  ChevronUp,
  Cloud,
  History,
  Search,
  SearchX,
  Star,
  Trash2,
} from "lucide-react";
import { type ReactNode, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Folder, Profile } from "../lib/api";
import { ConfirmDialog } from "./ConfirmDialog";
import { TableFooter } from "./TableFooter";

interface TrashViewProps {
  items: Profile[];
  folders: Folder[];
  /** Non-trashed profile count — feeds the footer Info pill. */
  profileCount: number;
  settings: Record<string, string> | null;
  onRestore: (ids: string[]) => Promise<void>;
  onPurge: (ids: string[]) => Promise<void>;
}

function ToolButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick?: () => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      disabled={disabled}
      className="inline-flex h-[30px] w-[30px] shrink-0 items-center justify-center rounded-md text-[#1D192B] transition-colors hover:bg-surface-2 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent"
    >
      {children}
    </button>
  );
}

/** (W50E) Trash rebuilt on the standard card shell: toolbar + table + footer (MLX parity). */
export function TrashView({
  items,
  folders,
  profileCount,
  settings,
  onRestore,
  onPurge,
}: TrashViewProps) {
  const { t, i18n } = useTranslation();
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(100);
  /** Deleted column sort direction — MLX defaults to newest first. */
  const [sortDir, setSortDir] = useState<"asc" | "desc">("desc");
  const [busy, setBusy] = useState(false);
  /** (W47) Ids awaiting the purge confirmation; "all" = Empty trash bin. */
  const [purgeConfirm, setPurgeConfirm] = useState<string[] | "all" | null>(
    null,
  );

  const deletedAt = (p: Profile) =>
    (p as unknown as { deleted_at?: string }).deleted_at ?? p.updated_at;

  const fmtDate = (iso: string) => {
    const d = new Date(iso);
    return isNaN(d.getTime())
      ? iso
      : new Intl.DateTimeFormat(i18n.language, { dateStyle: "short" }).format(d);
  };

  const folderName = (id: string | null) =>
    id ? (folders.find((f) => f.id === id)?.name ?? "—") : "—";

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    const list = q
      ? items.filter((p) => p.name.toLowerCase().includes(q))
      : [...items];
    list.sort((a, b) => {
      const cmp = deletedAt(a).localeCompare(deletedAt(b));
      return sortDir === "asc" ? cmp : -cmp;
    });
    return list;
  }, [items, search, sortDir]);

  useEffect(() => {
    setPage(0);
  }, [search, rowsPerPage, items.length]);

  // Drop selected ids that no longer exist (e.g. after restore/purge).
  useEffect(() => {
    setSelected((prev) => {
      const next = new Set(
        [...prev].filter((id) => items.some((p) => p.id === id)),
      );
      return next.size === prev.size ? prev : next;
    });
  }, [items]);

  const totalPages = Math.max(1, Math.ceil(filtered.length / rowsPerPage));
  const safePage = Math.min(page, totalPages - 1);
  const paged = filtered.slice(
    safePage * rowsPerPage,
    (safePage + 1) * rowsPerPage,
  );
  const pageIds = paged.map((p) => p.id);
  const allChecked = paged.length > 0 && paged.every((p) => selected.has(p.id));
  const someChecked = paged.some((p) => selected.has(p.id));

  const toggleRow = (id: string) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setSelected(next);
  };

  const togglePage = (select: boolean) => {
    const next = new Set(selected);
    for (const id of pageIds) {
      if (select) next.add(id);
      else next.delete(id);
    }
    setSelected(next);
  };

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
    } catch (err) {
      console.error("Trash action failed:", err);
    } finally {
      setBusy(false);
    }
  };

  const handleRestore = (ids: string[]) => {
    if (ids.length === 0) return;
    void run(() => onRestore(ids));
  };

  const confirmPurge = () => {
    const target = purgeConfirm;
    setPurgeConfirm(null);
    if (!target) return;
    const ids = target === "all" ? items.map((p) => p.id) : target;
    if (ids.length === 0) return;
    void run(() => onPurge(ids));
  };

  const purgeMessage = () => {
    if (purgeConfirm === "all")
      return t("trash.confirmEmpty", { count: items.length });
    if (purgeConfirm?.length === 1) {
      const name =
        items.find((p) => p.id === purgeConfirm[0])?.name ?? purgeConfirm[0];
      return t("trash.confirmPurge", { name });
    }
    return t("trash.confirmPurgeMany", { count: purgeConfirm?.length ?? 0 });
  };

  const th = "h-10 px-3 text-left align-middle text-xs font-medium text-fg";

  return (
    <div className="flex h-full flex-col p-4">
      {/* Toolbar lives inside the table's white card (standard shell, F1a) */}
      <div className="card flex min-h-0 flex-1 flex-col overflow-hidden">
        <div className="flex min-h-[60px] flex-wrap items-center gap-3 p-3">
          {/* MLX: "Empty trash bin" accent-outline pill, always visible */}
          <button
            type="button"
            onClick={() => setPurgeConfirm("all")}
            disabled={busy || items.length === 0}
            className="inline-flex h-9 shrink-0 items-center gap-1.5 rounded-full border border-accent px-3.5 text-sm font-medium text-accent transition-colors hover:bg-accent/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent"
          >
            <Trash2 className="h-4 w-4" aria-hidden="true" />
            <span>{t("trash.emptyTrashBin")}</span>
          </button>
          <ToolButton
            label={t("trash.restore")}
            disabled={busy || selected.size === 0}
            onClick={() => handleRestore([...selected])}
          >
            <History className="h-4 w-4" aria-hidden="true" />
          </ToolButton>
          <ToolButton
            label={t("trash.deleteForever")}
            disabled={busy || selected.size === 0}
            onClick={() => setPurgeConfirm([...selected])}
          >
            <Trash2 className="h-4 w-4" aria-hidden="true" />
          </ToolButton>

          <div className="relative ml-auto w-[225px]">
            <Search
              className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-muted"
              aria-hidden="true"
            />
            <input
              type="search"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("trash.searchPlaceholder")}
              aria-label={t("trash.searchPlaceholder")}
              className="h-9 w-full rounded-md border border-border bg-surface-2 pl-9 pr-3 text-sm text-fg placeholder:text-fg-muted focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/50"
            />
          </div>
        </div>

        {items.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 p-12 text-center">
            <Trash2 className="h-8 w-8 text-fg-muted/50" aria-hidden="true" />
            <p className="text-sm text-fg-muted">{t("trash.empty")}</p>
          </div>
        ) : filtered.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 p-12 text-center">
            <SearchX className="h-8 w-8 text-fg-muted/50" aria-hidden="true" />
            <p className="text-sm text-fg-muted">{t("table.noMatches")}</p>
          </div>
        ) : (
          <div className="flex-1 overflow-auto">
            <table className="w-full text-sm">
              <thead className="sticky top-0 z-10 border-b border-border bg-surface-2">
                <tr className="h-10 border-b border-border">
                  <th scope="col" className="w-10 px-3 align-middle">
                    <input
                      type="checkbox"
                      aria-label={t("table.selectAll")}
                      checked={allChecked}
                      ref={(el) => {
                        if (el) el.indeterminate = someChecked && !allChecked;
                      }}
                      onChange={() => togglePage(!allChecked)}
                      className="h-4 w-4 cursor-pointer rounded border-border accent-accent"
                    />
                  </th>
                  <th scope="col" className="w-10 px-1 align-middle">
                    <span className="sr-only">{t("table.favorite")}</span>
                  </th>
                  <th scope="col" className={th}>{t("table.profileName")}</th>
                  <th scope="col" className={th}>{t("table.storage")}</th>
                  <th scope="col" className={th}>{t("table.folder")}</th>
                  <th scope="col" className={th}>
                    <button
                      type="button"
                      onClick={() =>
                        setSortDir(sortDir === "desc" ? "asc" : "desc")
                      }
                      className="inline-flex items-center gap-1 rounded hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                      aria-sort={sortDir === "asc" ? "ascending" : "descending"}
                    >
                      {t("trash.deletedAt")}
                      {sortDir === "asc" ? (
                        <ChevronUp className="h-3.5 w-3.5" aria-hidden="true" />
                      ) : (
                        <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
                      )}
                    </button>
                  </th>
                  <th scope="col" className={th}>{t("trash.created")}</th>
                  <th scope="col" className="w-24 px-3 text-right align-middle">
                    <span className="sr-only">{t("table.actions")}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {paged.map((p) => {
                  const isSelected = selected.has(p.id);
                  return (
                    <tr
                      key={p.id}
                      className={`h-[49px] border-b border-border transition-colors [&>td]:align-middle ${
                        isSelected ? "bg-[#F0F6FF]" : "hover:bg-accent/[0.03]"
                      }`}
                    >
                      <td className="px-3 py-2">
                        <input
                          type="checkbox"
                          aria-label={`${t("table.selectRow")}: ${p.name}`}
                          checked={isSelected}
                          onChange={() => toggleRow(p.id)}
                          className="h-4 w-4 cursor-pointer rounded border-border accent-accent"
                        />
                      </td>
                      <td className="px-1 py-2">
                        <span className="grid h-8 w-8 place-items-center text-fg-muted">
                          <Star
                            className={`h-4 w-4 ${p.favorite ? "fill-[#F5A623] text-[#F5A623]" : ""}`}
                            aria-hidden="true"
                          />
                        </span>
                      </td>
                      <td className="max-w-0 px-3 py-2">
                        <span className="block truncate font-medium text-fg" title={p.name}>
                          {p.name}
                        </span>
                      </td>
                      <td className="whitespace-nowrap px-3 py-2 text-fg-muted">
                        <span
                          className="inline-flex items-center gap-1.5"
                          title={t("table.local")}
                        >
                          <Cloud className="h-4 w-4" aria-hidden="true" />
                          {t("table.local")}
                        </span>
                      </td>
                      <td className="max-w-0 truncate px-3 py-2 text-fg-muted">
                        {folderName(p.folder_id)}
                      </td>
                      <td
                        className="whitespace-nowrap px-3 py-2 text-fg-muted"
                        title={new Date(deletedAt(p)).toLocaleString(i18n.language)}
                      >
                        {fmtDate(deletedAt(p))}
                      </td>
                      <td className="whitespace-nowrap px-3 py-2 text-fg-muted">
                        {fmtDate(p.created_at)}
                      </td>
                      <td className="whitespace-nowrap px-3 py-2 text-right">
                        {/* MLX: always-visible inline Restore text button per row */}
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => handleRestore([p.id])}
                          className="rounded text-sm font-medium text-accent hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-50"
                        >
                          {t("trash.restore")}
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}

        <TableFooter
          total={filtered.length}
          page={safePage}
          rowsPerPage={rowsPerPage}
          onPageChange={setPage}
          onRowsPerPageChange={setRowsPerPage}
          profileCount={profileCount}
          settings={settings}
        />
      </div>

      {purgeConfirm && (
        <ConfirmDialog
          message={purgeMessage()}
          confirmLabel={t("trash.deleteForever")}
          onConfirm={confirmPurge}
          onCancel={() => setPurgeConfirm(null)}
        />
      )}
    </div>
  );
}
