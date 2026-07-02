import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { api, type Proxy, type ProxyCheckResult } from "../../lib/api";
import type { FormState, SetField } from "./types";

interface ProxyTabProps {
  form: FormState;
  set: SetField;
  proxies: Proxy[];
}

/** Optional health status — present in newer backend payloads. */
type ProxyWithHealth = Proxy & { health_status?: string | null };

export function ProxyTab({ form, set, proxies }: ProxyTabProps) {
  const { t } = useTranslation();

  const [checking, setChecking] = useState(false);
  const [checkedId, setCheckedId] = useState<string | null>(null);
  const [checkResult, setCheckResult] = useState<ProxyCheckResult | null>(null);

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
        <span className="text-sm font-medium text-fg">{t("pform.noProxy")}</span>
      </label>

      {proxies.map((p) => {
        const health = (p as ProxyWithHealth).health_status;
        const checked = form.proxy_id === p.id;
        const showResult = checkedId === p.id && (checking || checkResult !== null);
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
                <span className="block truncate text-sm font-medium text-fg">{p.name}</span>
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
                  {health === "ok" ? t("pform.healthOk") : t("pform.healthFail")}
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
                    <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
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
                          ? t("proxycheck.latency", { ms: checkResult.latency_ms })
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

      <p className="pt-1 text-xs text-fg-muted">{t("pform.manageProxies")}</p>
    </fieldset>
  );
}
