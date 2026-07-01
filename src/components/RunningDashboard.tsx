import { Copy } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { Profile, RunningSession } from "../lib/api";
import { LaunchButton } from "./LaunchButton";
import { StatusIndicator } from "./StatusIndicator";

interface RunningDashboardProps {
  sessions: RunningSession[];
  profiles: Profile[];
  onStop: (profileId: string) => Promise<void>;
}

export function RunningDashboard({ sessions, profiles, onStop }: RunningDashboardProps) {
  const { t, i18n } = useTranslation();
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const profileName = (id: string) =>
    profiles.find((p) => p.id === id)?.name ?? id;

  const copyCdp = async (s: RunningSession) => {
    try {
      await navigator.clipboard.writeText(s.cdp_url);
      setCopiedId(s.profile_id);
      setTimeout(() => setCopiedId(null), 1500);
    } catch (err) {
      console.error("Clipboard failed:", err);
    }
  };

  const fmtDate = (iso: string) => {
    const d = new Date(iso);
    return isNaN(d.getTime())
      ? iso
      : new Intl.DateTimeFormat(i18n.language, { timeStyle: "medium" }).format(d);
  };

  return (
    <div className="p-6">
      <h2 className="text-lg font-semibold mb-4">{t("running.title")}</h2>
      {sessions.length === 0 ? (
        <p className="text-xs text-fg-muted">{t("running.empty")}</p>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
          {sessions.map((s) => (
            <div
              key={s.profile_id}
              className="rounded-lg border border-border bg-surface-1 p-4 space-y-3"
            >
              <div className="flex items-center gap-2">
                <StatusIndicator status="running" size="md" />
                <span className="font-medium truncate">{profileName(s.profile_id)}</span>
              </div>
              <dl className="text-xs text-fg-muted space-y-1">
                <div className="flex justify-between">
                  <dt>{t("running.pid")}</dt>
                  <dd className="font-mono">{s.pid}</dd>
                </div>
                <div className="flex justify-between items-center gap-2">
                  <dt>{t("running.cdp")}</dt>
                  <dd className="font-mono truncate flex items-center gap-1">
                    <span className="truncate">{s.cdp_url}</span>
                    <button
                      onClick={() => copyCdp(s)}
                      className="p-1 rounded hover:bg-surface-3 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                      aria-label={t("running.copyCdp")}
                      title={t("running.copyCdp")}
                    >
                      <Copy className="h-3 w-3" aria-hidden="true" />
                    </button>
                    {copiedId === s.profile_id && (
                      <span className="text-success">{t("running.copied")}</span>
                    )}
                  </dd>
                </div>
                <div className="flex justify-between">
                  <dt>{t("running.startedAt")}</dt>
                  <dd>{fmtDate(s.started_at)}</dd>
                </div>
              </dl>
              <LaunchButton
                status="running"
                onLaunch={async () => {}}
                onStop={() => onStop(s.profile_id)}
              />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
