import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  api,
  isTauri,
  onEngineUpdateProgress,
  type EngineUpdateInfo,
} from "../lib/api";

/** Module-level guard: check GitHub for a newer engine at most once per session. */
let checkedThisSession = false;

type Status = "available" | "updating" | "done" | "error";

interface BannerState {
  info: EngineUpdateInfo;
  status: Status;
  phase: string;
  pct: number;
  error: string | null;
}

const errMsg = (err: unknown) =>
  err instanceof Error ? err.message : String(err);

/**
 * (W58c) Non-blocking banner offering a newer browser engine. Checks once per
 * session ~5s after the engine is ready, stays silent when there is no update
 * (or outside Tauri / on any error). Applying downloads + verifies the new
 * build; it becomes the default for NEW profiles (existing profiles keep their
 * pinned engine and upgrade individually in the profile editor).
 */
export function EngineUpdateBanner({
  engineReady,
  onApplied,
}: {
  /** True once the first-run engine probe/download has resolved. */
  engineReady: boolean;
  /** Called with the new default version after a successful update. */
  onApplied?: (version: string) => void;
}) {
  const { t } = useTranslation();
  const [banner, setBanner] = useState<BannerState | null>(null);
  const [dismissed, setDismissed] = useState(false);

  // One deferred check per session, after the engine is ready + UI settled.
  useEffect(() => {
    if (!isTauri() || !engineReady || checkedThisSession) return;
    checkedThisSession = true;
    const timer = setTimeout(() => {
      api
        .checkEngineUpdate()
        .then((info) => {
          if (info) {
            setBanner({
              info,
              status: "available",
              phase: "download",
              pct: 0,
              error: null,
            });
          }
        })
        .catch(() => {
          // Background check — stay silent on any error (spec W58).
        });
    }, 5000);
    return () => clearTimeout(timer);
  }, [engineReady]);

  // Progress of an in-flight apply (same payload as binary://progress).
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    onEngineUpdateProgress((e) => {
      setBanner((prev) =>
        prev && prev.status === "updating"
          ? { ...prev, phase: e.phase, pct: e.pct }
          : prev,
      );
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => unlisten?.();
  }, []);

  // Keep the latest onApplied without re-creating apply().
  const onAppliedRef = useRef(onApplied);
  onAppliedRef.current = onApplied;

  const apply = useCallback((info: EngineUpdateInfo) => {
    setBanner({
      info,
      status: "updating",
      phase: "download",
      pct: 0,
      error: null,
    });
    api
      .applyEngineUpdate(info.latest)
      .then(() => {
        setBanner((prev) =>
          prev ? { ...prev, status: "done", pct: 100 } : prev,
        );
        onAppliedRef.current?.(info.latest);
      })
      .catch((err) => {
        setBanner((prev) =>
          prev ? { ...prev, status: "error", error: errMsg(err) } : prev,
        );
      });
  }, []);

  if (!banner || dismissed) return null;

  const { info, status, phase, pct, error } = banner;
  const phaseLabel = t(`engine.phase.${phase}`, { defaultValue: phase });

  return (
    <div
      className="border-b border-accent/30 bg-accent/10 px-4 py-2 text-xs"
      role="status"
      aria-live="polite"
    >
      <div className="flex items-center gap-3">
        <span className="shrink-0 font-medium text-accent">
          {t("engineUpdate.title")}
        </span>
        <span className="flex-1 truncate text-fg-muted">
          {status === "available" &&
            t("engineUpdate.available", {
              current: info.current,
              latest: info.latest,
            })}
          {status === "updating" && `${phaseLabel} — ${pct}%`}
          {status === "done" && t("engineUpdate.done")}
          {status === "error" && (
            <span className="text-danger/80" title={error ?? ""}>
              {t("engineUpdate.errorTitle")}: {error}
            </span>
          )}
        </span>
        <div className="flex shrink-0 items-center gap-2">
          {status === "available" && (
            <button
              type="button"
              onClick={() => apply(info)}
              className="rounded-md bg-accent px-3 py-1 font-medium text-white hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
            >
              {t("engineUpdate.update")}
            </button>
          )}
          {status === "error" && (
            <button
              type="button"
              onClick={() => apply(info)}
              className="rounded-md bg-danger px-3 py-1 font-medium text-white hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-danger/60"
            >
              {t("engineUpdate.retry")}
            </button>
          )}
          {status !== "updating" && (
            <button
              type="button"
              onClick={() => setDismissed(true)}
              className="rounded-md px-2 py-1 font-medium text-fg-muted hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
              aria-label={t("engineUpdate.dismiss")}
            >
              ✕
            </button>
          )}
        </div>
      </div>
      {status === "updating" && (
        <div className="mt-1.5 h-1.5 w-full overflow-hidden rounded-full bg-accent/20">
          <div
            className="h-full rounded-full bg-accent transition-all duration-300"
            style={{ width: `${Math.min(100, Math.max(0, pct))}%` }}
          />
        </div>
      )}
    </div>
  );
}
