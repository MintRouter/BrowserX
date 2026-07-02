import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  EngineSetup,
  initialEngineState,
  type EngineState,
} from "./components/EngineSetup";
import { ProfileForm } from "./components/ProfileForm";
import { ProfileList } from "./components/ProfileList";
import { ProxyForm } from "./components/ProxyForm";
import { QuickStopDialog } from "./components/QuickStopDialog";
import { RunningDashboard } from "./components/RunningDashboard";
import { SettingsView } from "./components/SettingsView";
import { Sidebar, type MainView } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { TrashView } from "./components/TrashView";
import {
  api,
  isTauri,
  onBinaryProgress,
  onProfileStatus,
  type Folder,
  type Profile,
  type ProfileInput,
  type Proxy,
  type ProxyInput,
  type RunningSession,
} from "./lib/api";

const errMsg = (err: unknown) =>
  err instanceof Error ? err.message : String(err);

export default function App() {
  const { t } = useTranslation();
  const [view, setView] = useState<MainView>("profiles");
  const [search, setSearch] = useState("");
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [proxies, setProxies] = useState<Proxy[]>([]);
  const [running, setRunning] = useState<RunningSession[]>([]);
  const [trash, setTrash] = useState<Profile[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [settings, setSettings] = useState<Record<string, string> | null>(null);
  const [editing, setEditing] = useState<Profile | "new" | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [activeFolderId, setActiveFolderId] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  /** Quick profiles awaiting the stop confirmation dialog (null = closed). */
  const [quickStop, setQuickStop] = useState<string[] | null>(null);
  const [quickStopBusy, setQuickStopBusy] = useState(false);
  /** First-run browser-engine download state (see EngineSetup). */
  const [engine, setEngine] = useState<EngineState>(() =>
    initialEngineState(!isTauri()),
  );

  /** Engine cache probe/download in flight — hold off launches to avoid concurrent downloads. */
  const engineBusy = engine.status === "checking" || engine.status === "downloading";

  /** Pre-warm the Chromium engine (download on first run) instead of waiting for Launch. */
  const ensureEngine = useCallback(async () => {
    if (!isTauri()) return;
    setEngine((e) => ({ ...e, status: "checking", error: null }));
    try {
      await api.ensureBinary();
      setEngine((e) => ({ ...e, status: "ready", error: null }));
    } catch (err) {
      setEngine((e) => ({ ...e, status: "error", error: errMsg(err) }));
    }
  }, []);

  useEffect(() => {
    void ensureEngine();
  }, [ensureEngine]);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    onBinaryProgress((e) => {
      setEngine((prev) => ({
        ...prev,
        status: e.phase === "done" ? "ready" : "downloading",
        phase: e.phase,
        pct: e.pct,
        downloadedBytes: e.downloadedBytes ?? 0,
        totalBytes: e.totalBytes ?? 0,
        error: null,
      }));
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => unlisten?.();
  }, []);

  const loadAll = useCallback(async () => {
    if (!isTauri()) return;
    const [p, x, r, f, tr, s] = await Promise.allSettled([
      api.listProfiles(),
      api.listProxies(),
      api.listRunning(),
      api.listFolders(),
      api.listTrash(),
      api.getSettings(),
    ]);
    if (p.status === "fulfilled") setProfiles(p.value);
    if (x.status === "fulfilled") setProxies(x.value);
    if (r.status === "fulfilled") setRunning(r.value);
    if (f.status === "fulfilled") setFolders(f.value);
    if (tr.status === "fulfilled") setTrash(tr.value);
    if (s.status === "fulfilled") setSettings(s.value);
    setLoadError([p, x, r].some((res) => res.status === "rejected"));
  }, []);

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    onProfileStatus(() => {
      api.listRunning().then(setRunning).catch(() => {});
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => unlisten?.();
  }, []);

  // Drop selected ids that no longer exist (e.g. after trash/purge).
  useEffect(() => {
    setSelected((prev) => {
      const next = new Set(
        [...prev].filter((id) => profiles.some((p) => p.id === id)),
      );
      return next.size === prev.size ? prev : next;
    });
  }, [profiles]);

  const runningIds = useMemo(
    () => new Set(running.map((s) => s.profile_id)),
    [running],
  );

  const favoriteProfiles = useMemo(
    () => profiles.filter((p) => p.favorite),
    [profiles],
  );

  const visibleProfiles = useMemo(() => {
    if (view === "favorites") return favoriteProfiles;
    if (activeFolderId)
      return profiles.filter((p) => p.folder_id === activeFolderId);
    return profiles;
  }, [profiles, favoriteProfiles, view, activeFolderId]);

  const sidebarFolders = useMemo(
    () => folders.map((f) => ({ id: f.id, name: f.name, count: f.profile_count })),
    [folders],
  );

  const sidebarCounts = useMemo(
    () => ({
      all: profiles.length,
      running: running.length,
      favorites: favoriteProfiles.length,
      trash: trash.length,
    }),
    [profiles.length, running.length, favoriteProfiles.length, trash.length],
  );

  const navigate = (v: MainView) => {
    setView(v);
    setEditing(null);
    setSelected(new Set());
    setActionError(null);
  };

  const selectFolder = (id: string | null) => {
    setActiveFolderId(id);
    setSelected(new Set());
  };

  const handleCreateFolder = async (name: string) => {
    try {
      await api.createFolder(name);
      setFolders(await api.listFolders());
    } catch (err) {
      setActionError(errMsg(err));
    }
  };

  const handleSaveProfile = async (input: ProfileInput) => {
    if (editing && editing !== "new") {
      await api.updateProfile(editing.id, input);
    } else {
      await api.createProfile(input);
    }
    setEditing(null);
    await loadAll();
  };

  const handleDeleteProfile = async () => {
    if (editing && editing !== "new") {
      await api.deleteProfile(editing.id);
      setEditing(null);
      await loadAll();
    }
  };

  const handleLaunch = async (id: string) => {
    if (engineBusy) {
      setActionError(t("engine.notReadyLaunch"));
      return;
    }
    await api.launchProfile(id);
    setRunning(await api.listRunning());
  };

  const handleStop = async (id: string) => {
    if (profiles.find((p) => p.id === id)?.is_quick) {
      setQuickStop([id]);
      return;
    }
    await api.stopProfile(id);
    setRunning(await api.listRunning());
  };

  const profileName = (id: string) =>
    profiles.find((p) => p.id === id)?.name ?? id;

  const handleLaunchSelected = async () => {
    if (engineBusy) {
      setActionError(t("engine.notReadyLaunch"));
      return;
    }
    const errors: string[] = [];
    for (const id of selected) {
      if (runningIds.has(id)) continue;
      try {
        await api.launchProfile(id);
      } catch (err) {
        errors.push(`${profileName(id)}: ${errMsg(err)}`);
      }
    }
    try {
      setRunning(await api.listRunning());
    } catch {
      // keep last known running state
    }
    setActionError(errors.length ? errors.join(" · ") : null);
  };

  const handleStopSelected = async () => {
    const errors: string[] = [];
    const quick: string[] = [];
    for (const id of selected) {
      if (!runningIds.has(id)) continue;
      if (profiles.find((p) => p.id === id)?.is_quick) {
        quick.push(id);
        continue;
      }
      try {
        await api.stopProfile(id);
      } catch (err) {
        errors.push(`${profileName(id)}: ${errMsg(err)}`);
      }
    }
    try {
      setRunning(await api.listRunning());
    } catch {
      // keep last known running state
    }
    setActionError(errors.length ? errors.join(" · ") : null);
    if (quick.length > 0) setQuickStop(quick);
  };

  /** Resolve the quick-stop dialog: keep data as a regular profile or purge everything. */
  const resolveQuickStop = async (action: "save" | "delete") => {
    if (!quickStop) return;
    setQuickStopBusy(true);
    const errors: string[] = [];
    for (const id of quickStop) {
      try {
        if (action === "save") {
          if (runningIds.has(id)) await api.stopProfile(id);
          await api.convertQuickProfile(id);
        } else {
          await api.deleteQuickProfile(id);
        }
      } catch (err) {
        errors.push(`${profileName(id)}: ${errMsg(err)}`);
      }
    }
    setQuickStopBusy(false);
    setQuickStop(null);
    setActionError(errors.length ? errors.join(" · ") : null);
    await loadAll();
  };

  const nextQuickName = () => {
    let n = 1;
    for (const p of profiles) {
      const m = /^Quick (\d+)$/.exec(p.name);
      if (m) n = Math.max(n, Number(m[1]) + 1);
    }
    return `Quick ${n}`;
  };

  const handleQuickProfile = async () => {
    if (engineBusy) {
      setActionError(t("engine.notReadyLaunch"));
      return;
    }
    setActionError(null);
    try {
      const created = await api.createProfile({
        name: nextQuickName(),
        is_quick: true,
      });
      await api.launchProfile(created.id);
    } catch (err) {
      setActionError(errMsg(err));
    }
    await loadAll();
  };

  const handleClone = async (p: Profile) => {
    setActionError(null);
    try {
      await api.createProfile({
        name: `${p.name} (copy)`,
        fingerprint_seed: p.fingerprint_seed,
        platform: p.platform,
        timezone: p.timezone,
        locale: p.locale,
        screen_width: p.screen_width,
        screen_height: p.screen_height,
        gpu_vendor: p.gpu_vendor,
        gpu_renderer: p.gpu_renderer,
        hardware_concurrency: p.hardware_concurrency,
        humanize: p.humanize,
        human_preset: p.human_preset,
        headless: p.headless,
        geoip: p.geoip,
        color_scheme: p.color_scheme,
        launch_args: p.launch_args,
        notes: p.notes,
        proxy_id: p.proxy_id,
        tags: p.tags,
        store_history: p.store_history,
        store_passwords: p.store_passwords,
        store_sw_cache: p.store_sw_cache,
      });
      await loadAll();
    } catch (err) {
      setActionError(errMsg(err));
    }
  };

  const handleTrash = async (ids: string[]) => {
    if (ids.length === 0) return;
    if (!confirm(t("toolbar.confirmTrash", { count: ids.length }))) return;
    setActionError(null);
    try {
      await api.trashProfiles(ids);
      setSelected(new Set());
      await loadAll();
    } catch (err) {
      setActionError(errMsg(err));
    }
  };

  const handleMove = async (ids: string[], folderId: string | null) => {
    if (ids.length === 0) return;
    setActionError(null);
    try {
      await api.moveProfilesToFolder(ids, folderId);
      setSelected(new Set());
      await loadAll();
    } catch (err) {
      setActionError(errMsg(err));
    }
  };

  const handleAddTags = async (ids: string[], tags: string[]) => {
    if (ids.length === 0 || tags.length === 0) return;
    const errors: string[] = [];
    for (const id of ids) {
      const p = profiles.find((x) => x.id === id);
      if (!p) continue;
      const merged = [...new Set([...p.tags, ...tags])];
      try {
        await api.setProfileTags(id, merged);
      } catch (err) {
        errors.push(`${p.name}: ${errMsg(err)}`);
      }
    }
    setActionError(errors.length ? errors.join(" · ") : null);
    await loadAll();
  };

  const handleToggleFavorite = async (p: Profile) => {
    const next = !p.favorite;
    setProfiles((prev) =>
      prev.map((x) => (x.id === p.id ? { ...x, favorite: next } : x)),
    );
    try {
      await api.setFavorite(p.id, next);
    } catch (err) {
      setProfiles((prev) =>
        prev.map((x) => (x.id === p.id ? { ...x, favorite: p.favorite } : x)),
      );
      setActionError(errMsg(err));
    }
  };

  const handleRestore = async (ids: string[]) => {
    await api.restoreProfiles(ids);
    await loadAll();
  };

  const handlePurge = async (ids: string[]) => {
    await api.purgeProfiles(ids);
    await loadAll();
  };

  const handleCreateProxy = async (input: ProxyInput) => {
    await api.createProxy(input);
    setProxies(await api.listProxies());
  };

  const handleDeleteProxy = async (id: string) => {
    await api.deleteProxy(id);
    setProxies(await api.listProxies());
  };

  return (
    <div className="flex flex-col h-full">
      <TopBar
        search={search}
        onSearchChange={setSearch}
        onNewProfile={() => setEditing("new")}
        view={view}
        onNavigate={navigate}
      />
      <div className="flex flex-1 min-h-0">
        <Sidebar
          view={view}
          onNavigate={navigate}
          folders={sidebarFolders}
          counts={sidebarCounts}
          activeFolderId={activeFolderId}
          onSelectFolder={selectFolder}
          onCreateFolder={(name) => void handleCreateFolder(name)}
        />
        <main className="flex-1 min-w-0 overflow-auto bg-surface-0">
          <EngineSetup engine={engine} onRetry={() => void ensureEngine()} />
          {loadError && (
            <p className="text-warning text-xs px-4 py-2 bg-warning/10 border-b border-warning/30" role="alert">
              {t("errors.loadFailed")}
            </p>
          )}
          {actionError && (
            <p className="text-danger text-xs px-4 py-2 bg-danger/10 border-b border-danger/30" role="alert">
              {actionError}
            </p>
          )}
          {editing !== null ? (
            <ProfileForm
              profile={editing === "new" ? null : editing}
              proxies={proxies}
              folders={folders}
              onSave={handleSaveProfile}
              onDelete={editing !== "new" ? handleDeleteProfile : undefined}
              onCancel={() => setEditing(null)}
            />
          ) : view === "profiles" || view === "favorites" ? (
            <ProfileList
              profiles={visibleProfiles}
              folders={folders}
              runningIds={runningIds}
              search={search}
              onSearchChange={setSearch}
              selected={selected}
              onSelectedChange={setSelected}
              settings={settings}
              onNewProfile={() => setEditing("new")}
              onQuickProfile={handleQuickProfile}
              onEdit={(p) => setEditing(p)}
              onLaunch={handleLaunch}
              onStop={handleStop}
              onLaunchSelected={handleLaunchSelected}
              onStopSelected={handleStopSelected}
              onRefresh={loadAll}
              onClone={handleClone}
              onTrash={handleTrash}
              onMove={handleMove}
              onAddTags={handleAddTags}
              onToggleFavorite={handleToggleFavorite}
            />
          ) : view === "running" ? (
            <RunningDashboard
              sessions={running}
              profiles={profiles}
              onStop={handleStop}
            />
          ) : view === "proxies" ? (
            <ProxyForm
              proxies={proxies}
              onCreate={handleCreateProxy}
              onDelete={handleDeleteProxy}
            />
          ) : view === "settings" ? (
            <SettingsView />
          ) : (
            <TrashView
              items={trash}
              onRestore={handleRestore}
              onPurge={handlePurge}
            />
          )}
        </main>
      </div>
      {quickStop && (
        <QuickStopDialog
          names={quickStop.map(profileName)}
          busy={quickStopBusy}
          onSaveAsRegular={() => void resolveQuickStop("save")}
          onCloseDelete={() => void resolveQuickStop("delete")}
          onCancel={() => setQuickStop(null)}
        />
      )}
    </div>
  );
}
