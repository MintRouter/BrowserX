import {
  EllipsisVertical,
  Folder,
  Globe,
  Loader2,
  Plus,
  Puzzle,
  Search,
  SearchX,
  Trash2,
  Users2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Extension, type Profile } from "../lib/api";
import { MenuItem, Popover } from "./Popover";
import { Segmented, Toggle } from "./profile-form/controls";
import { TableFooter } from "./TableFooter";

interface ExtensionsViewProps {
  extensions: Extension[];
  /** Non-trashed profiles — feeds the assignment counts and the assign dialog. */
  profiles: Profile[];
  settings: Record<string, string> | null;
  /** Refetch the extensions list after any mutation. */
  onChanged: () => Promise<void>;
}

const errMsg = (err: unknown) =>
  err instanceof Error ? err.message : String(err);

export function ExtensionsView({
  extensions,
  profiles,
  settings,
  onChanged,
}: ExtensionsViewProps) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(100);
  /** null = closed · "add" = add dialog · Extension = assign-profiles dialog. */
  const [dialog, setDialog] = useState<Extension | "add" | null>(null);
  const [menuId, setMenuId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** profile id → assigned extension ids (Profiles column + assign dialog). */
  const [assignments, setAssignments] = useState<Record<string, string[]>>({});

  const loadAssignments = useCallback(async () => {
    try {
      const entries = await Promise.all(
        profiles.map(
          async (p) =>
            [
              p.id,
              (await api.getProfileExtensions(p.id)).map((e) => e.id),
            ] as const,
        ),
      );
      setAssignments(Object.fromEntries(entries));
    } catch {
      // offline / non-Tauri: keep last known map
    }
  }, [profiles]);

  useEffect(() => {
    void loadAssignments();
  }, [loadAssignments]);

  // Profiles currently assigned to each extension (ext_id → count).
  const usage = useMemo(() => {
    const map = new Map<string, number>();
    for (const ids of Object.values(assignments)) {
      for (const id of ids) map.set(id, (map.get(id) ?? 0) + 1);
    }
    return map;
  }, [assignments]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return extensions;
    return extensions.filter(
      (e) =>
        e.name.toLowerCase().includes(q) ||
        e.source_ref.toLowerCase().includes(q),
    );
  }, [extensions, search]);

  useEffect(() => {
    setPage(0);
  }, [search, rowsPerPage, extensions.length]);

  const totalPages = Math.max(1, Math.ceil(filtered.length / rowsPerPage));
  const safePage = Math.min(page, totalPages - 1);
  const paged = filtered.slice(
    safePage * rowsPerPage,
    (safePage + 1) * rowsPerPage,
  );

  const handleRemove = async (ext: Extension) => {
    if (!confirm(t("ext.confirmRemove", { name: ext.name }))) return;
    setError(null);
    try {
      await api.removeExtension(ext.id);
      await onChanged();
      await loadAssignments();
    } catch (err) {
      setError(errMsg(err));
    }
  };

  const handleToggle = async (ext: Extension, enabled: boolean) => {
    setError(null);
    try {
      await api.setExtensionEnabled(ext.id, enabled);
      await onChanged();
    } catch (err) {
      setError(errMsg(err));
    }
  };

  const th = "h-10 px-3 text-left align-middle text-xs font-medium text-fg";

  return (
    <div className="flex h-full flex-col p-4">
      <div className="card flex min-h-0 flex-1 flex-col overflow-hidden">
        {error && (
          <p
            role="alert"
            className="border-b border-danger/30 bg-danger/10 px-3 py-2 text-xs text-danger"
          >
            {error}
          </p>
        )}

        <div className="flex min-h-[60px] flex-wrap items-center gap-3 p-3">
          <button
            type="button"
            onClick={() => setDialog("add")}
            className="btn-primary h-9 py-1.5"
          >
            <Plus className="h-4 w-4" aria-hidden="true" />
            <span>{t("ext.add")}</span>
          </button>

          <div className="relative ml-auto w-[225px]">
            <Search
              className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-muted"
              aria-hidden="true"
            />
            <input
              type="search"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("ext.searchPlaceholder")}
              aria-label={t("ext.searchPlaceholder")}
              className="h-9 w-full rounded-md border border-border bg-surface-2 pl-9 pr-3 text-sm text-fg placeholder:text-fg-muted focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/50"
            />
          </div>
        </div>

        {extensions.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 p-12 text-center">
            <div className="grid h-16 w-16 place-items-center rounded-full bg-[#F0F6FF]">
              <Puzzle className="h-8 w-8 text-accent" aria-hidden="true" />
            </div>
            <p className="text-xl font-medium text-fg">{t("ext.emptyTitle")}</p>
            <p className="max-w-xs text-sm text-fg-muted">{t("ext.emptyHint")}</p>
            <button
              type="button"
              onClick={() => setDialog("add")}
              className="btn-primary mt-1"
            >
              <Plus className="h-4 w-4" aria-hidden="true" />
              <span>{t("ext.add")}</span>
            </button>
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
                  <th scope="col" className={th}>{t("ext.name")}</th>
                  <th scope="col" className={th}>{t("ext.source")}</th>
                  <th scope="col" className={th}>{t("ext.enabled")}</th>
                  <th scope="col" className={th}>{t("ext.profilesCol")}</th>
                  <th scope="col" className="w-10 px-1 align-middle">
                    <span className="sr-only">{t("table.rowMenu")}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {paged.map((ext) => (
                  <tr
                    key={ext.id}
                    className="h-[49px] border-b border-border transition-colors hover:bg-accent/[0.03] [&>td]:align-middle"
                  >
                    <td className="max-w-0 px-3 py-2">
                      <div className="flex items-center gap-2">
                        <Puzzle
                          className={`h-4 w-4 shrink-0 ${ext.enabled ? "text-accent" : "text-fg-muted"}`}
                          aria-hidden="true"
                        />
                        <span
                          className={`truncate font-medium ${ext.enabled ? "text-fg" : "text-fg-muted"}`}
                        >
                          {ext.name}
                        </span>
                        {!ext.enabled && (
                          <span className="inline-flex shrink-0 items-center rounded bg-surface-3 px-1.5 py-0.5 text-[10px] text-fg-muted">
                            {t("ext.disabledBadge")}
                          </span>
                        )}
                      </div>
                    </td>
                    <td className="max-w-0 px-3 py-2">
                      <div className="flex items-center gap-2 text-fg-muted">
                        {ext.source_type === "folder" ? (
                          <Folder className="h-4 w-4 shrink-0" aria-hidden="true" />
                        ) : (
                          <Globe className="h-4 w-4 shrink-0" aria-hidden="true" />
                        )}
                        <span className="shrink-0">
                          {ext.source_type === "folder"
                            ? t("ext.sourceFolder")
                            : t("ext.sourceStore")}
                        </span>
                        <span className="truncate font-mono text-xs" title={ext.source_ref}>
                          {ext.source_ref}
                        </span>
                      </div>
                    </td>
                    <td className="px-3 py-2">
                      <Toggle
                        checked={ext.enabled}
                        onChange={(next) => void handleToggle(ext, next)}
                        label={`${t("ext.enabled")}: ${ext.name}`}
                      />
                    </td>
                    <td className="px-3 py-2 tabular-nums text-fg-muted">
                      {usage.get(ext.id) ?? 0}
                    </td>
                    <td className="px-1 py-2">
                      <Popover
                        open={menuId === ext.id}
                        onClose={() => setMenuId(null)}
                        align="end"
                        label={t("table.rowMenu")}
                        trigger={
                          <button
                            type="button"
                            aria-label={`${t("table.rowMenu")}: ${ext.name}`}
                            aria-haspopup="menu"
                            aria-expanded={menuId === ext.id}
                            onClick={() => setMenuId(menuId === ext.id ? null : ext.id)}
                            className="grid h-8 w-8 place-items-center rounded-full text-fg-muted transition-colors hover:bg-surface-2 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                          >
                            <EllipsisVertical className="h-4 w-4" aria-hidden="true" />
                          </button>
                        }
                      >
                        <div role="menu" className="w-56">
                          <MenuItem
                            icon={<Users2 className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
                            onClick={() => {
                              setMenuId(null);
                              setDialog(ext);
                            }}
                          >
                            {t("ext.manageProfiles")}
                          </MenuItem>
                          <MenuItem
                            danger
                            icon={<Trash2 className="h-4 w-4" aria-hidden="true" />}
                            onClick={() => {
                              setMenuId(null);
                              void handleRemove(ext);
                            }}
                          >
                            {t("ext.remove")}
                          </MenuItem>
                        </div>
                      </Popover>
                    </td>
                  </tr>
                ))}
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
          profileCount={profiles.length}
          settings={settings}
        />
      </div>

      {dialog === "add" && (
        <AddExtensionDialog
          onClose={() => setDialog(null)}
          onAdded={async () => {
            await onChanged();
            await loadAssignments();
          }}
        />
      )}
      {dialog !== null && dialog !== "add" && (
        <AssignProfilesDialog
          ext={dialog}
          profiles={profiles}
          onClose={() => setDialog(null)}
          onSaved={loadAssignments}
        />
      )}
    </div>
  );
}

// --- Add dialog: local unpacked folder path OR Chrome Web Store URL ---

function AddExtensionDialog({
  onClose,
  onAdded,
}: {
  onClose: () => void;
  onAdded: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<"folder" | "store">("folder");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const canSubmit = value.trim().length > 0 && !busy;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    try {
      if (mode === "folder") {
        await api.addExtensionFromFolder(value.trim());
      } else {
        await api.addExtensionFromStoreUrl(value.trim());
      }
      await onAdded();
      onClose();
    } catch (err) {
      setError(errMsg(err));
      setBusy(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t("ext.addTitle")}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !busy) onClose();
      }}
    >
      <form className="card w-full max-w-lg p-5" onSubmit={handleSubmit}>
        <h2 className="text-base font-semibold text-fg">{t("ext.addTitle")}</h2>

        <div className="mt-4">
          <Segmented
            label={t("ext.sourceLabel")}
            value={mode}
            onChange={(next) => {
              setMode(next);
              setError(null);
            }}
            disabled={busy}
            options={[
              { value: "folder", label: t("ext.sourceFolder") },
              { value: "store", label: t("ext.sourceStore") },
            ]}
          />
        </div>

        <div className="mt-4">
          <label
            htmlFor="ext-add-input"
            className="mb-1.5 block text-xs font-medium text-fg"
          >
            {mode === "folder" ? t("ext.folderLabel") : t("ext.urlLabel")}
          </label>
          <input
            id="ext-add-input"
            type="text"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            disabled={busy}
            autoFocus
            placeholder={
              mode === "folder"
                ? t("ext.folderPlaceholder")
                : t("ext.urlPlaceholder")
            }
            className="h-11 w-full rounded-md border border-border bg-surface-2 px-3 font-mono text-sm text-fg placeholder:font-sans placeholder:text-fg-muted focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/50 disabled:opacity-50"
          />
          <p className="mt-1.5 text-xs text-fg-muted">
            {mode === "folder" ? t("ext.folderHint") : t("ext.urlHint")}
          </p>
        </div>

        {error && (
          <p role="alert" className="mt-3 text-xs text-danger">
            {error}
          </p>
        )}

        <div className="mt-5 flex flex-wrap justify-end gap-2">
          <button
            type="button"
            className="btn-secondary px-3 py-1.5 text-sm"
            disabled={busy}
            onClick={onClose}
          >
            {t("ext.cancel")}
          </button>
          <button
            type="submit"
            className="btn-primary inline-flex items-center gap-1.5 px-3 py-1.5 text-sm"
            disabled={!canSubmit}
          >
            {busy && (
              <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
            )}
            {busy && mode === "store" ? t("ext.downloading") : t("ext.add")}
          </button>
        </div>
      </form>
    </div>
  );
}

// --- Assign dialog: tick profiles that should load this extension ---

function AssignProfilesDialog({
  ext,
  profiles,
  onClose,
  onSaved,
}: {
  ext: Extension;
  profiles: Profile[];
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  /** profile id → full assigned ext-id list (fresh from backend). */
  const [current, setCurrent] = useState<Record<string, string[]>>({});
  const [checked, setChecked] = useState<Set<string>>(new Set());

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const entries = await Promise.all(
          profiles.map(
            async (p) =>
              [
                p.id,
                (await api.getProfileExtensions(p.id)).map((e) => e.id),
              ] as const,
          ),
        );
        if (cancelled) return;
        setCurrent(Object.fromEntries(entries));
        setChecked(
          new Set(
            entries.filter(([, ids]) => ids.includes(ext.id)).map(([id]) => id),
          ),
        );
      } catch (err) {
        if (!cancelled) setError(errMsg(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [profiles, ext.id]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return profiles;
    return profiles.filter((p) => p.name.toLowerCase().includes(q));
  }, [profiles, search]);

  const toggle = (id: string) => {
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleSave = async () => {
    setBusy(true);
    setError(null);
    try {
      const changed = profiles.filter((p) => {
        const had = (current[p.id] ?? []).includes(ext.id);
        return had !== checked.has(p.id);
      });
      for (const p of changed) {
        const ids = new Set(current[p.id] ?? []);
        if (checked.has(p.id)) ids.add(ext.id);
        else ids.delete(ext.id);
        await api.assignExtensions(p.id, [...ids]);
      }
      await onSaved();
      onClose();
    } catch (err) {
      setError(errMsg(err));
      setBusy(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t("ext.assignTitle", { name: ext.name })}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !busy) onClose();
      }}
    >
      <div className="card flex max-h-[80vh] w-full max-w-md flex-col p-5">
        <h2 className="text-base font-semibold text-fg">
          {t("ext.assignTitle", { name: ext.name })}
        </h2>
        <p className="mt-1 text-xs text-fg-muted">{t("ext.assignHint")}</p>

        <div className="relative mt-4">
          <Search
            className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-muted"
            aria-hidden="true"
          />
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t("ext.searchProfiles")}
            aria-label={t("ext.searchProfiles")}
            className="h-9 w-full rounded-md border border-border bg-surface-2 pl-9 pr-3 text-sm text-fg placeholder:text-fg-muted focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/50"
          />
        </div>

        <div className="mt-3 min-h-0 flex-1 overflow-auto rounded-md border border-border">
          {loading ? (
            <div className="grid place-items-center p-8">
              <Loader2
                className="h-5 w-5 animate-spin text-fg-muted"
                aria-hidden="true"
              />
              <span className="sr-only">{t("ext.loading")}</span>
            </div>
          ) : profiles.length === 0 ? (
            <p className="p-4 text-center text-sm text-fg-muted">
              {t("ext.noProfiles")}
            </p>
          ) : filtered.length === 0 ? (
            <p className="p-4 text-center text-sm text-fg-muted">
              {t("table.noMatches")}
            </p>
          ) : (
            <ul className="divide-y divide-border">
              {filtered.map((p) => (
                <li key={p.id}>
                  <label className="flex h-10 cursor-pointer items-center gap-2.5 px-3 text-sm text-fg transition-colors hover:bg-accent/[0.03]">
                    <input
                      type="checkbox"
                      checked={checked.has(p.id)}
                      onChange={() => toggle(p.id)}
                      disabled={busy}
                      className="h-4 w-4 rounded border-border text-accent accent-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                    />
                    <span className="truncate">{p.name}</span>
                  </label>
                </li>
              ))}
            </ul>
          )}
        </div>

        <p className="mt-2 text-xs text-fg-muted" aria-live="polite">
          {t("ext.selectedCount", { count: checked.size })}
        </p>

        {error && (
          <p role="alert" className="mt-2 text-xs text-danger">
            {error}
          </p>
        )}

        <div className="mt-4 flex flex-wrap justify-end gap-2">
          <button
            type="button"
            className="btn-secondary px-3 py-1.5 text-sm"
            disabled={busy}
            onClick={onClose}
          >
            {t("ext.cancel")}
          </button>
          <button
            type="button"
            className="btn-primary inline-flex items-center gap-1.5 px-3 py-1.5 text-sm"
            disabled={busy || loading}
            onClick={() => void handleSave()}
          >
            {busy && (
              <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
            )}
            {t("ext.save")}
          </button>
        </div>
      </div>
    </div>
  );
}
