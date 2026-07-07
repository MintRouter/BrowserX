import { Loader2, Minus, Plus, X } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  api,
  isTauri,
  type Platform,
  type ProfileInput,
  type ProfileTemplate,
  type Proxy,
  type ProxyCheckResult,
} from "../lib/api";
import { detectHostPlatform } from "../lib/host";
import { Segmented, Toggle } from "./profile-form/controls";

const MAX_COUNT = 25;

type ScreenMode = "masked" | "custom" | "real";
type ProxyMode = "saved" | "none";

interface QuickProfileModalProps {
  templates: ProfileTemplate[];
  proxies: Proxy[];
  /** Create + launch `count` quick profiles from the assembled input. */
  onStart: (input: Omit<ProfileInput, "name">, count: number) => Promise<void>;
  onClose: () => void;
}

/**
 * (W50G) MLX-parity "Create quick profile" modal: 2-column body, dark tip
 * banner, footer with Cancel / Check proxy / Start (Start = create + launch).
 * MLX-only fields with no backend (pre-made cookies, connection type,
 * location, sticky session, traffic saver, browser) are omitted.
 */
export function QuickProfileModal({
  templates,
  proxies,
  onStart,
  onClose,
}: QuickProfileModalProps) {
  const { t } = useTranslation();
  const [templateId, setTemplateId] = useState("");
  const [count, setCount] = useState(1);
  const [platform, setPlatform] = useState<Platform>(detectHostPlatform());
  const [screenMode, setScreenMode] = useState<ScreenMode>("masked");
  const [width, setWidth] = useState(1920);
  const [height, setHeight] = useState(1080);
  const [startupBehavior, setStartupBehavior] = useState<"restore" | "custom">(
    "restore",
  );
  const [startupUrl, setStartupUrl] = useState("");
  const [headless, setHeadless] = useState(false);
  const [proxyMode, setProxyMode] = useState<ProxyMode>(
    proxies.length > 0 ? "saved" : "none",
  );
  const [proxyId, setProxyId] = useState<string>(proxies[0]?.id ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkResult, setCheckResult] = useState<ProxyCheckResult | null>(null);

  const clampCount = (n: number) => Math.min(MAX_COUNT, Math.max(1, n));

  const screenDims = (): { screen_width: number; screen_height: number } => {
    if (screenMode === "custom") return { screen_width: width, screen_height: height };
    if (screenMode === "real")
      return { screen_width: window.screen.width, screen_height: window.screen.height };
    return { screen_width: 1920, screen_height: 1080 };
  };

  const buildInput = (): Omit<ProfileInput, "name"> => {
    const tpl = templates.find((x) => x.id === templateId);
    const base: Partial<ProfileInput> = tpl ? { ...tpl.config } : {};
    delete base.name;
    delete base.fingerprint_seed;
    const url = startupUrl.trim();
    return {
      ...base,
      is_quick: true,
      platform,
      ...screenDims(),
      headless,
      startup_behavior: startupBehavior,
      startup_urls: startupBehavior === "custom" && url ? [url] : [],
      proxy_id: proxyMode === "saved" && proxyId ? proxyId : null,
    };
  };

  const handleStart = async () => {
    setBusy(true);
    setError(null);
    try {
      await onStart(buildInput(), count);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  };

  const handleCheckProxy = async () => {
    if (!proxyId) return;
    setChecking(true);
    setCheckResult(null);
    setError(null);
    try {
      setCheckResult(await api.checkProxy({ proxy_id: proxyId }));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setChecking(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t("quick.title")}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !busy) onClose();
      }}
    >
      <div className="card flex max-h-[85vh] w-full max-w-[920px] flex-col overflow-hidden">
        {/* Header */}
        <div className="flex items-start gap-3 px-6 pb-3 pt-5">
          <div className="min-w-0 flex-1">
            <h2 className="text-lg font-medium text-fg">{t("quick.title")}</h2>
            <p className="mt-1 text-sm text-fg-muted">{t("quick.subtitle")}</p>
          </div>
          <button
            type="button"
            aria-label={t("quick.close")}
            onClick={onClose}
            disabled={busy}
            className="grid h-8 w-8 shrink-0 place-items-center rounded-md text-fg-muted hover:bg-surface-2 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
          >
            <X className="h-4 w-4" aria-hidden="true" />
          </button>
        </div>

        {/* Body: dark tip banner + 2 columns, scrollable */}
        <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-5">
          <div className="rounded-md bg-[#1D192B] px-4 py-2.5 text-sm text-white dark:bg-surface-3">
            {t("quick.tipCheckProxy")}
          </div>
          <div className="mt-5 grid grid-cols-1 gap-x-8 gap-y-5 lg:grid-cols-2">
            {/* Left column */}
            <div className="space-y-5">
              <div>
                <label className="label" htmlFor="qp-template">
                  {t("quick.template")}
                </label>
                <select
                  id="qp-template"
                  className="input"
                  value={templateId}
                  onChange={(e) => setTemplateId(e.target.value)}
                >
                  <option value="">{t("quick.templateNone")}</option>
                  {templates.map((tpl) => (
                    <option key={tpl.id} value={tpl.id}>
                      {tpl.name}
                    </option>
                  ))}
                </select>
              </div>

              <div>
                <span className="label" id="qp-count-label">
                  {t("quick.count", { max: MAX_COUNT })}
                </span>
                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    aria-label={t("quick.countDec")}
                    onClick={() => setCount((c) => clampCount(c - 1))}
                    disabled={count <= 1}
                    className="btn-secondary grid h-9 w-9 place-items-center p-0"
                  >
                    <Minus className="h-4 w-4" aria-hidden="true" />
                  </button>
                  <input
                    type="number"
                    min={1}
                    max={MAX_COUNT}
                    value={count}
                    onChange={(e) => setCount(clampCount(Number(e.target.value) || 1))}
                    aria-labelledby="qp-count-label"
                    className="input w-20 text-center"
                  />
                  <button
                    type="button"
                    aria-label={t("quick.countInc")}
                    onClick={() => setCount((c) => clampCount(c + 1))}
                    disabled={count >= MAX_COUNT}
                    className="btn-secondary grid h-9 w-9 place-items-center p-0"
                  >
                    <Plus className="h-4 w-4" aria-hidden="true" />
                  </button>
                </div>
              </div>

              <div>
                <span className="label">{t("pform.operatingSystem")}</span>
                <Segmented
                  options={[
                    { value: "macos", label: "macOS" },
                    { value: "windows", label: "Windows" },
                    { value: "linux", label: "Linux" },
                  ]}
                  value={platform}
                  onChange={setPlatform}
                  label={t("pform.operatingSystem")}
                />
              </div>

              <div>
                <span className="label">{t("pform.screenResolution")}</span>
                <Segmented
                  options={[
                    { value: "masked", label: t("quick.screenMasked") },
                    { value: "custom", label: t("quick.screenCustom") },
                    { value: "real", label: t("quick.screenReal") },
                  ]}
                  value={screenMode}
                  onChange={setScreenMode}
                  label={t("pform.screenResolution")}
                />
                {screenMode === "custom" && (
                  <div className="mt-2 flex items-center gap-2">
                    <input
                      type="number"
                      min={320}
                      value={width}
                      onChange={(e) => setWidth(Number(e.target.value) || 0)}
                      aria-label={t("pform.width")}
                      className="input w-24"
                    />
                    <span className="text-fg-muted" aria-hidden="true">
                      ×
                    </span>
                    <input
                      type="number"
                      min={320}
                      value={height}
                      onChange={(e) => setHeight(Number(e.target.value) || 0)}
                      aria-label={t("pform.height")}
                      className="input w-24"
                    />
                  </div>
                )}
              </div>

              <div>
                <span className="label">{t("pform.startupBehavior")}</span>
                <Segmented
                  options={[
                    { value: "restore", label: t("pform.startupRestore") },
                    { value: "custom", label: t("pform.startupCustom") },
                  ]}
                  value={startupBehavior}
                  onChange={setStartupBehavior}
                  label={t("pform.startupBehavior")}
                />
                {startupBehavior === "custom" && (
                  <input
                    type="text"
                    inputMode="url"
                    value={startupUrl}
                    onChange={(e) => setStartupUrl(e.target.value)}
                    placeholder={t("pform.startupUrlPlaceholder")}
                    aria-label={t("pform.startupUrlsLabel")}
                    className="input mt-2"
                  />
                )}
              </div>

              <div className="flex items-center justify-between gap-3">
                <span className="text-xs font-medium text-fg">
                  {t("quick.headless")}
                </span>
                <Toggle
                  checked={headless}
                  onChange={setHeadless}
                  label={t("quick.headless")}
                />
              </div>
            </div>

            {/* Right column: proxy */}
            <div className="space-y-5">
              <div>
                <span className="label">{t("pform.proxyLabel")}</span>
                <Segmented
                  options={[
                    { value: "saved", label: t("quick.proxySaved") },
                    { value: "none", label: t("quick.proxyNone") },
                  ]}
                  value={proxyMode}
                  onChange={setProxyMode}
                  label={t("pform.proxyLabel")}
                />
              </div>
              {proxyMode === "saved" && (
                <div>
                  <label className="label" htmlFor="qp-proxy">
                    {t("quick.proxyPick")}
                  </label>
                  <select
                    id="qp-proxy"
                    className="input"
                    value={proxyId}
                    onChange={(e) => {
                      setProxyId(e.target.value);
                      setCheckResult(null);
                    }}
                  >
                    <option value="">{t("pform.noProxy")}</option>
                    {proxies.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name} — {p.protocol}://{p.host}:{p.port}
                      </option>
                    ))}
                  </select>
                  {proxies.length === 0 && (
                    <p className="mt-1.5 text-xs text-fg-muted">
                      {t("quick.noProxies")}
                    </p>
                  )}
                  {checkResult && (
                    <p
                      className={`mt-1.5 text-xs ${checkResult.ok ? "text-success" : "text-danger"}`}
                      role="status"
                    >
                      {checkResult.ok
                        ? t("quick.checkOk", {
                            ip: checkResult.external_ip ?? "?",
                            ms: checkResult.latency_ms ?? "?",
                          })
                        : (checkResult.error ?? t("proxycheck.failed"))}
                    </p>
                  )}
                </div>
              )}
            </div>
          </div>
          {error && (
            <p role="alert" className="mt-4 text-sm text-danger">
              {error}
            </p>
          )}
        </div>

        {/* Sticky footer */}
        <div className="flex items-center gap-3 border-t border-border bg-surface-1 px-6 py-3">
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className="inline-flex h-10 items-center rounded px-3 text-sm font-medium text-accent hover:text-accent-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
          >
            {t("quick.cancel")}
          </button>
          <div className="ml-auto flex items-center gap-2">
            {proxyMode === "saved" && proxyId && isTauri() && (
              <button
                type="button"
                onClick={() => void handleCheckProxy()}
                disabled={checking || busy}
                className="inline-flex h-10 items-center gap-1.5 rounded-full bg-accent/10 px-4 text-sm font-medium text-accent hover:bg-accent/15 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:opacity-50"
              >
                {checking && (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
                )}
                {t("proxycheck.button")}
              </button>
            )}
            <button
              type="button"
              onClick={() => void handleStart()}
              disabled={busy}
              className="btn-primary inline-flex h-10 items-center gap-1.5 px-4"
            >
              {busy && (
                <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
              )}
              {t("quick.start")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
