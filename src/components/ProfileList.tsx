import { Pencil } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Profile, Proxy } from "../lib/api";
import { LaunchButton } from "./LaunchButton";
import { StatusIndicator } from "./StatusIndicator";

type SortKey = "name" | "updated";

interface ProfileListProps {
  profiles: Profile[];
  proxies: Proxy[];
  runningIds: Set<string>;
  search: string;
  onEdit: (profile: Profile) => void;
  onLaunch: (id: string) => Promise<void>;
  onStop: (id: string) => Promise<void>;
}

export function ProfileList({
  profiles,
  proxies,
  runningIds,
  search,
  onEdit,
  onLaunch,
  onStop,
}: ProfileListProps) {
  const { t, i18n } = useTranslation();
  const [tagFilter, setTagFilter] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("updated");

  const allTags = useMemo(
    () => [...new Set(profiles.flatMap((p) => p.tags))].sort(),
    [profiles],
  );

  const proxyName = (id: string | null) =>
    id ? (proxies.find((p) => p.id === id)?.name ?? "—") : "—";

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    let list = profiles.filter((p) => {
      if (tagFilter && !p.tags.includes(tagFilter)) return false;
      if (!q) return true;
      return (
        p.name.toLowerCase().includes(q) ||
        p.platform.toLowerCase().includes(q) ||
        (p.notes ?? "").toLowerCase().includes(q) ||
        p.tags.some((tag) => tag.toLowerCase().includes(q)) ||
        proxyName(p.proxy_id).toLowerCase().includes(q)
      );
    });
    list = [...list].sort((a, b) =>
      sortKey === "name"
        ? a.name.localeCompare(b.name)
        : b.updated_at.localeCompare(a.updated_at),
    );
    return list;
  }, [profiles, search, tagFilter, sortKey, proxies]);

  const fmtDate = (iso: string) => {
    const d = new Date(iso);
    return isNaN(d.getTime())
      ? iso
      : new Intl.DateTimeFormat(i18n.language, {
          dateStyle: "short",
          timeStyle: "short",
        }).format(d);
  };

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 p-3 border-b border-border">
        <select
          className="input w-auto text-xs"
          value={tagFilter}
          onChange={(e) => setTagFilter(e.target.value)}
          aria-label={t("table.allTags")}
        >
          <option value="">{t("table.allTags")}</option>
          {allTags.map((tag) => (
            <option key={tag} value={tag}>{tag}</option>
          ))}
        </select>
        <select
          className="input w-auto text-xs"
          value={sortKey}
          onChange={(e) => setSortKey(e.target.value as SortKey)}
          aria-label={t("table.sortUpdated")}
        >
          <option value="updated">{t("table.sortUpdated")}</option>
          <option value="name">{t("table.sortName")}</option>
        </select>
      </div>

      <div className="flex-1 overflow-auto">
        {filtered.length === 0 ? (
          <div className="text-center text-fg-muted text-xs py-12">
            {profiles.length === 0 ? t("table.empty") : t("table.noMatches")}
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead className="sticky top-0 bg-surface-1 text-left text-xs text-fg-muted uppercase tracking-wider">
              <tr>
                <th scope="col" className="px-4 py-2 font-medium">{t("table.name")}</th>
                <th scope="col" className="px-4 py-2 font-medium">{t("table.status")}</th>
                <th scope="col" className="px-4 py-2 font-medium">{t("table.os")}</th>
                <th scope="col" className="px-4 py-2 font-medium">{t("table.proxy")}</th>
                <th scope="col" className="px-4 py-2 font-medium">{t("table.tags")}</th>
                <th scope="col" className="px-4 py-2 font-medium">{t("table.updated")}</th>
                <th scope="col" className="px-4 py-2 font-medium">{t("table.actions")}</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((p) => {
                const running = runningIds.has(p.id);
                return (
                  <tr key={p.id} className="border-b border-border hover:bg-surface-1">
                    <td className="px-4 py-2 font-medium">{p.name}</td>
                    <td className="px-4 py-2">
                      <span className="inline-flex items-center gap-1.5">
                        <StatusIndicator status={running ? "running" : "stopped"} />
                        <span className="text-xs text-fg-muted">
                          {running ? t("table.running") : t("table.stopped")}
                        </span>
                      </span>
                    </td>
                    <td className="px-4 py-2 capitalize text-fg-muted">{p.platform}</td>
                    <td className="px-4 py-2 text-fg-muted">{proxyName(p.proxy_id)}</td>
                    <td className="px-4 py-2">
                      <span className="flex gap-1 flex-wrap">
                        {p.tags.map((tag) => (
                          <span key={tag} className="text-[10px] px-1.5 py-0.5 rounded-full bg-surface-3 text-fg-muted">
                            {tag}
                          </span>
                        ))}
                      </span>
                    </td>
                    <td className="px-4 py-2 text-xs text-fg-muted whitespace-nowrap">{fmtDate(p.updated_at)}</td>
                    <td className="px-4 py-2">
                      <span className="flex items-center gap-2">
                        <LaunchButton
                          status={running ? "running" : "stopped"}
                          onLaunch={() => onLaunch(p.id)}
                          onStop={() => onStop(p.id)}
                        />
                        <button
                          onClick={() => onEdit(p)}
                          className="btn-secondary px-2"
                          aria-label={`${t("table.edit")}: ${p.name}`}
                        >
                          <Pencil className="h-3.5 w-3.5" aria-hidden="true" />
                        </button>
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
