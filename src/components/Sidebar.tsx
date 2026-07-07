import {
  BookmarkPlus,
  Building2,
  Folder,
  Laptop,
  LayoutGrid,
  Network,
  Plus,
  Puzzle,
  Search,
  Smartphone,
  Star,
  Trash2,
  User,
  Users2,
} from "lucide-react";
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { api, isTauri } from "../lib/api";

/** (W50D) Mobile/Browser toggle — module-level store so ProfileList can react
 * to the sidebar toggle without threading state through App.tsx. */
export type DeviceType = "mobile" | "browser";

let deviceType: DeviceType = "browser";
const deviceListeners = new Set<() => void>();

export function setDeviceType(d: DeviceType) {
  deviceType = d;
  deviceListeners.forEach((l) => l());
}

export function useDeviceType(): DeviceType {
  return useSyncExternalStore(
    (cb) => {
      deviceListeners.add(cb);
      return () => deviceListeners.delete(cb);
    },
    () => deviceType,
  );
}

export type MainView =
  | "profiles"
  | "running"
  | "favorites"
  | "trash"
  | "proxies"
  | "proxyTemplates"
  | "templates"
  | "extensions"
  | "settings";

export interface SidebarFolder {
  id: string;
  name: string;
  count: number;
}

interface SidebarProps {
  view: MainView;
  onNavigate: (view: MainView) => void;
  folders: SidebarFolder[];
  counts: {
    all: number;
    running: number;
    favorites: number;
    trash: number;
    proxies: number;
    proxyTemplates: number;
    templates: number;
    extensions: number;
  };
  activeFolderId: string | null;
  onSelectFolder: (id: string | null) => void;
  onCreateFolder: (name: string) => void;
}

const rowBase =
  "w-full flex items-center gap-2.5 p-2 rounded-md text-sm font-medium text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60";
const rowIdle = "text-[#1D192B] hover:bg-surface-3";
const rowActive = "bg-[#F0F6FF] text-accent";

function NavRow({
  icon,
  label,
  count,
  active,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  count?: number;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={`${rowBase} ${active ? rowActive : rowIdle}`}
    >
      {icon}
      <span className="flex-1 truncate">{label}</span>
      {count !== undefined && (
        <span className={`text-xs tabular-nums ${active ? "text-accent" : "text-fg-muted"}`}>
          {count}
        </span>
      )}
    </button>
  );
}

export function Sidebar({
  view,
  onNavigate,
  folders,
  counts,
  activeFolderId,
  onSelectFolder,
  onCreateFolder,
}: SidebarProps) {
  const { t } = useTranslation();
  const device = useDeviceType();
  const [folderSearch, setFolderSearch] = useState("");
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");

  const visibleFolders = useMemo(() => {
    const q = folderSearch.trim().toLowerCase();
    return q ? folders.filter((f) => f.name.toLowerCase().includes(q)) : folders;
  }, [folders, folderSearch]);

  const commitCreate = () => {
    const name = newName.trim();
    if (name) onCreateFolder(name);
    setNewName("");
    setCreating(false);
  };

  // Templates view keeps the shell sidebar but swaps folders for template nav (F2b).
  if (view === "templates") {
    return (
      <nav
        className="card my-4 ml-4 flex w-[270px] shrink-0 flex-col overflow-y-auto p-3"
        aria-label={t("sidebar.templates")}
      >
        <div className="space-y-0.5">
          <NavRow
            icon={<Laptop className="h-[18px] w-[18px] shrink-0" aria-hidden="true" />}
            label={t("sidebar.allTemplates")}
            count={counts.templates}
            active
            onClick={() => onNavigate("templates")}
          />
        </div>
        <SidebarStats counts={counts} />
      </nav>
    );
  }

  // Extensions view keeps the shell sidebar but swaps folders for extension nav (P3-1b).
  if (view === "extensions") {
    return (
      <nav
        className="card my-4 ml-4 flex w-[270px] shrink-0 flex-col overflow-y-auto p-3"
        aria-label={t("sidebar.extensions")}
      >
        <div className="space-y-0.5">
          <NavRow
            icon={<Puzzle className="h-[18px] w-[18px] shrink-0" aria-hidden="true" />}
            label={t("sidebar.allExtensions")}
            count={counts.extensions}
            active
            onClick={() => onNavigate("extensions")}
          />
        </div>
        <SidebarStats counts={counts} />
      </nav>
    );
  }

  // Proxies + proxy-templates views keep the shell sidebar but swap folders
  // for proxy nav (F2a, P3-3b).
  if (view === "proxies" || view === "proxyTemplates") {
    return (
      <nav
        className="card my-4 ml-4 flex w-[270px] shrink-0 flex-col overflow-y-auto p-3"
        aria-label={t("sidebar.proxies")}
      >
        <div className="space-y-0.5">
          <NavRow
            icon={<Network className="h-[18px] w-[18px] shrink-0" aria-hidden="true" />}
            label={t("sidebar.allProxies")}
            count={counts.proxies}
            active={view === "proxies"}
            onClick={() => onNavigate("proxies")}
          />
          <NavRow
            icon={<BookmarkPlus className="h-[18px] w-[18px] shrink-0" aria-hidden="true" />}
            label={t("sidebar.proxyTemplates")}
            count={counts.proxyTemplates}
            active={view === "proxyTemplates"}
            onClick={() => onNavigate("proxyTemplates")}
          />
        </div>
        <SidebarStats counts={counts} />
      </nav>
    );
  }

  return (
    <nav
      className="card my-4 ml-4 flex w-[270px] shrink-0 flex-col overflow-y-auto p-3"
      aria-label={t("sidebar.folders")}
    >
      <div className="flex h-9 shrink-0 rounded-lg bg-surface-3 p-1" role="group" aria-label={t("sidebar.deviceType")}>
        {(["mobile", "browser"] as const).map((d) => (
          <button
            key={d}
            type="button"
            onClick={() => setDeviceType(d)}
            aria-pressed={device === d}
            className={`h-7 flex-1 rounded-md text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 ${
              device === d
                ? "bg-accent text-white shadow-sm"
                : "text-fg-muted hover:text-fg"
            }`}
          >
            {t(`sidebar.${d}`)}
          </button>
        ))}
      </div>

      <div className="mt-3 space-y-0.5">
        <NavRow
          icon={<LayoutGrid className="h-[18px] w-[18px] shrink-0" aria-hidden="true" />}
          label={t("sidebar.allProfiles")}
          count={counts.all}
          active={view === "profiles" && activeFolderId === null}
          onClick={() => {
            onSelectFolder(null);
            onNavigate("profiles");
          }}
        />
        <NavRow
          icon={<User className="h-[18px] w-[18px] shrink-0" aria-hidden="true" />}
          label={t("sidebar.runningProfiles")}
          count={counts.running}
          active={view === "running"}
          onClick={() => onNavigate("running")}
        />
        <NavRow
          icon={<Star className="h-[18px] w-[18px] shrink-0" aria-hidden="true" />}
          label={t("sidebar.favorites")}
          count={counts.favorites}
          active={view === "favorites"}
          onClick={() => onNavigate("favorites")}
        />
      </div>

      <div className="mt-3 flex items-center gap-2">
        <div className="relative flex-1">
          <input
            type="search"
            value={folderSearch}
            onChange={(e) => setFolderSearch(e.target.value)}
            placeholder={t("sidebar.searchFolders")}
            aria-label={t("sidebar.searchFolders")}
            className="h-9 w-full rounded-md bg-surface-2 pl-3 pr-8 text-sm text-fg placeholder:text-fg-muted focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
          />
          <Search
            className="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-fg-muted"
            aria-hidden="true"
          />
        </div>
        <button
          type="button"
          onClick={() => setCreating(true)}
          aria-label={t("sidebar.newFolder")}
          title={t("sidebar.newFolder")}
          className="grid h-9 w-9 shrink-0 place-items-center rounded-md bg-[#F0F6FF] text-accent transition-colors hover:bg-[#E0EDFF] focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
        >
          <Plus className="h-4 w-4" aria-hidden="true" />
        </button>
      </div>

      {creating && (
        <input
          autoFocus
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") commitCreate();
            if (e.key === "Escape") {
              setNewName("");
              setCreating(false);
            }
          }}
          onBlur={commitCreate}
          placeholder={t("sidebar.folderName")}
          aria-label={t("sidebar.folderName")}
          className="input mt-2 py-1.5 text-xs"
        />
      )}

      <div className="mt-2 space-y-0.5">
        {visibleFolders.map((f) => {
          const active = view === "profiles" && activeFolderId === f.id;
          return (
            <button
              key={f.id}
              type="button"
              onClick={() => {
                onSelectFolder(f.id);
                onNavigate("profiles");
              }}
              aria-current={active ? "page" : undefined}
              className={`${rowBase} ${active ? rowActive : rowIdle}`}
            >
              <Folder
                fill="currentColor"
                strokeWidth={0}
                className={`h-4 w-4 shrink-0 ${
                  active || f.count > 0 ? "text-accent" : "text-fg-muted"
                }`}
                aria-hidden="true"
              />
              <span className="flex-1 truncate">{f.name}</span>
              <span className={`text-xs tabular-nums ${active ? "text-accent" : "text-fg-muted"}`}>
                {f.count}
              </span>
            </button>
          );
        })}
      </div>

      <hr className="my-2 border-border" aria-hidden="true" />
      <NavRow
        icon={<Trash2 className="h-[18px] w-[18px] shrink-0" aria-hidden="true" />}
        label={t("sidebar.trashBin")}
        count={counts.trash}
        active={view === "trash"}
        onClick={() => onNavigate("trash")}
      />

      <SidebarStats counts={counts} />
    </nav>
  );
}

function SidebarStats({ counts }: { counts: SidebarProps["counts"] }) {
  const { t } = useTranslation();
  // (W50D) Concurrency cap from settings → "Profiles x/y" like MLX.
  const [cap, setCap] = useState<string | null>(null);
  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    api
      .getSettings()
      .then((s) => {
        if (cancelled) return;
        setCap(
          s.max_concurrent_profiles ?? s.max_concurrent ?? s.concurrent_cap ?? null,
        );
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);
  return (
    <div className="mt-auto pt-3">
      <hr className="mb-2 border-border" aria-hidden="true" />
      <div className="space-y-0.5">
        <StatRow
          icon={<Users2 className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />}
          label={t("sidebar.statTeam")}
          value="—"
        />
        <StatRow
          icon={<Network className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />}
          label={t("sidebar.statProxy")}
          value={counts.proxies}
        />
        <StatRow
          icon={<Building2 className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />}
          label={t("sidebar.statIspProxies")}
          value="0"
        />
        <StatRow
          icon={<Smartphone className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />}
          label={t("sidebar.statMinutes")}
          value={t("system.na")}
        />
        <StatRow
          icon={<LayoutGrid className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />}
          label={t("sidebar.profilesStat")}
          value={cap !== null ? `${counts.all}/${cap}` : counts.all}
        />
      </div>
    </div>
  );
}

function StatRow({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between px-3 py-1 text-xs">
      <span className="flex items-center gap-2 font-medium text-fg-muted">
        {icon}
        {label}
      </span>
      <span className="tabular-nums text-fg">{value}</span>
    </div>
  );
}
