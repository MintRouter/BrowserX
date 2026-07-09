import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  EngineSetup,
  initialEngineState,
  type EngineState,
} from "./components/EngineSetup";
import { CloudSyncView } from "./components/CloudSyncView";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ExtensionsView } from "./components/ExtensionsView";
import { ProfileForm } from "./components/ProfileForm";
import { ProfileList } from "./components/ProfileList";
import { ProxiesView, type ProxyPatch } from "./components/ProxiesView";
import { ProxyTemplatesView } from "./components/ProxyTemplatesView";
import { QuickProfileModal } from "./components/QuickProfileModal";
import { QuickStopDialog } from "./components/QuickStopDialog";
import { QuitDialog } from "./components/QuitDialog";
import { SettingsView } from "./components/SettingsView";
import { Sidebar, type MainView } from "./components/Sidebar";
import { TemplatesView } from "./components/TemplatesView";
import { TopBar } from "./components/TopBar";
import { TrashView } from "./components/TrashView";
import {
  api,
  DEFAULT_TEMPLATE_SETTING,
  isTauri,
  onBinaryProgress,
  onExitRequested,
  onProfileStatus,
  type CloudBackupInfo,
  type Extension,
  type Folder,
  type Profile,
  type ProfileInput,
  type ProfileTemplate,
  type Proxy,
  type ProxyInput,
  type ProxyTemplate,
  type ProxyTemplateCreate,
  type ProxyTemplatePatch,
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
  const [proxyTemplates, setProxyTemplates] = useState<ProxyTemplate[]>([]);
  const [running, setRunning] = useState<RunningSession[]>([]);
  const [trash, setTrash] = useState<Profile[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [templates, setTemplates] = useState<ProfileTemplate[]>([]);
  const [extensions, setExtensions] = useState<Extension[]>([]);
  /** (W51-B2) Telegram cloud backups — feeds the "Cloud sync profiles" view/count. */
  const [cloudBackups, setCloudBackups] = useState<CloudBackupInfo[]>([]);
  const [settings, setSettings] = useState<Record<string, string> | null>(null);
  const [editing, setEditing] = useState<Profile | "new" | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [activeFolderId, setActiveFolderId] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  /** Quick profiles awaiting the stop confirmation dialog (null = closed). */
  const [quickStop, setQuickStop] = useState<string[] | null>(null);
  /** (W50G) MLX-parity "Create quick profile" modal. */
  const [quickModalOpen, setQuickModalOpen] = useState(false);
  /** (W47) Profiles awaiting the move-to-trash confirmation (null = closed). */
  const [trashConfirm, setTrashConfirm] = useState<string[] | null>(null);
  const [quickStopBusy, setQuickStopBusy] = useState(false);
  /** (W23a) Running-session count when quit was requested (null = no dialog). */
  const [quitCount, setQuitCount] = useState<number | null>(null);
  const [quitBusy, setQuitBusy] = useState(false);
  /** (W23a) Profiles whose session died unexpectedly (cleared on relaunch). */
  const [crashedIds, setCrashedIds] = useState<Set<string>>(new Set());
  const [crashBannerDismissed, setCrashBannerDismissed] = useState(false);
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

  // (W23d) Per-resource loaders — mutation handlers refetch only the
  // resources they touched instead of reloading everything on every action.
  const refetchProfiles = useCallback(async () => {
    setProfiles(await api.listProfiles());
  }, []);
  const refetchProxies = useCallback(async () => {
    setProxies(await api.listProxies());
  }, []);
  const refetchProxyTemplates = useCallback(async () => {
    setProxyTemplates(await api.listProxyTemplates());
  }, []);
  const refetchRunning = useCallback(async () => {
    setRunning(await api.listRunning());
  }, []);
  const refetchFolders = useCallback(async () => {
    setFolders(await api.listFolders());
  }, []);
  const refetchTrash = useCallback(async () => {
    setTrash(await api.listTrash());
  }, []);
  const refetchTemplates = useCallback(async () => {
    setTemplates(await api.listTemplates());
  }, []);
  const refetchExtensions = useCallback(async () => {
    setExtensions(await api.listExtensions());
  }, []);
  const refetchCloudBackups = useCallback(async () => {
    setCloudBackups(await api.listCloudBackups());
  }, []);

  /** Await the given refetches; a failed one keeps the last known state. */
  const refetch = (...tasks: Promise<void>[]) => Promise.allSettled(tasks);

  /** Full reload — only for the initial mount and the manual Refresh button. */
  const loadAll = useCallback(async () => {
    if (!isTauri()) return;
    const [p, x, r] = await Promise.allSettled([
      refetchProfiles(),
      refetchProxies(),
      refetchRunning(),
      refetchFolders(),
      refetchTrash(),
      refetchTemplates(),
      refetchExtensions(),
      refetchProxyTemplates(),
      refetchCloudBackups(),
      api.getSettings().then(setSettings),
    ]);
    setLoadError([p, x, r].some((res) => res.status === "rejected"));
  }, [refetchProfiles, refetchProxies, refetchRunning, refetchFolders, refetchTrash, refetchTemplates, refetchExtensions, refetchProxyTemplates, refetchCloudBackups]);

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

  // (W23b) Once per app open: backend checks the master-key-check blob and
  // logs a warning when the key has changed (stale proxy credentials).
  useEffect(() => {
    if (!isTauri()) return;
    api.masterKeyStatus().catch(() => {});
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    onProfileStatus((e) => {
      // (W23a) Track crashes: flag on "crashed", clear when the profile
      // launches again. A normal user-initiated stop never sets the flag.
      if (e.status === "crashed") {
        setCrashedIds((prev) => {
          if (prev.has(e.profile_id)) return prev;
          const next = new Set(prev);
          next.add(e.profile_id);
          return next;
        });
        setCrashBannerDismissed(false);
      } else if (e.status === "starting" || e.status === "running") {
        setCrashedIds((prev) => {
          if (!prev.has(e.profile_id)) return prev;
          const next = new Set(prev);
          next.delete(e.profile_id);
          return next;
        });
      }
      api.listRunning().then(setRunning).catch(() => {});
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => unlisten?.();
  }, []);

  // (W23a) Backend blocked a quit because sessions are running — confirm first.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    onExitRequested((e) => {
      setQuitCount(e.count);
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
    // (W50E) Running is the same table filtered to live sessions (MLX parity).
    if (view === "running")
      return profiles.filter((p) => runningIds.has(p.id));
    if (activeFolderId)
      return profiles.filter((p) => p.folder_id === activeFolderId);
    return profiles;
  }, [profiles, favoriteProfiles, view, activeFolderId, runningIds]);

  const sidebarFolders = useMemo(
    () => folders.map((f) => ({ id: f.id, name: f.name, count: f.profile_count })),
    [folders],
  );

  // (W51-B2) Profiles with at least one Telegram cloud backup.
  const cloudSyncCount = useMemo(
    () => new Set(cloudBackups.map((b) => b.profile_id)).size,
    [cloudBackups],
  );

  const sidebarCounts = useMemo(
    () => ({
      all: profiles.length,
      running: running.length,
      cloudSync: cloudSyncCount,
      favorites: favoriteProfiles.length,
      trash: trash.length,
      proxies: proxies.length,
      proxyTemplates: proxyTemplates.length,
      templates: templates.length,
      extensions: extensions.length,
    }),
    [profiles.length, running.length, cloudSyncCount, favoriteProfiles.length, trash.length, proxies.length, proxyTemplates.length, templates.length, extensions.length],
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
      await refetchFolders();
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
    await refetch(refetchProfiles(), refetchFolders());
  };

  const handleDeleteProfile = async () => {
    if (editing && editing !== "new") {
      await api.deleteProfile(editing.id);
      setEditing(null);
      await refetch(refetchProfiles(), refetchFolders());
    }
  };

  const handleLaunch = async (id: string) => {
    if (engineBusy) {
      setActionError(t("engine.notReadyLaunch"));
      return;
    }
    try {
      await api.launchProfile(id);
      await refetchRunning();
    } catch (err) {
      setActionError(errMsg(err));
    }
  };

  const handleStop = async (id: string) => {
    if (profiles.find((p) => p.id === id)?.is_quick) {
      setQuickStop([id]);
      return;
    }
    try {
      await api.stopProfile(id);
      await refetchRunning();
    } catch (err) {
      setActionError(errMsg(err));
    }
  };

  const profileName = (id: string) =>
    profiles.find((p) => p.id === id)?.name ?? id;

  // (W23a) Confirm quit: stop every session (full cleanup) then exit the app.
  const handleStopAllQuit = async () => {
    setQuitBusy(true);
    try {
      await api.stopAllAndQuit();
    } catch {
      // The app exits during the invoke — a rejected promise here is expected.
    } finally {
      setQuitBusy(false);
      setQuitCount(null);
    }
  };

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
      await refetchRunning();
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
      await refetchRunning();
    } catch {
      // keep last known running state
    }
    setActionError(errors.length ? errors.join(" · ") : null);
    if (quick.length > 0) setQuickStop(quick);
  };

  // (W39) Rotate the assigned proxy for every selected profile. The backend
  // rejects profiles without a pool or with no healthy candidate — surface
  // that as an action error, then refresh assignments + health flags.
  const handleRotateProxies = async () => {
    setActionError(null);
    try {
      await api.rotateProxies([...selected]);
      // (W41) Rotation only takes effect on the next launch — tell the user
      // when at least one selected profile is currently running.
      if ([...selected].some((id) => runningIds.has(id))) {
        setActionError(t("toolbar.rotateProxyAppliesNextLaunch"));
      }
    } catch (err) {
      setActionError(errMsg(err));
    }
    await refetch(refetchProfiles(), refetchProxies());
  };

  // (W40) Rotate the assigned proxy for a single profile (row ⋮ menu).
  const handleRotateProxy = async (p: Profile) => {
    setActionError(null);
    try {
      await api.rotateProxy(p.id);
      // (W41) Same next-launch notice as handleRotateProxies.
      if (runningIds.has(p.id)) {
        setActionError(t("toolbar.rotateProxyAppliesNextLaunch"));
      }
    } catch (err) {
      setActionError(errMsg(err));
    }
    await refetch(refetchProfiles(), refetchProxies());
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
    await refetch(refetchProfiles(), refetchRunning());
  };

  const nextQuickName = () => {
    let n = 1;
    for (const p of profiles) {
      const m = /^Quick (\d+)$/.exec(p.name);
      if (m) n = Math.max(n, Number(m[1]) + 1);
    }
    return `Quick ${n}`;
  };

  // (W50G) ⚡Quick now opens the MLX-parity modal instead of creating directly.
  const handleQuickProfile = () => {
    if (engineBusy) {
      setActionError(t("engine.notReadyLaunch"));
      return;
    }
    setActionError(null);
    setQuickModalOpen(true);
  };

  /** (W50G) Quick modal "Start": create `count` quick profiles + launch them.
   *  (W56) Masked screen mode: each profile gets its own random seed + a
   *  suggested GPU/screen from it, so N quick profiles differ from each other. */
  const handleQuickStart = async (
    input: Omit<ProfileInput, "name">,
    count: number,
    maskedFingerprint: boolean,
  ) => {
    let n = Number(/^Quick (\d+)$/.exec(nextQuickName())?.[1] ?? 1);
    try {
      for (let i = 0; i < count; i++) {
        let fp: Partial<ProfileInput> = {};
        if (maskedFingerprint) {
          const seed = String(Math.floor(Math.random() * 90000) + 10000);
          fp = { fingerprint_seed: seed };
          try {
            const s = await api.suggestFingerprint(
              input.platform ?? "windows",
              seed,
            );
            fp = {
              ...fp,
              screen_width: s.screen_width,
              screen_height: s.screen_height,
              gpu_vendor: s.gpu?.vendor ?? null,
              gpu_renderer: s.gpu?.renderer ?? null,
            };
          } catch {
            // non-Tauri / suggest failure: keep the modal's default dims
          }
        }
        const created = await api.createProfile({
          ...input,
          ...fp,
          name: `Quick ${n + i}`,
        });
        await api.launchProfile(created.id);
      }
    } finally {
      await refetch(refetchProfiles(), refetchRunning());
    }
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
        extensions: p.extensions,
      });
      await refetch(refetchProfiles());
    } catch (err) {
      setActionError(errMsg(err));
    }
  };

  const handleTrash = async (ids: string[]) => {
    if (ids.length === 0) return;
    setTrashConfirm(ids);
  };

  // (W47) window.confirm() is a no-op in the Tauri WKWebView — the trash
  // action runs from the in-app ConfirmDialog instead.
  const confirmTrash = async () => {
    const ids = trashConfirm;
    setTrashConfirm(null);
    if (!ids) return;
    setActionError(null);
    try {
      await api.trashProfiles(ids);
      setSelected(new Set());
      await refetch(refetchProfiles(), refetchFolders(), refetchTrash());
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
      await refetch(refetchProfiles(), refetchFolders());
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
    await refetch(refetchProfiles());
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
    setActionError(null);
    try {
      await api.restoreProfiles(ids);
      await refetch(refetchProfiles(), refetchFolders(), refetchTrash());
    } catch (err) {
      setActionError(errMsg(err));
    }
  };

  const handlePurge = async (ids: string[]) => {
    setActionError(null);
    try {
      await api.purgeProfiles(ids);
      await refetch(refetchTrash());
    } catch (err) {
      setActionError(errMsg(err));
    }
  };

  const handleCreateProxy = async (input: ProxyInput) => {
    await api.createProxy(input);
    await refetchProxies();
  };

  // (W23b) Typing a password re-encrypts with the current master key;
  // clear_credentials wipes stale blobs so the invalid flag can't linger.
  const handleUpdateProxy = async (id: string, patch: ProxyPatch) => {
    await api.updateProxy(id, patch);
    await refetchProxies();
  };

  const handleDeleteProxies = async (ids: string[]) => {
    for (const id of ids) await api.deleteProxy(id);
    await refetchProxies();
  };

  // (P3-3b) Proxy-template CRUD — same credential semantics as proxies.
  const handleCreateProxyTemplate = async (input: ProxyTemplateCreate) => {
    await api.createProxyTemplate(input);
    await refetchProxyTemplates();
  };

  const handleUpdateProxyTemplate = async (
    id: string,
    patch: ProxyTemplatePatch,
  ) => {
    await api.updateProxyTemplate(id, patch);
    await refetchProxyTemplates();
  };

  const handleDeleteProxyTemplates = async (ids: string[]) => {
    setActionError(null);
    try {
      for (const id of ids) await api.deleteProxyTemplate(id);
      await refetchProxyTemplates();
    } catch (err) {
      setActionError(errMsg(err));
    }
  };

  // (F2b) Template CRUD + default-template setting.
  const handleCreateTemplate = async (name: string, config: ProfileInput) => {
    await api.saveAsTemplate(name, config);
    await refetchTemplates();
  };

  const handleUpdateTemplate = async (
    id: string,
    name: string,
    config: ProfileInput,
  ) => {
    await api.updateTemplate(id, name, config);
    await refetchTemplates();
  };

  const handleDeleteTemplates = async (ids: string[]) => {
    setActionError(null);
    try {
      for (const id of ids) await api.deleteTemplate(id);
      // Deleting the default template clears the setting too.
      if (ids.includes(settings?.[DEFAULT_TEMPLATE_SETTING] ?? "")) {
        await api.setSetting(DEFAULT_TEMPLATE_SETTING, "");
        setSettings((s) => (s ? { ...s, [DEFAULT_TEMPLATE_SETTING]: "" } : s));
      }
      await refetchTemplates();
    } catch (err) {
      setActionError(errMsg(err));
    }
  };

  // (W29a) Bulk create N profiles từ template — refetch profiles + folder
  // counts như onImported (W23d); dialog trong TemplatesView tự hiện lỗi.
  const handleBulkCreateFromTemplate = async (
    templateId: string,
    count: number,
    namePrefix: string | null,
  ) => {
    await api.createProfilesFromTemplate(templateId, count, namePrefix);
    await refetch(refetchProfiles(), refetchFolders());
  };

  const handleSetDefaultTemplate = async (id: string) => {
    setActionError(null);
    try {
      await api.setSetting(DEFAULT_TEMPLATE_SETTING, id);
      setSettings((s) => (s ? { ...s, [DEFAULT_TEMPLATE_SETTING]: id } : s));
    } catch (err) {
      setActionError(errMsg(err));
    }
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
          {crashedIds.size > 0 && !crashBannerDismissed && (
            <div
              className="flex items-center justify-between gap-2 px-4 py-2 bg-danger/10 border-b border-danger/30"
              role="alert"
            >
              <p className="text-danger text-xs">
                {t("crash.banner", {
                  names: [...crashedIds].map(profileName).join(", "),
                })}
              </p>
              <button
                type="button"
                className="text-xs font-medium text-danger underline shrink-0"
                onClick={() => setCrashBannerDismissed(true)}
              >
                {t("crash.dismiss")}
              </button>
            </div>
          )}
          {editing !== null ? (
            <ProfileForm
              profile={editing === "new" ? null : editing}
              proxies={proxies}
              folders={folders}
              onSave={handleSaveProfile}
              onDelete={editing !== "new" ? handleDeleteProfile : undefined}
              onCancel={() => setEditing(null)}
              onTrashed={async () => {
                await refetch(refetchProfiles(), refetchFolders(), refetchTrash());
              }}
              onProxiesChanged={refetchProxies}
            />
          ) : view === "profiles" || view === "favorites" || view === "running" ? (
            <ProfileList
              profiles={visibleProfiles}
              folders={folders}
              runningIds={runningIds}
              crashedIds={crashedIds}
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
              onRotateProxies={handleRotateProxies}
              onRefresh={loadAll}
              onImported={async () => {
                await refetch(refetchProfiles(), refetchFolders());
              }}
              onRenamed={refetchProfiles}
              onClone={handleClone}
              onRotateProxy={handleRotateProxy}
              onTrash={handleTrash}
              onMove={handleMove}
              onAddTags={handleAddTags}
              onToggleFavorite={handleToggleFavorite}
            />
          ) : view === "cloudSync" ? (
            <CloudSyncView
              profiles={profiles}
              runningIds={runningIds}
              profileCount={profiles.length}
              settings={settings}
            />
          ) : view === "proxies" ? (
            <ProxiesView
              proxies={proxies}
              profiles={profiles}
              settings={settings}
              onCreate={handleCreateProxy}
              onUpdate={handleUpdateProxy}
              onDelete={handleDeleteProxies}
            />
          ) : view === "proxyTemplates" ? (
            <ProxyTemplatesView
              templates={proxyTemplates}
              profileCount={profiles.length}
              settings={settings}
              onCreate={handleCreateProxyTemplate}
              onUpdate={handleUpdateProxyTemplate}
              onDelete={handleDeleteProxyTemplates}
            />
          ) : view === "templates" ? (
            <TemplatesView
              templates={templates}
              proxies={proxies}
              profileCount={profiles.length}
              settings={settings}
              onCreate={handleCreateTemplate}
              onUpdate={handleUpdateTemplate}
              onDelete={handleDeleteTemplates}
              onSetDefault={handleSetDefaultTemplate}
              onBulkCreate={handleBulkCreateFromTemplate}
            />
          ) : view === "extensions" ? (
            <ExtensionsView
              extensions={extensions}
              profiles={profiles}
              settings={settings}
              onChanged={refetchExtensions}
            />
          ) : view === "settings" ? (
            <SettingsView />
          ) : (
            <TrashView
              items={trash}
              folders={folders}
              profileCount={profiles.length}
              settings={settings}
              onRestore={handleRestore}
              onPurge={handlePurge}
            />
          )}
        </main>
      </div>
      {trashConfirm && (
        <ConfirmDialog
          message={t("toolbar.confirmTrash", { count: trashConfirm.length })}
          confirmLabel={t("toolbar.trash")}
          onConfirm={() => void confirmTrash()}
          onCancel={() => setTrashConfirm(null)}
        />
      )}
      {quickModalOpen && (
        <QuickProfileModal
          templates={templates}
          proxies={proxies}
          onStart={handleQuickStart}
          onClose={() => setQuickModalOpen(false)}
        />
      )}
      {quickStop && (
        <QuickStopDialog
          names={quickStop.map(profileName)}
          busy={quickStopBusy}
          onSaveAsRegular={() => void resolveQuickStop("save")}
          onCloseDelete={() => void resolveQuickStop("delete")}
          onCancel={() => setQuickStop(null)}
        />
      )}
      {quitCount !== null && (
        <QuitDialog
          count={quitCount}
          busy={quitBusy}
          onStopAllQuit={() => void handleStopAllQuit()}
          onCancel={() => setQuitCount(null)}
        />
      )}
    </div>
  );
}
