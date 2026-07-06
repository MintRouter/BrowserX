import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type SystemMetrics, api, isTauri } from "../../lib/api";

/** Refresh interval while the panel is mounted (Settings open). */
const POLL_MS = 3000;

function Stat({
  label,
  value,
  sub,
}: {
  label: string;
  value: string;
  sub?: string;
}) {
  return (
    <div className="rounded-md border border-border p-3">
      <p className="text-xs text-fg-muted">{label}</p>
      <p className="mt-1 text-lg font-medium text-fg">{value}</p>
      {sub && <p className="mt-0.5 text-xs text-fg-muted">{sub}</p>}
    </div>
  );
}

/**
 * (W26b) Observability panel for Settings: live sessions, RAM per session
 * (main-process RSS; N/A where not measurable), launch p95 and error rate.
 * (W27) Polls every 3s only while the section is in the viewport — an
 * IntersectionObserver gates the interval; both are cleaned up on unmount.
 */
export function SystemPanel() {
  const { t } = useTranslation();
  const rootRef = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const [metrics, setMetrics] = useState<SystemMetrics | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    const el = rootRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(([entry]) =>
      setVisible(entry?.isIntersecting ?? false),
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!isTauri() || !visible) return;
    let cancelled = false;
    const load = async () => {
      try {
        const m = await api.getMetrics();
        if (!cancelled) {
          setMetrics(m);
          setError(false);
        }
      } catch {
        if (!cancelled) setError(true);
      }
    };
    load();
    const id = setInterval(load, POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [visible]);

  if (!isTauri()) {
    return <p className="text-sm text-fg-muted">{t("system.desktopOnly")}</p>;
  }

  const na = t("system.na");
  const perSession = metrics?.ram_per_session_mb ?? [];
  // (W27) Partial sample: fewer measured sessions than live ones — the total
  // is a lower bound, so label it "≥" with an explanatory sub note.
  const ramPartial =
    metrics != null &&
    metrics.ram_total_mb != null &&
    perSession.length < metrics.live_sessions;
  const ramTotal =
    metrics?.ram_total_mb != null
      ? `${ramPartial ? "≥ " : ""}${metrics.ram_total_mb} MB`
      : na;
  const ramMean =
    perSession.length > 0
      ? `${Math.round(perSession.reduce((a, b) => a + b, 0) / perSession.length)} MB`
      : na;
  const p95 = metrics?.launch_p95_ms != null ? `${metrics.launch_p95_ms} ms` : na;
  const attempts = (metrics?.launch_success ?? 0) + (metrics?.launch_fail ?? 0);
  const errorRate =
    metrics && attempts > 0
      ? `${((metrics.launch_fail / attempts) * 100).toFixed(1)}%`
      : na;

  return (
    <div ref={rootRef} className="space-y-3">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
        <Stat
          label={t("system.liveSessions")}
          value={metrics ? String(metrics.live_sessions) : "—"}
        />
        <Stat
          label={t("system.ramTotal")}
          value={metrics ? ramTotal : "—"}
          sub={
            metrics && ramPartial
              ? t("system.ramPartial", {
                  measured: perSession.length,
                  live: metrics.live_sessions,
                })
              : undefined
          }
        />
        <Stat
          label={t("system.ramPerSession")}
          value={metrics ? ramMean : "—"}
        />
        <Stat label={t("system.launchP95")} value={metrics ? p95 : "—"} />
        <Stat
          label={t("system.errorRate")}
          value={metrics ? errorRate : "—"}
          sub={
            metrics
              ? t("system.launches", {
                  success: metrics.launch_success,
                  fail: metrics.launch_fail,
                })
              : undefined
          }
        />
      </div>
      <p className="text-xs text-fg-muted">{t("system.ramNote")}</p>
      {error && (
        <p className="text-xs text-danger" role="alert">
          {t("system.loadFailed")}
        </p>
      )}
    </div>
  );
}
