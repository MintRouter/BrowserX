import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { isTauri } from "../lib/api";

/** Module-level guard: check for a newer app build at most once per session. */
let checkedThisSession = false;

type Status = "available" | "downloading" | "installing" | "error";

interface BannerState {
  update: Update;
  status: Status;
  pct: number;
  hasTotal: boolean;
  error: string | null;
}

const errMsg = (err: unknown) =>
  err instanceof Error ? err.message : String(err);

/**
 * (W60c) Non-blocking banner offering a newer BrowserX build, mirroring
 * EngineUpdateBanner. Checks once per session ~8s after mount, stays silent
 * when there is no update (or outside Tauri / on any error). Applying
 * downloads + installs via the updater plugin, then relaunches the app
 * (on Windows the installer exits the app itself, hence the "installing,
 * the app will restart" copy).
 */
export function AppUpdateBanner() {
  const { t } = useTranslation();
  const [banner, setBanner] = useState<BannerState | null>(null);
  const [dismissed, setDismissed] = useState(false);

  // One deferred check per session, after the UI has settled.
  useEffect(() => {
    if (!isTauri() || checkedThisSession) return;
    checkedThisSession = true;
    const timer = setTimeout(() => {
      check()
        .then((update) => {
          if (update) {
            setBanner({
              update,
              status: "available",
              pct: 0,
              hasTotal: false,
              error: null,
            });
          }
        })
        .catch((err) => {
          // Background check — stay silent on any error/404 (spec W60).
          console.warn("app update check failed:", errMsg(err));
        });
    }, 8000);
    return () => clearTimeout(timer);
  }, []);

  const apply = useCallback((update: Update) => {
    setBanner({
      update,
      status: "downloading",
      pct: 0,
      hasTotal: false,
      error: null,
    });
    let total = 0;
    let downloaded = 0;
    update
      .downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
          setBanner((prev) =>
            prev ? { ...prev, pct: 0, hasTotal: total > 0 } : prev,
          );
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (total > 0) {
            const pct = Math.min(100, Math.round((downloaded / total) * 100));
            setBanner((prev) => (prev ? { ...prev, pct } : prev));
          }
        } else if (event.event === "Finished") {
          setBanner((prev) =>
            prev ? { ...prev, status: "installing", pct: 100 } : prev,
          );
        }
      })
      .then(() => {
        setBanner((prev) =>
          prev ? { ...prev, status: "installing", pct: 100 } : prev,
        );
        return relaunch();
      })
      .catch((err) => {
        setBanner((prev) =>
          prev ? { ...prev, status: "error", error: errMsg(err) } : prev,
        );
      });
  }, []);

  if (!banner || dismissed) return null;

  const { update, status, pct, hasTotal, error } = banner;
  const notes = update.body?.trim();

  return (
    <div
      className="border-b border-accent/30 bg-accent/10 px-4 py-2 text-xs"
      role="status"
      aria-live="polite"
    >
      <div className="flex items-center gap-3">
        <span className="shrink-0 font-medium text-accent">
          {t("appUpdate.title")}
        </span>
        <span className="flex-1 truncate text-fg-muted" title={notes ?? ""}>
          {status === "available" && (
            <>
              {t("appUpdate.available", {
                current: update.currentVersion,
                latest: update.version,
              })}
              {notes ? ` — ${notes}` : ""}
            </>
          )}
          {status === "downloading" &&
            (hasTotal
              ? `${t("appUpdate.downloading")} — ${pct}%`
              : `${t("appUpdate.downloading")}…`)}
          {status === "installing" && t("appUpdate.installing")}
          {status === "error" && (
            <span className="text-danger/80" title={error ?? ""}>
              {t("appUpdate.errorTitle")}: {error}
            </span>
          )}
        </span>
        <div className="flex shrink-0 items-center gap-2">
          {status === "available" && (
            <button
              type="button"
              onClick={() => apply(update)}
              className="rounded-md bg-accent px-3 py-1 font-medium text-white hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
            >
              {t("appUpdate.update")}
            </button>
          )}
          {status === "error" && (
            <button
              type="button"
              onClick={() => apply(update)}
              className="rounded-md bg-danger px-3 py-1 font-medium text-white hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-danger/60"
            >
              {t("appUpdate.retry")}
            </button>
          )}
          {status === "available" || status === "error" ? (
            <button
              type="button"
              onClick={() => setDismissed(true)}
              className="rounded-md px-2 py-1 font-medium text-fg-muted hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
              aria-label={t("appUpdate.dismiss")}
            >
              ✕
            </button>
          ) : null}
        </div>
      </div>
      {status === "downloading" && (
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
