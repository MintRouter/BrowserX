import { useTranslation } from "react-i18next";

/** UI state for the first-run browser-engine download (driven by `binary://progress`). */
export interface EngineState {
  /** checking = ensure_binary in flight, no download event yet (cache probe). */
  status: "checking" | "downloading" | "ready" | "error";
  /** Backend phase: "download" | "verify" | "extract" | "done". */
  phase: string;
  pct: number;
  downloadedBytes: number;
  totalBytes: number;
  error: string | null;
}

export const initialEngineState = (ready: boolean): EngineState => ({
  status: ready ? "ready" : "checking",
  phase: "download",
  pct: 0,
  downloadedBytes: 0,
  totalBytes: 0,
  error: null,
});

const toMB = (bytes: number) => (bytes / (1024 * 1024)).toFixed(1);

/**
 * Non-blocking banner showing browser-engine download progress on first run.
 * Renders nothing when the engine is ready (or still probing the cache), so
 * profile management stays fully usable while the download runs.
 */
export function EngineSetup({
  engine,
  onRetry,
}: {
  engine: EngineState;
  onRetry: () => void;
}) {
  const { t } = useTranslation();

  if (engine.status === "ready" || engine.status === "checking") return null;

  if (engine.status === "error") {
    return (
      <div
        className="px-4 py-2 bg-danger/10 border-b border-danger/30 text-xs"
        role="alert"
      >
        <div className="flex items-center gap-3">
          <span className="shrink-0 font-medium text-danger">
            {t("engine.errorTitle")}
          </span>
          <span className="flex-1 truncate text-danger/80" title={engine.error ?? ""}>
            {engine.error}
          </span>
          <button
            type="button"
            onClick={onRetry}
            className="shrink-0 rounded-md bg-danger px-3 py-1 font-medium text-white hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-danger/60"
          >
            {t("engine.retry")}
          </button>
        </div>
      </div>
    );
  }

  const phaseLabel = t(`engine.phase.${engine.phase}`, {
    defaultValue: engine.phase,
  });
  const showBytes = engine.phase === "download" && engine.totalBytes > 0;

  return (
    <div
      className="px-4 py-2 bg-accent/10 border-b border-accent/30 text-xs"
      role="status"
      aria-live="polite"
    >
      <div className="flex items-center gap-3">
        <span className="shrink-0 font-medium text-accent">
          {t("engine.downloading")}
        </span>
        <span className="flex-1 truncate text-fg-muted">
          {phaseLabel} — {engine.pct}%
          {showBytes &&
            ` (${t("engine.bytes", {
              done: toMB(engine.downloadedBytes),
              total: toMB(engine.totalBytes),
            })})`}
        </span>
      </div>
      <div className="mt-1.5 h-1.5 w-full overflow-hidden rounded-full bg-accent/20">
        <div
          className="h-full rounded-full bg-accent transition-all duration-300"
          style={{ width: `${Math.min(100, Math.max(0, engine.pct))}%` }}
        />
      </div>
    </div>
  );
}
