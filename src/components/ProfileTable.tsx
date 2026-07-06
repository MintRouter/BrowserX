import {
  Apple,
  AppWindow,
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  Bot,
  ClipboardCopy,
  Columns3,
  Cookie,
  Copy,
  Download,
  EllipsisVertical,
  Eraser,
  FolderInput,
  HardDrive,
  Link2,
  Loader2,
  MonitorUp,
  Pencil,
  PenLine,
  Play,
  Puzzle,
  Square,
  Star,
  Tag,
  Terminal,
  Trash2,
} from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Folder, Platform, Profile } from "../lib/api";
import { ExtensionsPanel, FolderPanel, MenuItem, Popover, TagPanel } from "./Popover";

export type ProfilesSort = { key: "name" | "updated"; dir: "asc" | "desc" };

export type ColumnKey =
  | "storage"
  | "folder"
  | "notes"
  | "tags"
  | "browser"
  | "os"
  | "lastStart";
export type ColumnVisibility = Record<ColumnKey, boolean>;

export const ALL_COLUMNS: ColumnKey[] = [
  "storage",
  "folder",
  "notes",
  "tags",
  "browser",
  "os",
  "lastStart",
];

export const DEFAULT_COLUMNS: ColumnVisibility = {
  storage: true,
  folder: true,
  notes: true,
  tags: true,
  browser: true,
  os: true,
  lastStart: true,
};

/** (W26c) Windowing: fixed row height (W15/F4) + overscan rows above/below. */
const ROW_HEIGHT = 49;
const OVERSCAN = 10;

/** Relative "14 hours ago" formatting for the Last start column. */
function relativeTime(iso: string, locale: string): string {
  const ts = new Date(iso).getTime();
  if (Number.isNaN(ts)) return iso;
  const diff = ts - Date.now();
  const abs = Math.abs(diff);
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const MIN = 60_000;
  const HOUR = 3_600_000;
  const DAY = 86_400_000;
  if (abs < MIN) return rtf.format(Math.round(diff / 1000), "second");
  if (abs < HOUR) return rtf.format(Math.round(diff / MIN), "minute");
  if (abs < DAY) return rtf.format(Math.round(diff / HOUR), "hour");
  return rtf.format(Math.round(diff / DAY), "day");
}

/** Human-readable byte size for the Storage column (e.g. "3.4 MB"). */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = -1;
  do {
    v /= 1024;
    i++;
  } while (v >= 1024 && i < units.length - 1);
  return `${v >= 10 ? Math.round(v).toString() : v.toFixed(1)} ${units[i]}`;
}

interface ProfileTableProps {
  rows: Profile[];
  folders: Folder[];
  runningIds: Set<string>;
  /** (W23a) Profiles whose last session crashed (badge on the row). */
  crashedIds: Set<string>;
  selected: Set<string>;
  /** Lazy-loaded user_data_dir sizes by profile id (missing = still loading). */
  sizes: Record<string, number>;
  onToggleRow: (id: string) => void;
  onTogglePage: (ids: string[], select: boolean) => void;
  sort: ProfilesSort;
  onSortChange: (sort: ProfilesSort) => void;
  columns: ColumnVisibility;
  onColumnsChange: (columns: ColumnVisibility) => void;
  onToggleFavorite: (profile: Profile) => void;
  onLaunch: (id: string) => Promise<void>;
  onStop: (id: string) => Promise<void>;
  onEdit: (profile: Profile) => void;
  onClone: (profile: Profile) => void;
  onExport: (profile: Profile) => void;
  /** (W24a) Open the cookie export/import dialog for one profile. */
  onExportCookies: (profile: Profile) => void;
  onImportCookies: (profile: Profile) => void;
  /** (P3-4b) Open the CookieRobot dialog for one profile. */
  onCookieRobot: (profile: Profile) => void;
  onMove: (ids: string[], folderId: string | null) => void;
  onAddTags: (ids: string[], tags: string[]) => void;
  onClearCache: (ids: string[]) => void;
  onTrash: (ids: string[]) => void;
  /** (W20a) Inline rename: id of the row being renamed, or null. */
  renamingId: string | null;
  onRenameStart: (id: string) => void;
  onRenameSubmit: (id: string, name: string) => void;
  onRenameCancel: () => void;
  onCopyId: (id: string) => void;
  /** (W24c) Copy the CDP websocket URL of a running session. */
  onCopyCdpUrl: (id: string) => void;
  onBringToFront: (id: string) => void;
}

/** Chromium glyph — lucide dropped brand icons, so keep an inline equivalent. */
function ChromiumIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="10" />
      <circle cx="12" cy="12" r="4" />
      <line x1="21.17" y1="8" x2="12" y2="8" />
      <line x1="3.95" y1="6.06" x2="8.54" y2="14" />
      <line x1="10.88" y1="21.94" x2="15.46" y2="14" />
    </svg>
  );
}

function OsIcon({ platform }: { platform: Platform }) {
  const cls = "h-4 w-4 text-fg-muted";
  if (platform === "macos") return <Apple className={cls} aria-hidden="true" />;
  if (platform === "windows") return <AppWindow className={cls} aria-hidden="true" />;
  return <Terminal className={cls} aria-hidden="true" />;
}

const OS_LABEL: Record<Platform, string> = {
  macos: "macOS",
  windows: "Windows",
  linux: "Linux",
};

/** Per-row launch/stop control with its own busy state. */
function RowPlayButton({
  name,
  running,
  onLaunch,
  onStop,
}: {
  name: string;
  running: boolean;
  onLaunch: () => Promise<void>;
  onStop: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);

  const handle = async () => {
    setBusy(true);
    try {
      await (running ? onStop() : onLaunch());
    } catch (err) {
      console.error("Launch/stop failed:", err);
    } finally {
      setBusy(false);
    }
  };

  const label = `${running ? t("table.stop") : t("table.launch")}: ${name}`;
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={busy}
      onClick={handle}
      className={`grid h-[30px] w-[30px] place-items-center rounded-md transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-60 ${
        running
          ? "bg-danger/10 text-danger hover:bg-danger/20"
          : "bg-[#F0F6FF] text-accent hover:bg-[#E0EDFF]"
      }`}
    >
      {busy ? (
        <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
      ) : running ? (
        <Square className="h-3 w-3" fill="currentColor" aria-hidden="true" />
      ) : (
        <Play className="h-3.5 w-3.5" fill="currentColor" aria-hidden="true" />
      )}
    </button>
  );
}

/** (W20a) Inline rename input — Enter saves, Escape cancels, blur saves. */
function RenameInput({
  initial,
  onSubmit,
  onCancel,
}: {
  initial: string;
  onSubmit: (name: string) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [value, setValue] = useState(initial);
  // Enter also fires blur when the input unmounts — guard against double submit.
  const doneRef = useRef(false);
  const submit = () => {
    if (doneRef.current) return;
    doneRef.current = true;
    onSubmit(value);
  };
  const cancel = () => {
    if (doneRef.current) return;
    doneRef.current = true;
    onCancel();
  };
  return (
    <input
      // eslint-disable-next-line jsx-a11y/no-autofocus
      autoFocus
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onFocus={(e) => e.currentTarget.select()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") submit();
        else if (e.key === "Escape") cancel();
      }}
      onBlur={submit}
      aria-label={t("listUtils.renameLabel")}
      className="h-7 w-full max-w-[16rem] rounded-md border border-accent bg-surface-0 px-2 text-xs font-medium text-fg focus:outline-none focus:ring-2 focus:ring-accent/50"
    />
  );
}

/** Per-row kebab menu with in-panel folder picker and tag composer. */
function RowMenu({
  profile,
  folders,
  running,
  onEdit,
  onRename,
  onCopyId,
  onCopyCdpUrl,
  onBringToFront,
  onClone,
  onExport,
  onExportCookies,
  onImportCookies,
  onCookieRobot,
  onMove,
  onAddTags,
  onClearCache,
  onTrash,
  onOpenChange,
}: {
  profile: Profile;
  folders: Folder[];
  running: boolean;
  onEdit: () => void;
  onRename: () => void;
  onCopyId: () => void;
  onCopyCdpUrl: () => void;
  onBringToFront: () => void;
  onClone: () => void;
  onExport: () => void;
  onExportCookies: () => void;
  onImportCookies: () => void;
  onCookieRobot: () => void;
  onMove: (folderId: string | null) => void;
  onAddTags: (tags: string[]) => void;
  onClearCache: () => void;
  onTrash: () => void;
  /** (W27) Tells the table which row's menu is open so it stays mounted while scrolling. */
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<"root" | "move" | "tags" | "extensions">("root");

  const openMenu = () => {
    setOpen(true);
    onOpenChange(true);
  };
  const close = () => {
    setOpen(false);
    setMode("root");
    onOpenChange(false);
  };

  return (
    <Popover
      open={open}
      onClose={close}
      align="end"
      label={`${t("table.rowMenu")}: ${profile.name}`}
      trigger={
        <button
          type="button"
          aria-label={`${t("table.rowMenu")}: ${profile.name}`}
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => (open ? close() : openMenu())}
          className="grid h-8 w-8 place-items-center rounded-md text-fg-muted transition-colors hover:bg-surface-2 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
        >
          <EllipsisVertical className="h-4 w-4" aria-hidden="true" />
        </button>
      }
    >
      {mode === "root" ? (
        <>
          <MenuItem
            icon={<Pencil className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
            onClick={() => {
              close();
              onEdit();
            }}
          >
            {t("toolbar.editSelected")}
          </MenuItem>
          <MenuItem
            icon={<PenLine className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
            onClick={() => {
              close();
              onRename();
            }}
          >
            {t("listUtils.rename")}
          </MenuItem>
          <MenuItem
            icon={
              <ClipboardCopy className="h-4 w-4 text-fg-muted" aria-hidden="true" />
            }
            onClick={() => {
              close();
              onCopyId();
            }}
          >
            {t("listUtils.copyId")}
          </MenuItem>
          <span
            className="block"
            title={!running ? t("listUtils.copyCdpUrlNotRunning") : undefined}
          >
            <MenuItem
              disabled={!running}
              icon={<Link2 className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
              onClick={() => {
                close();
                onCopyCdpUrl();
              }}
            >
              {t("listUtils.copyCdpUrl")}
            </MenuItem>
          </span>
          <span
            className="block"
            title={!running ? t("listUtils.bringToFrontNotRunning") : undefined}
          >
            <MenuItem
              disabled={!running}
              icon={<MonitorUp className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
              onClick={() => {
                close();
                onBringToFront();
              }}
            >
              {t("listUtils.bringToFront")}
            </MenuItem>
          </span>
          <MenuItem
            icon={<Copy className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
            onClick={() => {
              close();
              onClone();
            }}
          >
            {t("toolbar.clone")}
          </MenuItem>
          <span
            className="block"
            title={running ? t("exchange.exportRunning") : undefined}
          >
            <MenuItem
              disabled={running}
              icon={<Download className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
              onClick={() => {
                close();
                onExport();
              }}
            >
              {t("exchange.export")}
            </MenuItem>
          </span>
          <MenuItem
            icon={<Cookie className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
            onClick={() => {
              close();
              onExportCookies();
            }}
          >
            {t("cookies.menuExport")}
          </MenuItem>
          <MenuItem
            icon={<Cookie className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
            onClick={() => {
              close();
              onImportCookies();
            }}
          >
            {t("cookies.menuImport")}
          </MenuItem>
          <MenuItem
            icon={<Bot className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
            onClick={() => {
              close();
              onCookieRobot();
            }}
          >
            {t("robot.menu")}
          </MenuItem>
          <MenuItem
            icon={<FolderInput className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
            onClick={() => setMode("move")}
          >
            {t("toolbar.moveToFolder")}
          </MenuItem>
          <MenuItem
            icon={<Tag className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
            onClick={() => setMode("tags")}
          >
            {t("toolbar.addTags")}
          </MenuItem>
          <MenuItem
            icon={<Puzzle className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
            onClick={() => setMode("extensions")}
          >
            {t("ext.rowMenu")}
          </MenuItem>
          <span
            className="block"
            title={running ? t("table.clearCacheRunning") : undefined}
          >
            <MenuItem
              disabled={running}
              icon={<Eraser className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
              onClick={() => {
                close();
                onClearCache();
              }}
            >
              {t("table.clearCache")}
            </MenuItem>
          </span>
          <MenuItem
            danger
            icon={<Trash2 className="h-4 w-4" aria-hidden="true" />}
            onClick={() => {
              close();
              onTrash();
            }}
          >
            {t("toolbar.trash")}
          </MenuItem>
        </>
      ) : mode === "move" ? (
        <FolderPanel
          folders={folders}
          onPick={(folderId) => {
            close();
            onMove(folderId);
          }}
        />
      ) : mode === "extensions" ? (
        <ExtensionsPanel profileId={profile.id} onDone={close} />
      ) : (
        <TagPanel
          onApply={(tags) => {
            close();
            onAddTags(tags);
          }}
        />
      )}
    </Popover>
  );
}

export function ProfileTable({
  rows,
  folders,
  runningIds,
  crashedIds,
  selected,
  sizes,
  onToggleRow,
  onTogglePage,
  sort,
  onSortChange,
  columns,
  onColumnsChange,
  onToggleFavorite,
  onLaunch,
  onStop,
  onEdit,
  onClone,
  onExport,
  onExportCookies,
  onImportCookies,
  onCookieRobot,
  onMove,
  onAddTags,
  onClearCache,
  onTrash,
  renamingId,
  onRenameStart,
  onRenameSubmit,
  onRenameCancel,
  onCopyId,
  onCopyCdpUrl,
  onBringToFront,
}: ProfileTableProps) {
  const { t, i18n } = useTranslation();
  const [pickerOpen, setPickerOpen] = useState(false);
  // (W27) Row whose kebab menu is currently open (kept mounted during scroll).
  const [menuOpenId, setMenuOpenId] = useState<string | null>(null);

  // (W26c) Manual windowing: only mount ~viewport+overscan rows. The scroll
  // container is owned here so the slice tracks the real scroll element.
  const scrollRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportH, setViewportH] = useState(0);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const measure = () => setViewportH(el.clientHeight);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Small lists (or unmeasured container, e.g. tests) render fully as before.
  let start = 0;
  let end = rows.length;
  if (viewportH > 0 && rows.length * ROW_HEIGHT > viewportH) {
    start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
    end = Math.min(
      rows.length,
      Math.ceil((scrollTop + viewportH) / ROW_HEIGHT) + OVERSCAN,
    );
  }
  // Keep the row being renamed mounted: RenameInput submits on blur, so
  // unmounting it mid-scroll would fire a stray submit. (W27) Same for the
  // row whose kebab menu is open — unmounting would silently close the menu.
  for (const pinnedId of [renamingId, menuOpenId]) {
    if (!pinnedId) continue;
    const idx = rows.findIndex((r) => r.id === pinnedId);
    if (idx >= 0) {
      start = Math.min(start, idx);
      end = Math.max(end, idx + 1);
    }
  }
  const visibleRows = rows.slice(start, end);
  const padTop = start * ROW_HEIGHT;
  const padBottom = (rows.length - end) * ROW_HEIGHT;
  // 6 fixed columns (checkbox, favorite, launch, name, column picker, menu).
  const colCount = 6 + ALL_COLUMNS.filter((key) => columns[key]).length;

  const pageIds = rows.map((r) => r.id);
  const allChecked = rows.length > 0 && pageIds.every((id) => selected.has(id));
  const someChecked = pageIds.some((id) => selected.has(id));

  const folderName = (id: string | null) =>
    id
      ? (folders.find((f) => f.id === id)?.name ?? t("toolbar.defaultFolder"))
      : t("toolbar.defaultFolder");

  const toggleNameSort = () =>
    onSortChange({
      key: "name",
      dir: sort.key === "name" && sort.dir === "asc" ? "desc" : "asc",
    });

  const th = "h-10 px-3 text-left align-middle text-xs font-medium text-fg";

  return (
    <div
      ref={scrollRef}
      onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
      className="min-h-0 flex-1 overflow-auto"
    >
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
              onChange={() => onTogglePage(pageIds, !allChecked)}
              className="h-4 w-4 cursor-pointer rounded border-border accent-accent"
            />
          </th>
          <th scope="col" className="w-9 px-1 align-middle">
            <span className="sr-only">{t("table.favorite")}</span>
          </th>
          <th scope="col" className="w-11 px-1 align-middle">
            <span className="sr-only">{t("table.launch")}</span>
          </th>
          <th scope="col" className={th}>
            <button
              type="button"
              onClick={toggleNameSort}
              className="inline-flex items-center gap-1 rounded hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
              aria-sort={
                sort.key === "name"
                  ? sort.dir === "asc"
                    ? "ascending"
                    : "descending"
                  : undefined
              }
            >
              {t("table.profileName")}
              {sort.key === "name" ? (
                sort.dir === "asc" ? (
                  <ArrowUp className="h-3 w-3" aria-hidden="true" />
                ) : (
                  <ArrowDown className="h-3 w-3" aria-hidden="true" />
                )
              ) : (
                <ArrowUpDown className="h-3 w-3 opacity-50" aria-hidden="true" />
              )}
            </button>
          </th>
          {columns.storage && <th scope="col" className={th}>{t("table.storage")}</th>}
          {columns.folder && <th scope="col" className={th}>{t("table.folder")}</th>}
          {columns.notes && <th scope="col" className={th}>{t("table.notes")}</th>}
          {columns.tags && <th scope="col" className={th}>{t("table.tags")}</th>}
          {columns.browser && <th scope="col" className={th}>{t("table.browser")}</th>}
          {columns.os && <th scope="col" className={th}>{t("table.os")}</th>}
          {columns.lastStart && <th scope="col" className={th}>{t("table.lastStart")}</th>}
          <th scope="col" className="w-10 px-1 align-middle">
            <Popover
              open={pickerOpen}
              onClose={() => setPickerOpen(false)}
              align="end"
              label={t("table.columns")}
              trigger={
                <button
                  type="button"
                  aria-label={t("table.columns")}
                  title={t("table.columns")}
                  aria-haspopup="dialog"
                  aria-expanded={pickerOpen}
                  onClick={() => setPickerOpen((v) => !v)}
                  className="grid h-7 w-7 place-items-center rounded-lg text-fg-muted transition-colors hover:bg-surface-2 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                >
                  <Columns3 className="h-4 w-4" aria-hidden="true" />
                </button>
              }
            >
              <div className="p-1">
                {ALL_COLUMNS.map((key) => (
                  <label
                    key={key}
                    className="flex cursor-pointer items-center gap-2 rounded-md px-2.5 py-1.5 text-sm text-fg hover:bg-surface-2"
                  >
                    <input
                      type="checkbox"
                      checked={columns[key]}
                      onChange={() =>
                        onColumnsChange({ ...columns, [key]: !columns[key] })
                      }
                      className="h-4 w-4 rounded border-border accent-accent"
                    />
                    {t(`table.${key === "os" ? "os" : key}`)}
                  </label>
                ))}
              </div>
            </Popover>
          </th>
          <th scope="col" className="w-10 px-1 align-middle">
            <span className="sr-only">{t("table.rowMenu")}</span>
          </th>
        </tr>
      </thead>
      <tbody>
        {padTop > 0 && (
          <tr aria-hidden="true">
            <td colSpan={colCount} className="p-0" style={{ height: padTop }} />
          </tr>
        )}
        {visibleRows.map((p) => {
          const running = runningIds.has(p.id);
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
                  onChange={() => onToggleRow(p.id)}
                  className="h-4 w-4 cursor-pointer rounded border-border accent-accent"
                />
              </td>
              <td className="px-1 py-2">
                <button
                  type="button"
                  aria-label={`${p.favorite ? t("table.unfavorite") : t("table.favorite")}: ${p.name}`}
                  aria-pressed={p.favorite}
                  onClick={() => onToggleFavorite(p)}
                  className="grid h-8 w-8 place-items-center rounded-full bg-transparent text-fg-muted transition-colors hover:bg-surface-2 hover:text-[#F5A623] focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                >
                  <Star
                    className={`h-4 w-4 ${p.favorite ? "fill-[#F5A623] text-[#F5A623]" : ""}`}
                    aria-hidden="true"
                  />
                </button>
              </td>
              <td className="px-1 py-2">
                <RowPlayButton
                  name={p.name}
                  running={running}
                  onLaunch={() => onLaunch(p.id)}
                  onStop={() => onStop(p.id)}
                />
              </td>
              <td className="max-w-0 px-3 py-2">
                {renamingId === p.id ? (
                  <RenameInput
                    initial={p.name}
                    onSubmit={(name) => onRenameSubmit(p.id, name)}
                    onCancel={onRenameCancel}
                  />
                ) : (
                  <span
                    className="inline-flex max-w-full items-center gap-1.5 text-fg"
                    onDoubleClick={() => onRenameStart(p.id)}
                  >
                    <span className="truncate" title={p.name}>{p.name}</span>
                    {running && (
                      <>
                        <span
                          className="h-1.5 w-1.5 shrink-0 rounded-full bg-success"
                          aria-hidden="true"
                        />
                        <span className="sr-only">{t("table.running")}</span>
                      </>
                    )}
                    {!running && crashedIds.has(p.id) && (
                      <span className="shrink-0 rounded bg-danger/10 px-1.5 py-0.5 text-[10px] font-semibold text-danger">
                        {t("table.crashed")}
                      </span>
                    )}
                  </span>
                )}
              </td>
              {columns.storage && (
                <td className="whitespace-nowrap px-3 py-2 text-fg-muted">
                  <span
                    className="inline-flex items-center gap-1.5"
                    title={t("table.local")}
                  >
                    <HardDrive className="h-4 w-4" aria-hidden="true" />
                    {sizes[p.id] !== undefined ? formatBytes(sizes[p.id] ?? 0) : "—"}
                    <span className="sr-only">{t("table.local")}</span>
                  </span>
                </td>
              )}
              {columns.folder && (
                <td
                  className="max-w-[14rem] truncate px-3 py-2 text-fg-muted"
                  title={folderName(p.folder_id)}
                >
                  {folderName(p.folder_id)}
                </td>
              )}
              {columns.notes && (
                <td className="max-w-[14rem] truncate px-3 py-2 text-fg-muted" title={p.notes ?? undefined}>
                  {p.notes ?? ""}
                </td>
              )}
              {columns.tags && (
                <td className="px-3 py-2">
                  <span
                    className="flex max-w-[14rem] items-center gap-1 overflow-hidden"
                    title={p.tags.join(", ") || undefined}
                  >
                    {p.tags.map((tag) => (
                      <span
                        key={tag}
                        className="whitespace-nowrap rounded-full bg-surface-2 px-2 py-0.5 text-[11px] text-fg-muted"
                      >
                        {tag}
                      </span>
                    ))}
                  </span>
                </td>
              )}
              {columns.browser && (
                <td className="px-3 py-2">
                  <span title="Chromium">
                    <ChromiumIcon className="h-4 w-4 text-fg-muted" />
                    <span className="sr-only">Chromium</span>
                  </span>
                </td>
              )}
              {columns.os && (
                <td className="px-3 py-2">
                  <span className="inline-flex items-center" title={OS_LABEL[p.platform]}>
                    <OsIcon platform={p.platform} />
                    <span className="sr-only">{OS_LABEL[p.platform]}</span>
                  </span>
                </td>
              )}
              {columns.lastStart && (
                <td
                  className="whitespace-nowrap px-3 py-2 text-fg-muted"
                  title={
                    p.last_start_at
                      ? new Date(p.last_start_at).toLocaleString(i18n.language)
                      : undefined
                  }
                >
                  {p.last_start_at
                    ? relativeTime(p.last_start_at, i18n.language)
                    : "—"}
                </td>
              )}
              <td className="px-1 py-2" />
              <td className="px-1 py-2">
                <RowMenu
                  profile={p}
                  folders={folders}
                  running={running}
                  onEdit={() => onEdit(p)}
                  onRename={() => onRenameStart(p.id)}
                  onCopyId={() => onCopyId(p.id)}
                  onCopyCdpUrl={() => onCopyCdpUrl(p.id)}
                  onBringToFront={() => onBringToFront(p.id)}
                  onClone={() => onClone(p)}
                  onExport={() => onExport(p)}
                  onExportCookies={() => onExportCookies(p)}
                  onImportCookies={() => onImportCookies(p)}
                  onCookieRobot={() => onCookieRobot(p)}
                  onMove={(folderId) => onMove([p.id], folderId)}
                  onAddTags={(tags) => onAddTags([p.id], tags)}
                  onClearCache={() => onClearCache([p.id])}
                  onTrash={() => onTrash([p.id])}
                  onOpenChange={(o) => setMenuOpenId(o ? p.id : null)}
                />
              </td>
            </tr>
          );
        })}
        {padBottom > 0 && (
          <tr aria-hidden="true">
            <td colSpan={colCount} className="p-0" style={{ height: padBottom }} />
          </tr>
        )}
      </tbody>
    </table>
    </div>
  );
}
