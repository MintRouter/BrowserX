import { Cloud, CloudDownload, Search, SearchX, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, isTauri, type CloudBackupInfo, type Profile } from "../lib/api";
import { ConfirmDialog } from "./ConfirmDialog";
import { TableFooter } from "./TableFooter";

interface CloudSyncViewProps {
  profiles: Profile[];
  runningIds: Set<string>;
  /** Non-trashed profile count — feeds the footer Info pill. */
  profileCount: number;
  settings: Record<string, string> | null;
}

/** Human-readable size (backups range from KB to multi-part 100MB+). */
function fmtBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/**
 * (W51-B2) Cloud sync profiles — profiles that have a Telegram cloud backup
 * (`cloud_backups` records). Standard card shell like Running/Trash (W50E):
 * toolbar + table + footer. Row actions: Restore from cloud / Delete backup.
 */
export function CloudSyncView({
  profiles,
  runningIds,
  profileCount,
  settings,
}: CloudSyncViewProps) {
  const { t, i18n } = useTranslation();
  const [backups, setBackups] = useState<CloudBackupInfo[]>([]);
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(100);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [restoreConfirm, setRestoreConfirm] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    try {
      setBackups(await api.listCloudBackups());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  /** Latest backup per profile (list is newest-first from the backend). */
  const latestByProfile = useMemo(() => {
    const map = new Map<string, CloudBackupInfo>();
    for (const b of backups) if (!map.has(b.profile_id)) map.set(b.profile_id, b);
    return map;
  }, [backups]);

  const rows = useMemo(() => {
    const q = search.trim().toLowerCase();
    return profiles
      .filter((p) => latestByProfile.has(p.id))
      .filter((p) => !q || p.name.toLowerCase().includes(q))
      .map((p) => ({ profile: p, backup: latestByProfile.get(p.id)! }))
      .sort((a, b) => b.backup.uploaded_at.localeCompare(a.backup.uploaded_at));
  }, [profiles, latestByProfile, search]);

  useEffect(() => {
    setPage(0);
  }, [search, rowsPerPage, rows.length]);

  const fmtDate = (iso: string) => {
    const d = new Date(iso);
    return isNaN(d.getTime())
      ? iso
      : new Intl.DateTimeFormat(i18n.language, {
          dateStyle: "short",
          timeStyle: "short",
        }).format(d);
  };

  const profileName = (id: string) =>
    profiles.find((p) => p.id === id)?.name ?? id;

  const totalPages = Math.max(1, Math.ceil(rows.length / rowsPerPage));
  const safePage = Math.min(page, totalPages - 1);
  const paged = rows.slice(safePage * rowsPerPage, (safePage + 1) * rowsPerPage);

  const runAction = async (id: string, fn: () => Promise<void>, doneMsg: string) => {
    setBusyId(id);
    setError(null);
    setNotice(null);
    try {
      await fn();
      setNotice(doneMsg);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusyId(null);
    }
  };

  const confirmRestore = () => {
    const id = restoreConfirm;
    setRestoreConfirm(null);
    if (!id) return;
    void runAction(
      id,
      () => api.restoreFromCloud(id),
      t("cloudSync.restoreDone", { name: profileName(id) }),
    );
  };

  const confirmDelete = () => {
    const id = deleteConfirm;
    setDeleteConfirm(null);
    if (!id) return;
    void runAction(
      id,
      () => api.deleteCloudBackup(id),
      t("cloudSync.deleteDone", { name: profileName(id) }),
    );
  };

  const th = "h-10 px-3 text-left align-middle text-xs font-medium text-fg";

  return (
    <div className="flex h-full flex-col p-4">
      <div className="card flex min-h-0 flex-1 flex-col overflow-hidden">
        <div className="flex min-h-[60px] flex-wrap items-center gap-3 p-3">
          <span className="inline-flex items-center gap-1.5 text-sm font-medium text-fg">
            <Cloud className="h-4 w-4 text-accent" aria-hidden="true" />
            {t("cloudSync.title")}
          </span>
          <div className="relative ml-auto w-[225px]">
            <Search
              className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-muted"
              aria-hidden="true"
            />
            <input
              type="search"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("cloudSync.searchPlaceholder")}
              aria-label={t("cloudSync.searchPlaceholder")}
              className="h-9 w-full rounded-md border border-border bg-surface-2 pl-9 pr-3 text-sm text-fg placeholder:text-fg-muted focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/50"
            />
          </div>
        </div>

        {(error || notice) && (
          <p
            className={`px-4 py-2 text-xs ${error ? "bg-danger/10 text-danger" : "bg-accent/10 text-accent"}`}
            role={error ? "alert" : "status"}
          >
            {error ?? notice}
          </p>
        )}

        {rows.length === 0 && !search ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 p-12 text-center">
            <Cloud className="h-8 w-8 text-fg-muted/50" aria-hidden="true" />
            <p className="text-sm text-fg-muted">{t("cloudSync.empty")}</p>
            <p className="max-w-md text-xs text-fg-muted">{t("cloudSync.emptyHint")}</p>
          </div>
        ) : rows.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 p-12 text-center">
            <SearchX className="h-8 w-8 text-fg-muted/50" aria-hidden="true" />
            <p className="text-sm text-fg-muted">{t("table.noMatches")}</p>
          </div>
        ) : (
          <div className="flex-1 overflow-auto">
            <table className="w-full text-sm">
              <thead className="sticky top-0 z-10 border-b border-border bg-surface-2">
                <tr className="h-10 border-b border-border">
                  <th scope="col" className={th}>{t("table.profileName")}</th>
                  <th scope="col" className={th}>{t("cloudSync.lastBackup")}</th>
                  <th scope="col" className={th}>{t("cloudSync.size")}</th>
                  <th scope="col" className={th}>{t("cloudSync.parts")}</th>
                  <th scope="col" className="w-56 px-3 text-right align-middle">
                    <span className="sr-only">{t("table.actions")}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {paged.map(({ profile: p, backup: b }) => {
                  const busy = busyId === p.id;
                  const running = runningIds.has(p.id);
                  return (
                    <tr
                      key={p.id}
                      className="h-[49px] border-b border-border transition-colors hover:bg-accent/[0.03] [&>td]:align-middle"
                    >
                      <td className="max-w-0 px-3 py-2">
                        <span className="block truncate font-medium text-fg" title={p.name}>
                          {p.name}
                        </span>
                      </td>
                      <td
                        className="whitespace-nowrap px-3 py-2 text-fg-muted"
                        title={`sha256: ${b.sha256}`}
                      >
                        {fmtDate(b.uploaded_at)}
                      </td>
                      <td className="whitespace-nowrap px-3 py-2 text-fg-muted">
                        {fmtBytes(b.size)}
                      </td>
                      <td className="whitespace-nowrap px-3 py-2 text-fg-muted">
                        {b.part_count}
                      </td>
                      <td className="whitespace-nowrap px-3 py-2 text-right">
                        <button
                          type="button"
                          disabled={busy || running}
                          title={running ? t("cloudSync.restoreWhileRunning") : undefined}
                          onClick={() => setRestoreConfirm(p.id)}
                          className="mr-3 inline-flex items-center gap-1 rounded text-sm font-medium text-accent hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-50"
                        >
                          <CloudDownload className="h-4 w-4" aria-hidden="true" />
                          {t("cloudSync.restore")}
                        </button>
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => setDeleteConfirm(p.id)}
                          className="inline-flex items-center gap-1 rounded text-sm font-medium text-danger hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-50"
                        >
                          <Trash2 className="h-4 w-4" aria-hidden="true" />
                          {t("cloudSync.delete")}
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
          total={rows.length}
          page={safePage}
          rowsPerPage={rowsPerPage}
          onPageChange={setPage}
          onRowsPerPageChange={setRowsPerPage}
          profileCount={profileCount}
          settings={settings}
        />
      </div>

      {restoreConfirm && (
        <ConfirmDialog
          message={t("cloudSync.confirmRestore", { name: profileName(restoreConfirm) })}
          confirmLabel={t("cloudSync.restore")}
          onConfirm={confirmRestore}
          onCancel={() => setRestoreConfirm(null)}
        />
      )}
      {deleteConfirm && (
        <ConfirmDialog
          message={t("cloudSync.confirmDelete", { name: profileName(deleteConfirm) })}
          confirmLabel={t("cloudSync.delete")}
          onConfirm={confirmDelete}
          onCancel={() => setDeleteConfirm(null)}
        />
      )}
    </div>
  );
}
