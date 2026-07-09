import { Loader2, Play, Square } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

interface LaunchButtonProps {
  status: "running" | "stopped";
  onLaunch: () => Promise<void>;
  onStop: () => Promise<void>;
}

export function LaunchButton({ status, onLaunch, onStop }: LaunchButtonProps) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleClick = async () => {
    setLoading(true);
    setError(null);
    try {
      if (status === "running") {
        await onStop();
      } else {
        await onLaunch();
      }
    } catch (err) {
      const msg =
        err instanceof Error ? err.message : String(err ?? t("launch.failed"));
      setError(msg);
      console.error("Action failed:", err);
    } finally {
      setLoading(false);
    }
  };

  if (loading) {
    return (
      <button disabled className="btn-secondary opacity-60 cursor-not-allowed">
        <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
        <span>
          {status === "running" ? t("launch.stopping") : t("launch.launching")}
        </span>
      </button>
    );
  }

  if (status === "running") {
    return (
      <button onClick={handleClick} className="btn-danger">
        <Square className="h-3.5 w-3.5" aria-hidden="true" />
        <span>{t("launch.stop")}</span>
      </button>
    );
  }

  return (
    <div>
      <button onClick={handleClick} className="btn-primary">
        <Play className="h-3.5 w-3.5" aria-hidden="true" />
        <span>{t("launch.launch")}</span>
      </button>
      {error && (
        <p className="text-danger text-xs mt-1" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
