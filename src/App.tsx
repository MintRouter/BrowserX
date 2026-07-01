import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ProfileForm } from "./components/ProfileForm";
import { ProfileList } from "./components/ProfileList";
import { ProxyForm } from "./components/ProxyForm";
import { RunningDashboard } from "./components/RunningDashboard";
import { Sidebar, type MainView } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import {
  api,
  isTauri,
  onProfileStatus,
  type Profile,
  type ProfileInput,
  type Proxy,
  type ProxyInput,
  type RunningSession,
} from "./lib/api";

export default function App() {
  const { t } = useTranslation();
  const [view, setView] = useState<MainView>("profiles");
  const [search, setSearch] = useState("");
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [proxies, setProxies] = useState<Proxy[]>([]);
  const [running, setRunning] = useState<RunningSession[]>([]);
  const [editing, setEditing] = useState<Profile | "new" | null>(null);
  const [loadError, setLoadError] = useState(false);

  const loadAll = useCallback(async () => {
    if (!isTauri()) return;
    const [p, x, r] = await Promise.allSettled([
      api.listProfiles(),
      api.listProxies(),
      api.listRunning(),
    ]);
    if (p.status === "fulfilled") setProfiles(p.value);
    if (x.status === "fulfilled") setProxies(x.value);
    if (r.status === "fulfilled") setRunning(r.value);
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

  const runningIds = useMemo(
    () => new Set(running.map((s) => s.profile_id)),
    [running],
  );

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
    await api.launchProfile(id);
    setRunning(await api.listRunning());
  };

  const handleStop = async (id: string) => {
    await api.stopProfile(id);
    setRunning(await api.listRunning());
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
      />
      <div className="flex flex-1 min-h-0">
        <Sidebar
          view={view}
          runningCount={running.length}
          onNavigate={(v) => {
            setView(v);
            setEditing(null);
          }}
        />
        <main className="flex-1 min-w-0 overflow-auto bg-surface-0">
          {loadError && (
            <p className="text-warning text-xs px-4 py-2 bg-warning/10 border-b border-warning/30" role="alert">
              {t("errors.loadFailed")}
            </p>
          )}
          {editing !== null ? (
            <ProfileForm
              profile={editing === "new" ? null : editing}
              proxies={proxies}
              onSave={handleSaveProfile}
              onDelete={editing !== "new" ? handleDeleteProfile : undefined}
              onCancel={() => setEditing(null)}
            />
          ) : view === "profiles" ? (
            <ProfileList
              profiles={profiles}
              proxies={proxies}
              runningIds={runningIds}
              search={search}
              onEdit={(p) => setEditing(p)}
              onLaunch={handleLaunch}
              onStop={handleStop}
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
          ) : (
            <div className="p-6">
              <h2 className="text-lg font-semibold mb-2">{t("trash.title")}</h2>
              <p className="text-xs text-fg-muted">{t("trash.empty")}</p>
            </div>
          )}
        </main>
      </div>
    </div>
  );
}
