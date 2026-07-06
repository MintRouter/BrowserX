import { Plus, SearchX } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Folder, type Profile } from "../lib/api";
import { CookieDialog } from "./CookieDialog";
import { CookieRobotDialog } from "./CookieRobotDialog";
import { toProfileFilter, type ProfileFilters } from "./FilterPanel";
import { ProfilesToolbar } from "./ProfilesToolbar";
import {
  DEFAULT_COLUMNS,
  formatBytes,
  ProfileTable,
  type ColumnVisibility,
  type ProfilesSort,
} from "./ProfileTable";
import { TableFooter } from "./TableFooter";

interface ProfileListProps {
  profiles: Profile[];
  folders: Folder[];
  runningIds: Set<string>;
  /** (W23a) Profiles whose last session crashed (badge on the row). */
  crashedIds: Set<string>;
  search: string;
  onSearchChange: (value: string) => void;
  selected: Set<string>;
  onSelectedChange: (selected: Set<string>) => void;
  settings: Record<string, string> | null;
  onNewProfile: () => void;
  onQuickProfile: () => Promise<void>;
  onEdit: (profile: Profile) => void;
  onLaunch: (id: string) => Promise<void>;
  onStop: (id: string) => Promise<void>;
  onLaunchSelected: () => Promise<void>;
  onStopSelected: () => Promise<void>;
  onRefresh: () => Promise<void>;
  /** (F1a) Selective refetch after import — profiles + folder counts (W23d pattern). */
  onImported: () => Promise<void>;
  /** (F1a) Selective refetch after inline rename — profiles only (W23d pattern). */
  onRenamed: () => Promise<void>;
  onClone: (profile: Profile) => Promise<void>;
  onTrash: (ids: string[]) => Promise<void>;
  onMove: (ids: string[], folderId: string | null) => Promise<void>;
  onAddTags: (ids: string[], tags: string[]) => Promise<void>;
  onToggleFavorite: (profile: Profile) => Promise<void>;
}

export function ProfileList(props: ProfileListProps) {
  const {
    profiles,
    folders,
    runningIds,
    search,
    selected,
    onSelectedChange,
  } = props;
  const { t } = useTranslation();
  const [sort, setSort] = useState<ProfilesSort>({ key: "updated", dir: "desc" });
  // (P3-2b) Advanced filters. os/proxy/tag/folder run through the backend
  // search_profiles (allowed-ID set, selective fetch — no loadAll); the
  // runtime criterion is FE-only (matched against runningIds below).
  const [filters, setFilters] = useState<ProfileFilters>({});
  const [allowedIds, setAllowedIds] = useState<Set<string> | null>(null);
  const [columns, setColumns] = useState<ColumnVisibility>(DEFAULT_COLUMNS);
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(100);
  const [sizes, setSizes] = useState<Record<string, number>>({});
  const [toast, setToast] = useState<string | null>(null);
  // (W20a) Inline rename target + signal that opens the toolbar move popover.
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [moveSignal, setMoveSignal] = useState(0);
  // (W24a) Cookie import/export dialog target (export supports bulk).
  const [cookieDialog, setCookieDialog] = useState<{
    mode: "export" | "import";
    profiles: Profile[];
  } | null>(null);
  // (P3-4b) CookieRobot dialog target (one profile at a time).
  const [robotProfile, setRobotProfile] = useState<Profile | null>(null);

  const folderName = (id: string | null) =>
    id ? (folders.find((f) => f.id === id)?.name ?? "") : "";

  // (P3-2b) Backend criteria (os/proxy/tag/folder) → search_profiles returns
  // the matching ID set; only that call runs, nothing else is refetched.
  const backendFilter = toProfileFilter(filters);
  const hasBackendFilter = Object.values(backendFilter).some(
    (v) => v !== undefined,
  );
  const backendKey = JSON.stringify(backendFilter);
  useEffect(() => {
    if (!hasBackendFilter) {
      setAllowedIds(null);
      return;
    }
    let cancelled = false;
    api
      .searchProfiles("", JSON.parse(backendKey))
      .then((res) => {
        if (!cancelled) setAllowedIds(new Set(res.map((p) => p.id)));
      })
      .catch(() => {
        if (!cancelled) setAllowedIds(null);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [backendKey, hasBackendFilter, profiles]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    let list = q
      ? profiles.filter(
          (p) =>
            p.name.toLowerCase().includes(q) ||
            p.platform.toLowerCase().includes(q) ||
            (p.notes ?? "").toLowerCase().includes(q) ||
            p.tags.some((tag) => tag.toLowerCase().includes(q)) ||
            folderName(p.folder_id).toLowerCase().includes(q),
        )
      : [...profiles];
    // (P3-2b) Intersect with the backend filter result, then apply the
    // FE-only runtime criterion (running state is not a DB column).
    if (allowedIds) list = list.filter((p) => allowedIds.has(p.id));
    if (filters.runtime)
      list = list.filter(
        (p) => runningIds.has(p.id) === (filters.runtime === "running"),
      );
    list.sort((a, b) => {
      const cmp =
        sort.key === "name"
          ? a.name.localeCompare(b.name)
          : a.updated_at.localeCompare(b.updated_at);
      return sort.dir === "asc" ? cmp : -cmp;
    });
    return list;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [profiles, search, sort, folders, allowedIds, filters.runtime, runningIds]);

  useEffect(() => {
    setPage(0);
  }, [search, rowsPerPage, profiles.length, filters]);

  const totalPages = Math.max(1, Math.ceil(filtered.length / rowsPerPage));
  const safePage = Math.min(page, totalPages - 1);
  const paged = filtered.slice(safePage * rowsPerPage, (safePage + 1) * rowsPerPage);

  const singleSelected =
    selected.size === 1 ? (profiles.find((p) => selected.has(p.id)) ?? null) : null;
  const hasRunningSelected = [...selected].some((id) => runningIds.has(id));

  // Lazy-load storage sizes for the visible page only (never blocks render).
  const pagedKey = paged.map((p) => p.id).join(",");
  useEffect(() => {
    const missing = pagedKey.split(",").filter((id) => id && !(id in sizes));
    if (missing.length === 0) return;
    let cancelled = false;
    api
      .profileStorageSizes(missing)
      .then((res) => {
        if (cancelled) return;
        setSizes((prev) => {
          const next = { ...prev };
          for (const s of res) next[s.profile_id] = s.bytes;
          return next;
        });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [pagedKey, sizes]);

  // Auto-hide the toast after a few seconds.
  useEffect(() => {
    if (!toast) return;
    const timer = setTimeout(() => setToast(null), 4000);
    return () => clearTimeout(timer);
  }, [toast]);

  const handleClearCache = async (ids: string[]) => {
    const targets = ids.filter((id) => !runningIds.has(id));
    if (targets.length === 0) return;
    try {
      const results = await api.clearProfileCache(targets);
      const freed = results
        .filter((r) => !r.error)
        .reduce((sum, r) => sum + r.freed_bytes, 0);
      const failed = results.filter((r) => r.error).length;
      setToast(
        failed > 0
          ? t("table.clearCachePartial", { freed: formatBytes(freed), failed })
          : t("table.cacheCleared", { freed: formatBytes(freed) }),
      );
      const refreshed = await api.profileStorageSizes(targets);
      setSizes((prev) => {
        const next = { ...prev };
        for (const s of refreshed) next[s.profile_id] = s.bytes;
        return next;
      });
    } catch {
      setToast(t("table.clearCacheFailed"));
    }
  };

  // Export/import .bxprofile (W19a). Proxy password is never in the file.
  const downloadBxprofile = (p: Profile, json: string) => {
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${p.name.replace(/[\\/:*?"<>|]+/g, "_")}.bxprofile`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
  };

  const handleExport = async (p: Profile) => {
    try {
      const json = await api.exportProfile(p.id);
      downloadBxprofile(p, json);
      setToast(t("exchange.exportSuccess", { name: p.name }));
    } catch (err) {
      setToast(t("exchange.exportFailed", { error: String(err) }));
    }
  };

  // (W25a) Bulk export the selection — one .bxprofile download per profile,
  // reusing the W19a export command (extensions V8 + cookie behavior included).
  const handleExportSelected = async () => {
    const targets = profiles.filter((p) => selected.has(p.id));
    const first = targets[0];
    if (!first) return;
    if (targets.length === 1) return handleExport(first);
    let ok = 0;
    let lastError = "";
    for (const p of targets) {
      try {
        downloadBxprofile(p, await api.exportProfile(p.id));
        ok++;
      } catch (err) {
        lastError = String(err);
      }
    }
    setToast(
      ok === 0
        ? t("exchange.exportFailed", { error: lastError })
        : t("exchange.bulkExported", { ok, total: targets.length }),
    );
  };

  const handleImport = async (file: File) => {
    try {
      const json = await file.text();
      const profile = await api.importProfile(json);
      await props.onImported();
      setToast(t("exchange.importSuccess", { name: profile.name }));
    } catch (err) {
      setToast(t("exchange.importFailed", { error: String(err) }));
    }
  };

  // (W20a) Inline rename — empty/unchanged names are a silent cancel.
  const handleRenameSubmit = async (id: string, name: string) => {
    setRenamingId(null);
    const trimmed = name.trim();
    const current = profiles.find((p) => p.id === id);
    if (!trimmed || !current || trimmed === current.name) return;
    try {
      await api.updateProfile(id, { name: trimmed });
      await props.onRenamed();
    } catch (err) {
      setToast(t("listUtils.renameFailed", { error: String(err) }));
    }
  };

  const handleCopyId = async (id: string) => {
    try {
      await navigator.clipboard.writeText(id);
      setToast(t("listUtils.idCopied"));
    } catch {
      setToast(t("listUtils.copyFailed"));
    }
  };

  // (W24c) Copy the live CDP websocket endpoint (ws://127.0.0.1:{port}/devtools/…)
  // of a running session for Playwright/Puppeteer connectOverCDP.
  const handleCopyCdpUrl = async (id: string) => {
    try {
      const url = await api.getCdpWsUrl(id);
      await navigator.clipboard.writeText(url);
      setToast(t("listUtils.cdpUrlCopied"));
    } catch (err) {
      setToast(t("listUtils.copyCdpUrlFailed", { error: String(err) }));
    }
  };

  // (W24a) Bulk cookie export for the current selection.
  const handleExportCookiesSelected = () => {
    const targets = profiles.filter((p) => selected.has(p.id));
    if (targets.length > 0) setCookieDialog({ mode: "export", profiles: targets });
  };

  const handleBringToFront = async (id: string) => {
    try {
      await api.bringToFront(id);
    } catch (err) {
      setToast(t("listUtils.bringToFrontFailed", { error: String(err) }));
    }
  };

  // (W20a) Global shortcuts (skipped while typing in a field):
  // F2 rename · ⌘/Ctrl+Enter launch · ⌘/Ctrl+Shift+S stop · ⌘/Ctrl+Shift+C clone
  // · ⌘/Ctrl+Shift+M move · ⌘/Ctrl+Backspace trash.
  useEffect(() => {
    const isEditable = (el: EventTarget | null) =>
      el instanceof HTMLElement &&
      (el instanceof HTMLInputElement ||
        el instanceof HTMLTextAreaElement ||
        el instanceof HTMLSelectElement ||
        el.isContentEditable);

    const onKeyDown = (e: KeyboardEvent) => {
      if (isEditable(e.target)) return;
      if (e.key === "F2") {
        if (singleSelected) {
          e.preventDefault();
          setRenamingId(singleSelected.id);
        }
        return;
      }
      if (!(e.metaKey || e.ctrlKey)) return;
      if (e.key === "Enter" && !e.shiftKey) {
        if (selected.size > 0) {
          e.preventDefault();
          void props.onLaunchSelected();
        }
      } else if (e.shiftKey && e.key.toLowerCase() === "s") {
        if (hasRunningSelected) {
          e.preventDefault();
          void props.onStopSelected();
        }
      } else if (e.shiftKey && e.key.toLowerCase() === "c") {
        if (singleSelected) {
          e.preventDefault();
          void props.onClone(singleSelected);
        }
      } else if (e.shiftKey && e.key.toLowerCase() === "m") {
        if (selected.size > 0) {
          e.preventDefault();
          setMoveSignal((v) => v + 1);
        }
      } else if (e.key === "Backspace" && !e.shiftKey) {
        if (selected.size > 0) {
          e.preventDefault();
          void props.onTrash([...selected]);
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [selected, singleSelected, hasRunningSelected, props]);

  const toggleRow = (id: string) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    onSelectedChange(next);
  };

  const togglePage = (ids: string[], select: boolean) => {
    const next = new Set(selected);
    for (const id of ids) {
      if (select) next.add(id);
      else next.delete(id);
    }
    onSelectedChange(next);
  };

  const selectedIds = [...selected];

  return (
    <div className="flex h-full flex-col p-4">
      {/* Toolbar lives inside the table's white card (ML table-header, 1.1) */}
      <div className="card flex min-h-0 flex-1 flex-col overflow-hidden">
        <ProfilesToolbar
          search={search}
          onSearchChange={props.onSearchChange}
          filters={filters}
          onFiltersChange={setFilters}
          selectedCount={selected.size}
          hasRunningSelected={hasRunningSelected}
          folders={folders}
          sort={sort}
          onSortChange={setSort}
          onNewProfile={props.onNewProfile}
          onQuickProfile={() => void props.onQuickProfile()}
          onImport={(file) => void handleImport(file)}
          onLaunchSelected={() => void props.onLaunchSelected()}
          onStopSelected={() => void props.onStopSelected()}
          onRefresh={() => void props.onRefresh()}
          onEditSelected={() => singleSelected && props.onEdit(singleSelected)}
          onAddTags={(tags) => void props.onAddTags(selectedIds, tags)}
          onMoveToFolder={(folderId) => void props.onMove(selectedIds, folderId)}
          onCloneSelected={() => singleSelected && void props.onClone(singleSelected)}
          onExportSelected={() => void handleExportSelected()}
          onExportCookiesSelected={handleExportCookiesSelected}
          onClearCacheSelected={() => void handleClearCache(selectedIds)}
          onTrashSelected={() => void props.onTrash(selectedIds)}
          onClearSelection={() => onSelectedChange(new Set())}
          moveSignal={moveSignal}
        />
        {profiles.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 p-12 text-center">
            <svg
              width="140"
              height="100"
              viewBox="0 0 140 100"
              fill="none"
              aria-hidden="true"
            >
              <rect x="26" y="22" width="62" height="72" rx="6" fill="#F1EDED" transform="rotate(-8 26 22)" />
              <rect x="50" y="10" width="62" height="80" rx="6" fill="#FFFFFF" stroke="#E5E1E1" strokeWidth="1.5" />
              <rect x="60" y="24" width="34" height="5" rx="2.5" fill="#E5E1E1" />
              <rect x="60" y="36" width="42" height="5" rx="2.5" fill="#F1EDED" />
              <rect x="60" y="48" width="26" height="5" rx="2.5" fill="#F1EDED" />
              <circle cx="106" cy="72" r="16" fill="#F0F6FF" />
              <path d="M106 66v12M100 72h12" stroke="#055FF0" strokeWidth="2.5" strokeLinecap="round" />
            </svg>
            <p className="text-xl font-medium text-fg">{t("table.emptyTitle")}</p>
            <p className="max-w-xs text-sm text-fg-muted">{t("table.emptyHint")}</p>
            <button type="button" onClick={props.onNewProfile} className="btn-primary mt-1">
              <Plus className="h-4 w-4" aria-hidden="true" />
              <span>{t("toolbar.create")}</span>
            </button>
          </div>
        ) : filtered.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 p-12 text-center">
            <SearchX className="h-8 w-8 text-fg-muted/50" aria-hidden="true" />
            <p className="text-sm text-fg-muted">{t("table.noMatches")}</p>
          </div>
        ) : (
          <div className="flex-1 overflow-auto">
            <ProfileTable
              rows={paged}
              folders={folders}
              runningIds={runningIds}
              crashedIds={props.crashedIds}
              selected={selected}
              sizes={sizes}
              onToggleRow={toggleRow}
              onTogglePage={togglePage}
              sort={sort}
              onSortChange={setSort}
              columns={columns}
              onColumnsChange={setColumns}
              onToggleFavorite={(p) => void props.onToggleFavorite(p)}
              onLaunch={props.onLaunch}
              onStop={props.onStop}
              onEdit={props.onEdit}
              onClone={(p) => void props.onClone(p)}
              onExport={(p) => void handleExport(p)}
              onExportCookies={(p) => setCookieDialog({ mode: "export", profiles: [p] })}
              onImportCookies={(p) => setCookieDialog({ mode: "import", profiles: [p] })}
              onCookieRobot={setRobotProfile}
              onMove={(ids, folderId) => void props.onMove(ids, folderId)}
              onAddTags={(ids, tags) => void props.onAddTags(ids, tags)}
              onClearCache={(ids) => void handleClearCache(ids)}
              onTrash={(ids) => void props.onTrash(ids)}
              renamingId={renamingId}
              onRenameStart={setRenamingId}
              onRenameSubmit={(id, name) => void handleRenameSubmit(id, name)}
              onRenameCancel={() => setRenamingId(null)}
              onCopyId={(id) => void handleCopyId(id)}
              onCopyCdpUrl={(id) => void handleCopyCdpUrl(id)}
              onBringToFront={(id) => void handleBringToFront(id)}
            />
          </div>
        )}
        <TableFooter
          total={filtered.length}
          page={safePage}
          rowsPerPage={rowsPerPage}
          onPageChange={setPage}
          onRowsPerPageChange={setRowsPerPage}
          profileCount={profiles.length}
          settings={props.settings}
        />
      </div>

      {cookieDialog && (
        <CookieDialog
          mode={cookieDialog.mode}
          profiles={cookieDialog.profiles}
          onClose={() => setCookieDialog(null)}
          onDone={setToast}
        />
      )}

      {robotProfile && (
        <CookieRobotDialog
          profile={robotProfile}
          onClose={() => setRobotProfile(null)}
        />
      )}

      {toast && (
        <div
          role="status"
          aria-live="polite"
          className="fixed bottom-6 right-6 z-50 rounded-lg bg-fg px-4 py-2.5 text-sm text-surface-0 shadow-lg"
        >
          {toast}
        </div>
      )}
    </div>
  );
}
