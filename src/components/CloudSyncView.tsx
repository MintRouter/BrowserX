import {
  ChevronDown,
  ChevronRight,
  Cloud,
  CloudDownload,
  CloudUpload,
  RotateCcw,
  Search,
  SearchX,
  Trash2,
} from "lucide-react";
import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  api,
  isTauri,
  onCloudProgress,
  type CloudBackupInfo,
  type CloudProgressEvent,
  type CloudUploadState,
  type Profile,
} from "../lib/api";
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
 * (`cloud_backups` records) or a cloud upload state. Standard card shell like
 * Running/Trash (W50E): toolbar + table + footer.
 * (W52-C) Adds: expandable per-profile version history with restore/delete
 * per version (W52-F — commands accept `uploadedAt`), upload status + retry
 * (C1), per-part progress from `cloud://progress` (C6), and Sync now (C5).
 */
export function CloudSyncView({
  profiles,
  runningIds,
  profileCount,
  settings,
}: CloudSyncViewProps) {
  const { t, i18n } = useTranslation();
  const [backups, setBackups] = useState<CloudBackupInfo[]>([]);
  const [states, setStates] = useState<CloudUploadState[]>([]);
  const [progress, setProgress] = useState<Record<string, CloudProgressEvent>>({});
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(100);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  /** (W52-F) Version target of the pending confirm — profile + uploaded_at. */
  const [deleteConfirm, setDeleteConfirm] = useState<{
    id: string;
    uploadedAt: string;
  } | null>(null);
  const [restoreConfirm, setRestoreConfirm] = useState<{
    id: string;
    uploadedAt: string;
  } | null>(null);

  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const [b, s] = await Promise.all([
        api.listCloudBackups(),
        api.listCloudUploadStates(),
      ]);
      setBackups(b);
      setStates(s);
      // Prune stale upload bars: a failed background upload emits no
      // completion event, but its state row flips away from "uploading".
      setProgress((prev) => {
        const next: Record<string, CloudProgressEvent> = {};
        for (const [id, e] of Object.entries(prev)) {
          const st = s.find((x) => x.profile_id === id);
          if (e.phase === "upload" && st && st.status !== "uploading") continue;
          next[id] = e;
        }
        return next;
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // (W52-C C6) Per-part progress of cloud uploads/downloads.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onCloudProgress((e) => {
      const done = e.partIndex >= e.partCount && e.bytesDone >= e.bytesTotal;
      setProgress((prev) => {
        const next = { ...prev };
        if (done) delete next[e.profileId];
        else next[e.profileId] = e;
        return next;
      });
      if (done) void refresh();
    }).then((f) => {
      unlisten = f;
    });
    return () => unlisten?.();
  }, [refresh]);

  // While a transfer is in flight, refresh periodically so upload states stay
  // current and stale bars from failed background uploads get pruned.
  const hasProgress = Object.keys(progress).length > 0;
  useEffect(() => {
    if (!hasProgress) return;
    const id = setInterval(() => void refresh(), 10_000);
    return () => clearInterval(id);
  }, [hasProgress, refresh]);

  /** (W52-C C2) All backup versions per profile — newest first (backend order). */
  const versionsByProfile = useMemo(() => {
    const map = new Map<string, CloudBackupInfo[]>();
    for (const b of backups) {
      const list = map.get(b.profile_id);
      if (list) list.push(b);
      else map.set(b.profile_id, [b]);
    }
    return map;
  }, [backups]);

  /** (W52-C C1) Upload state per profile. */
  const stateByProfile = useMemo(() => {
    const map = new Map<string, CloudUploadState>();
    for (const s of states) if (!map.has(s.profile_id)) map.set(s.profile_id, s);
    return map;
  }, [states]);

  const rows = useMemo(() => {
    const q = search.trim().toLowerCase();
    return profiles
      .filter((p) => versionsByProfile.has(p.id) || stateByProfile.has(p.id))
      .filter((p) => !q || p.name.toLowerCase().includes(q))
      .map((p) => ({
        profile: p,
        versions: versionsByProfile.get(p.id) ?? [],
        state: stateByProfile.get(p.id),
      }))
      .sort((a, b) => {
        const ka = a.versions[0]?.uploaded_at ?? a.state?.updated_at ?? "";
        const kb = b.versions[0]?.uploaded_at ?? b.state?.updated_at ?? "";
        return kb.localeCompare(ka);
      });
  }, [profiles, versionsByProfile, stateByProfile, search]);

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
    const target = restoreConfirm;
    setRestoreConfirm(null);
    if (!target) return;
    void runAction(
      target.id,
      () => api.restoreFromCloud(target.id, target.uploadedAt),
      t("cloudSync.restoreDone", { name: profileName(target.id) }),
    );
  };

  const confirmDelete = () => {
    const target = deleteConfirm;
    setDeleteConfirm(null);
    if (!target) return;
    void runAction(
      target.id,
      () => api.deleteCloudBackup(target.id, target.uploadedAt),
      t("cloudSync.deleteDone", { name: profileName(target.id) }),
    );
  };

  const syncNow = (id: string) =>
    void runAction(
      id,
      () => api.backupNow(id),
      t("cloudSync.backupNowDone", { name: profileName(id) }),
    );

  const retryUpload = (id: string) =>
    void runAction(
      id,
      () => api.retryCloudUpload(id),
      t("cloudSync.retryUploadDone", { name: profileName(id) }),
    );

  const toggleExpanded = (id: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const statusLabel: Record<string, string> = {
    pending: t("cloudSync.statusPending"),
    uploading: t("cloudSync.statusUploading"),
    uploaded: t("cloudSync.statusUploaded"),
    failed: t("cloudSync.statusFailed"),
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
                  <th scope="col" className="w-8 px-2 align-middle">
                    <span className="sr-only">{t("cloudSync.history")}</span>
                  </th>
                  <th scope="col" className={th}>{t("table.profileName")}</th>
                  <th scope="col" className={th}>{t("cloudSync.lastBackup")}</th>
                  <th scope="col" className={th}>{t("cloudSync.size")}</th>
                  <th scope="col" className={th}>{t("cloudSync.parts")}</th>
                  <th scope="col" className={th}>{t("cloudSync.uploadStatus")}</th>
                  <th scope="col" className="w-56 px-3 text-right align-middle">
                    <span className="sr-only">{t("table.actions")}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {paged.map(({ profile: p, versions, state }) => {
                  const latest = versions[0];
                  const busy = busyId === p.id;
                  const running = runningIds.has(p.id);
                  const pr = progress[p.id];
                  const open = expanded.has(p.id);
                  const pct =
                    pr && pr.bytesTotal > 0
                      ? Math.min(100, Math.round((pr.bytesDone / pr.bytesTotal) * 100))
                      : 0;
                  return (
                    <Fragment key={p.id}>
                      <tr className="h-[49px] border-b border-border transition-colors hover:bg-accent/[0.03] [&>td]:align-middle">
                        <td className="px-2 py-2">
                          {versions.length > 0 && (
                            <button
                              type="button"
                              aria-expanded={open}
                              aria-label={t("cloudSync.history")}
                              title={t("cloudSync.history")}
                              onClick={() => toggleExpanded(p.id)}
                              className="grid h-6 w-6 place-items-center rounded text-fg-muted transition-colors hover:bg-surface-2 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                            >
                              {open ? (
                                <ChevronDown className="h-4 w-4" aria-hidden="true" />
                              ) : (
                                <ChevronRight className="h-4 w-4" aria-hidden="true" />
                              )}
                            </button>
                          )}
                        </td>
                        <td className="max-w-0 px-3 py-2">
                          <span className="block truncate font-medium text-fg" title={p.name}>
                            {p.name}
                          </span>
                        </td>
                        <td
                          className="whitespace-nowrap px-3 py-2 text-fg-muted"
                          title={latest ? `sha256: ${latest.sha256}` : undefined}
                        >
                          {latest ? fmtDate(latest.uploaded_at) : "—"}
                        </td>
                        <td className="whitespace-nowrap px-3 py-2 text-fg-muted">
                          {latest ? fmtBytes(latest.size) : "—"}
                        </td>
                        <td className="whitespace-nowrap px-3 py-2 text-fg-muted">
                          {latest ? latest.part_count : "—"}
                        </td>
                        <td className="whitespace-nowrap px-3 py-2">
                          {pr ? (
                            <div className="w-44">
                              <p className="mb-1 truncate text-xs text-fg-muted">
                                {t(
                                  pr.phase === "download"
                                    ? "cloudSync.progressDownload"
                                    : "cloudSync.progressUpload",
                                  {
                                    part: Math.min(pr.partIndex + 1, pr.partCount),
                                    total: pr.partCount,
                                  },
                                )}
                              </p>
                              <div
                                role="progressbar"
                                aria-valuenow={pct}
                                aria-valuemin={0}
                                aria-valuemax={100}
                                className="h-1 overflow-hidden rounded-full bg-border"
                              >
                                <div
                                  className="h-full rounded-full bg-accent transition-[width]"
                                  style={{ width: `${pct}%` }}
                                />
                              </div>
                            </div>
                          ) : state ? (
                            <span
                              className={`inline-flex items-center gap-1.5 text-xs ${
                                state.status === "failed"
                                  ? "text-danger"
                                  : state.status === "uploaded"
                                    ? "text-success"
                                    : "text-fg-muted"
                              }`}
                              title={
                                state.status === "failed" && state.last_error
                                  ? `${t("cloudSync.lastError", { error: state.last_error })} · ${t("cloudSync.retryCount", { count: state.retry_count })}`
                                  : undefined
                              }
                            >
                              <span
                                className="h-1.5 w-1.5 rounded-full bg-current"
                                aria-hidden="true"
                              />
                              {statusLabel[state.status] ?? state.status}
                            </span>
                          ) : (
                            <span className="text-xs text-fg-muted">—</span>
                          )}
                        </td>
                        <td className="whitespace-nowrap px-3 py-2 text-right">
                          {state?.status === "failed" && (
                            <button
                              type="button"
                              disabled={busy}
                              onClick={() => retryUpload(p.id)}
                              className="mr-3 inline-flex items-center gap-1 rounded text-sm font-medium text-accent hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-50"
                            >
                              <RotateCcw className="h-4 w-4" aria-hidden="true" />
                              {t("cloudSync.retryUpload")}
                            </button>
                          )}
                          <button
                            type="button"
                            disabled={busy || running}
                            title={running ? t("cloudSync.backupNowWhileRunning") : undefined}
                            onClick={() => syncNow(p.id)}
                            className="inline-flex items-center gap-1 rounded text-sm font-medium text-accent hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-50"
                          >
                            <CloudUpload className="h-4 w-4" aria-hidden="true" />
                            {t("cloudSync.backupNow")}
                          </button>
                        </td>
                      </tr>
                      {open && versions.length > 0 && (
                        <tr className="border-b border-border bg-surface-2/50">
                          <td className="px-2 py-2" />
                          <td colSpan={6} className="px-3 py-2">
                            <ul className="flex flex-col gap-1.5">
                              {versions.map((v, idx) => {
                                const isLatest = idx === 0;
                                return (
                                  <li
                                    key={v.uploaded_at}
                                    className="flex items-center gap-4 text-xs text-fg-muted"
                                  >
                                    <span
                                      className="whitespace-nowrap"
                                      title={`sha256: ${v.sha256}`}
                                    >
                                      {fmtDate(v.uploaded_at)}
                                    </span>
                                    <span className="whitespace-nowrap">{fmtBytes(v.size)}</span>
                                    <span className="whitespace-nowrap">
                                      {t("cloudSync.parts")}: {v.part_count}
                                    </span>
                                    {isLatest && (
                                      <span className="rounded-full bg-accent/10 px-2 py-0.5 text-[11px] font-medium text-accent">
                                        {t("cloudSync.latest")}
                                      </span>
                                    )}
                                    <span className="ml-auto flex items-center">
                                      <button
                                        type="button"
                                        disabled={busy || running}
                                        title={
                                          running
                                            ? t("cloudSync.restoreWhileRunning")
                                            : undefined
                                        }
                                        onClick={() =>
                                          setRestoreConfirm({
                                            id: p.id,
                                            uploadedAt: v.uploaded_at,
                                          })
                                        }
                                        className="mr-3 inline-flex items-center gap-1 rounded text-xs font-medium text-accent hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-50"
                                      >
                                        <CloudDownload className="h-3.5 w-3.5" aria-hidden="true" />
                                        {t("cloudSync.restore")}
                                      </button>
                                      <button
                                        type="button"
                                        disabled={busy}
                                        onClick={() =>
                                          setDeleteConfirm({
                                            id: p.id,
                                            uploadedAt: v.uploaded_at,
                                          })
                                        }
                                        className="inline-flex items-center gap-1 rounded text-xs font-medium text-danger hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-50"
                                      >
                                        <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
                                        {t("cloudSync.delete")}
                                      </button>
                                    </span>
                                  </li>
                                );
                              })}
                            </ul>
                          </td>
                        </tr>
                      )}
                    </Fragment>
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
          message={t("cloudSync.confirmRestore", {
            name: profileName(restoreConfirm.id),
            date: fmtDate(restoreConfirm.uploadedAt),
          })}
          confirmLabel={t("cloudSync.restore")}
          onConfirm={confirmRestore}
          onCancel={() => setRestoreConfirm(null)}
        />
      )}
      {deleteConfirm && (
        <ConfirmDialog
          message={t("cloudSync.confirmDelete", {
            name: profileName(deleteConfirm.id),
            date: fmtDate(deleteConfirm.uploadedAt),
          })}
          confirmLabel={t("cloudSync.delete")}
          onConfirm={confirmDelete}
          onCancel={() => setDeleteConfirm(null)}
        />
      )}
    </div>
  );
}
