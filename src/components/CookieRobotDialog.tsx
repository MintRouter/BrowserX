import { Loader2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  api,
  onCookieRobotProgress,
  type CookieRobotProgressEvent,
  type Profile,
} from "../lib/api";
import { Toggle } from "./profile-form/controls";

interface CookieRobotDialogProps {
  profile: Profile;
  onClose: () => void;
}

/** Phases after which the robot is no longer running. */
const TERMINAL_PHASES = new Set(["done", "cancelled", "error"]);

/** Flat i18n keys per backend phase (see cookierobot.rs). */
const PHASE_KEYS: Record<CookieRobotProgressEvent["phase"], string> = {
  starting: "robot.phaseStarting",
  proxy_check: "robot.phaseProxyCheck",
  goto: "robot.phaseGoto",
  consent: "robot.phaseConsent",
  dwell: "robot.phaseDwell",
  closing: "robot.phaseClosing",
  done: "robot.phaseDone",
  cancelled: "robot.phaseCancelled",
  error: "robot.phaseError",
};

/**
 * CookieRobot dialog (P3-4b) — visits a URL list on ONE profile to warm up
 * cookies. Live progress via `cookierobot://progress`; Cancel stops the robot
 * (the browser session stays open unless "close when done" already fired).
 */
export function CookieRobotDialog({
  profile,
  onClose,
}: CookieRobotDialogProps) {
  const { t } = useTranslation();
  const [urlsText, setUrlsText] = useState("");
  const [dwellSecs, setDwellSecs] = useState(0);
  const [randomOrder, setRandomOrder] = useState(false);
  const [processConsent, setProcessConsent] = useState(true);
  const [closeWhenDone, setCloseWhenDone] = useState(false);
  const [running, setRunning] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [progress, setProgress] = useState<CookieRobotProgressEvent | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);

  const urls = useMemo(
    () =>
      urlsText
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean),
    [urlsText],
  );

  // Follow this profile's robot; unlisten on unmount (also when the promise
  // resolves after the dialog already closed) to avoid leaked listeners.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void onCookieRobotProgress((e) => {
      if (e.profileId !== profile.id) return;
      setProgress(e);
      if (TERMINAL_PHASES.has(e.phase)) {
        setRunning(false);
        setStopping(false);
      }
    }).then((f) => {
      if (disposed) f();
      else unlisten = f;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [profile.id]);

  const handleStart = async () => {
    if (urls.length === 0 || running) return;
    setError(null);
    setProgress(null);
    setRunning(true);
    try {
      await api.startCookieRobot(profile.id, {
        urls,
        dwellSecs,
        randomOrder,
        processConsent,
        closeWhenDone,
      });
    } catch (err) {
      setRunning(false);
      setError(t("robot.startFailed", { error: String(err) }));
    }
  };

  const handleStop = async () => {
    if (stopping) return;
    setStopping(true);
    try {
      await api.stopCookieRobot(profile.id);
    } catch {
      // Robot already finished between the click and the call — ignore.
      setStopping(false);
    }
  };

  const title = t("robot.title", { name: profile.name });
  const pct =
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.current / progress.total) * 100))
      : 0;

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      onKeyDown={(e) => {
        if (e.key === "Escape") onClose();
      }}
    >
      <div className="card w-full max-w-md p-5">
        <h2 className="text-base font-semibold text-fg">{title}</h2>
        <p className="mt-1.5 text-xs text-fg-muted">{t("robot.hint")}</p>

        <div className="mt-4 space-y-3">
          <label className="block">
            <span className="label">{t("robot.urlsLabel")}</span>
            <textarea
              value={urlsText}
              onChange={(e) => setUrlsText(e.target.value)}
              placeholder={t("robot.urlsPlaceholder")}
              disabled={running}
              rows={5}
              className="input resize-y font-mono text-xs leading-relaxed"
            />
            <span className="mt-1 block text-xs text-fg-muted">
              {t("robot.urlCount", { count: urls.length })}
            </span>
          </label>

          <label className="block">
            <span className="label">{t("robot.dwellLabel")}</span>
            <input
              type="number"
              min={0}
              max={120}
              value={dwellSecs}
              onChange={(e) =>
                setDwellSecs(Math.max(0, Number(e.target.value) || 0))
              }
              disabled={running}
              className="input h-9 w-28 py-0"
            />
            <span className="mt-1 block text-xs text-fg-muted">
              {t("robot.dwellHint")}
            </span>
          </label>

          <div className="space-y-2 text-sm text-fg">
            <div className="flex items-center justify-between gap-3">
              <span>{t("robot.randomOrder")}</span>
              <Toggle
                checked={randomOrder}
                onChange={setRandomOrder}
                disabled={running}
                label={t("robot.randomOrder")}
              />
            </div>
            <div className="flex items-center justify-between gap-3">
              <span>{t("robot.processConsent")}</span>
              <Toggle
                checked={processConsent}
                onChange={setProcessConsent}
                disabled={running}
                label={t("robot.processConsent")}
              />
            </div>
            <div className="flex items-center justify-between gap-3">
              <span>{t("robot.closeWhenDone")}</span>
              <Toggle
                checked={closeWhenDone}
                onChange={setCloseWhenDone}
                disabled={running}
                label={t("robot.closeWhenDone")}
              />
            </div>
          </div>
        </div>

        {progress && (
          <div
            className="mt-4 rounded-md border border-border bg-surface-2 p-3"
            role="status"
            aria-live="polite"
          >
            <div className="flex items-center justify-between gap-2 text-sm">
              <span className="font-medium text-fg">
                {t(PHASE_KEYS[progress.phase])}
              </span>
              <span className="text-xs tabular-nums text-fg-muted">
                {progress.current} / {progress.total}
              </span>
            </div>
            <div
              className="mt-2 h-1.5 overflow-hidden rounded-full bg-surface-4"
              role="progressbar"
              aria-label={t("robot.progressLabel")}
              aria-valuemin={0}
              aria-valuemax={progress.total}
              aria-valuenow={progress.current}
            >
              <div
                className="h-full rounded-full bg-accent transition-[width] motion-reduce:transition-none"
                style={{ width: `${pct}%` }}
              />
            </div>
            {progress.url && (
              <p
                className="mt-2 truncate text-xs text-fg-muted"
                title={progress.url}
              >
                {progress.url}
              </p>
            )}
            {progress.error && (
              <p className="mt-2 text-xs text-danger">{progress.error}</p>
            )}
          </div>
        )}

        {error && <p className="mt-3 text-sm text-danger">{error}</p>}

        <div className="mt-5 flex justify-end gap-2">
          <button type="button" className="btn-secondary h-9" onClick={onClose}>
            {running ? t("robot.closeKeepRunning") : t("robot.close")}
          </button>
          {running ? (
            <button
              type="button"
              className="btn-danger h-9"
              disabled={stopping}
              onClick={() => void handleStop()}
            >
              {stopping && (
                <Loader2
                  className="h-3.5 w-3.5 animate-spin"
                  aria-hidden="true"
                />
              )}
              {t("robot.stopButton")}
            </button>
          ) : (
            <button
              type="button"
              className="btn-primary h-9"
              disabled={urls.length === 0}
              onClick={() => void handleStart()}
            >
              {t("robot.startButton")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
