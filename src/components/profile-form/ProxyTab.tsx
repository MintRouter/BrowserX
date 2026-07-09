import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import {
  api,
  type Proxy,
  type ProxyCheckResult,
  type ProxyTemplate,
} from "../../lib/api";
import { Toggle } from "./controls";
import type { FormState, SetField } from "./types";

interface ProxyTabProps {
  form: FormState;
  set: SetField;
  proxies: Proxy[];
  /** (P3-3b) Refetch proxies after "use template" creates one server-side. */
  onProxiesChanged?: () => void | Promise<void>;
}

/** Optional health status — present in newer backend payloads. */
type ProxyWithHealth = Proxy & { health_status?: string | null };

export function ProxyTab({
  form,
  set,
  proxies,
  onProxiesChanged,
}: ProxyTabProps) {
  const { t } = useTranslation();

  const [checking, setChecking] = useState(false);
  const [checkedId, setCheckedId] = useState<string | null>(null);
  const [checkResult, setCheckResult] = useState<ProxyCheckResult | null>(null);
  // (P3-3b) Proxy templates: pick one to create a pre-filled proxy and assign
  // it. Credentials are copied encrypted server-side (never decrypted here).
  const [templates, setTemplates] = useState<ProxyTemplate[]>([]);
  const [templateId, setTemplateId] = useState("");
  const [applying, setApplying] = useState(false);
  const [applyError, setApplyError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .listProxyTemplates()
      .then((list) => {
        if (!cancelled) setTemplates(list);
      })
      .catch(() => {
        // offline / non-Tauri: keep empty list
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const selectedTemplate = templates.find((x) => x.id === templateId) ?? null;

  const applyTemplate = async () => {
    if (!selectedTemplate) return;
    setApplying(true);
    setApplyError(null);
    try {
      const created = await api.createProxyFromTemplate(selectedTemplate.id);
      await onProxiesChanged?.();
      set("proxy_id", created.id);
      setTemplateId("");
    } catch (e) {
      setApplyError(e instanceof Error ? e.message : String(e));
    } finally {
      setApplying(false);
    }
  };

  async function runCheck(proxyId: string) {
    setChecking(true);
    setCheckedId(proxyId);
    setCheckResult(null);
    try {
      const res = await api.checkProxy({ proxy_id: proxyId });
      setCheckResult(res);
    } catch (e) {
      setCheckResult({
        ok: false,
        external_ip: null,
        country: null,
        latency_ms: null,
        error: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setChecking(false);
    }
  }

  const option = (checked: boolean) =>
    [
      "flex cursor-pointer items-center gap-3 rounded-lg border px-3.5 py-3",
      "transition-colors motion-reduce:transition-none",
      "focus-within:ring-2 focus-within:ring-accent/60",
      checked
        ? "border-accent bg-accent/5"
        : "border-border bg-surface-1 hover:border-border-hover",
    ].join(" ");

  return (
    <fieldset className="space-y-2">
      <legend className="label">{t("pform.proxyLabel")}</legend>

      <label className={option(form.proxy_id === null)}>
        <input
          type="radio"
          name="pf-proxy"
          className="h-4 w-4 accent-accent focus:outline-none"
          checked={form.proxy_id === null}
          onChange={() => set("proxy_id", null)}
        />
        <span className="text-sm font-medium text-fg">
          {t("pform.noProxy")}
        </span>
      </label>

      {proxies.map((p) => {
        const health = (p as ProxyWithHealth).health_status;
        const checked = form.proxy_id === p.id;
        const showResult =
          checkedId === p.id && (checking || checkResult !== null);
        return (
          <div key={p.id}>
            <label className={option(checked)}>
              <input
                type="radio"
                name="pf-proxy"
                className="h-4 w-4 accent-accent focus:outline-none"
                checked={checked}
                onChange={() => set("proxy_id", p.id)}
              />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium text-fg">
                  {p.name}
                </span>
                <span className="block truncate font-mono text-xs text-fg-muted">
                  {p.protocol}://{p.host}:{p.port}
                </span>
              </span>
              {health && (
                <span
                  className={[
                    "shrink-0 rounded-full px-2 py-0.5 text-xs font-medium",
                    health === "ok"
                      ? "bg-success/10 text-success"
                      : "bg-danger/10 text-danger",
                  ].join(" ")}
                >
                  {health === "ok"
                    ? t("pform.healthOk")
                    : t("pform.healthFail")}
                </span>
              )}
              {checked && (
                <button
                  type="button"
                  disabled={checking}
                  onClick={(e) => {
                    e.preventDefault();
                    void runCheck(p.id);
                  }}
                  className="btn-secondary shrink-0 px-2.5 py-1 text-xs disabled:cursor-not-allowed disabled:opacity-60"
                >
                  {checking && checkedId === p.id ? (
                    <Loader2
                      className="h-3.5 w-3.5 animate-spin"
                      aria-hidden="true"
                    />
                  ) : null}
                  {t("proxycheck.button")}
                </button>
              )}
            </label>
            {showResult && (
              <p
                role="status"
                aria-live="polite"
                className={[
                  "mt-1 px-3.5 text-xs",
                  checking
                    ? "text-fg-muted"
                    : checkResult?.ok
                      ? "text-success"
                      : "text-danger",
                ].join(" ")}
              >
                {checking
                  ? t("proxycheck.checking")
                  : checkResult?.ok
                    ? [
                        checkResult.external_ip,
                        checkResult.country,
                        checkResult.latency_ms != null
                          ? t("proxycheck.latency", {
                              ms: checkResult.latency_ms,
                            })
                          : null,
                      ]
                        .filter(Boolean)
                        .join(" · ")
                    : `${t("proxycheck.failed")}: ${checkResult?.error ?? ""}`}
              </p>
            )}
          </div>
        );
      })}

      {/* (W42) Rotate-on-launch: only meaningful with a proxy assigned */}
      <div className="flex items-center justify-between gap-3 pt-1">
        <label
          htmlFor="pf-rotate-on-launch"
          className={[
            "text-sm",
            form.proxy_id === null ? "text-fg-muted" : "text-fg cursor-pointer",
          ].join(" ")}
        >
          {t("pform.rotateOnLaunch")}
        </label>
        <Toggle
          id="pf-rotate-on-launch"
          checked={form.rotate_on_launch}
          onChange={(v) => set("rotate_on_launch", v)}
          disabled={form.proxy_id === null}
          label={t("pform.rotateOnLaunch")}
        />
      </div>

      {/* (P3-3b) Create + assign a proxy from a saved proxy template */}
      {templates.length > 0 && (
        <div className="rounded-lg border border-border bg-surface-1 px-3.5 py-3">
          <label htmlFor="pf-proxy-template" className="label">
            {t("pxtpl.useTemplate")}
          </label>
          <div className="flex items-center gap-2">
            <select
              id="pf-proxy-template"
              className="input flex-1"
              value={templateId}
              onChange={(e) => setTemplateId(e.target.value)}
              disabled={applying}
            >
              <option value="">{t("pxtpl.pickTemplate")}</option>
              {templates.map((tpl) => (
                <option key={tpl.id} value={tpl.id}>
                  {tpl.name} — {tpl.protocol}://{tpl.host}:{tpl.port}
                </option>
              ))}
            </select>
            <button
              type="button"
              disabled={!selectedTemplate || applying}
              onClick={() => void applyTemplate()}
              className="btn-secondary h-9 shrink-0 px-3 text-xs disabled:cursor-not-allowed disabled:opacity-60"
            >
              {applying && (
                <Loader2
                  className="h-3.5 w-3.5 animate-spin"
                  aria-hidden="true"
                />
              )}
              {t("pxtpl.apply")}
            </button>
          </div>
          {selectedTemplate &&
            (selectedTemplate.sticky_session ||
              selectedTemplate.traffic_saver) && (
              <p className="mt-1.5 flex flex-wrap gap-1.5 text-xs text-fg-muted">
                {selectedTemplate.sticky_session && (
                  <span className="rounded-full bg-accent/10 px-2 py-0.5 font-medium text-accent">
                    {t("pxtpl.stickySession")}
                  </span>
                )}
                {selectedTemplate.traffic_saver && (
                  <span className="rounded-full bg-accent/10 px-2 py-0.5 font-medium text-accent">
                    {t("pxtpl.trafficSaver")}
                  </span>
                )}
              </p>
            )}
          <p className="mt-1.5 text-xs text-fg-muted">
            {t("pxtpl.useTemplateHint")}
          </p>
          {applyError && (
            <p role="alert" className="mt-1.5 text-xs text-danger">
              {applyError}
            </p>
          )}
        </div>
      )}

      <p className="pt-1 text-xs text-fg-muted">{t("pform.manageProxies")}</p>
    </fieldset>
  );
}
