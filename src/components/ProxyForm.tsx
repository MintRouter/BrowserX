import { Trash2, TriangleAlert } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { Proxy, ProxyInput } from "../lib/api";

interface ProxyFormProps {
  proxies: Proxy[];
  onCreate: (input: ProxyInput) => Promise<void>;
  onDelete: (id: string) => Promise<void>;
  /** (W23b) Re-encrypt credentials of a proxy whose blobs no longer decrypt. */
  onReenterCredentials: (
    id: string,
    username: string | null,
    password: string,
  ) => Promise<void>;
}

const EMPTY: ProxyInput = {
  name: "",
  protocol: "http",
  host: "",
  port: 8080,
  username: null,
  password: null,
};

export function ProxyForm({ proxies, onCreate, onDelete, onReenterCredentials }: ProxyFormProps) {
  const { t } = useTranslation();
  const [form, setForm] = useState<ProxyInput>(EMPTY);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // (W23b) Per-proxy re-enter credential drafts, keyed by proxy id.
  const [reenter, setReenter] = useState<Record<string, { username: string; password: string }>>({});
  const [reenterSavingId, setReenterSavingId] = useState<string | null>(null);
  const anyInvalid = proxies.some((p) => p.credentials_invalid);

  const set = <K extends keyof ProxyInput>(key: K, value: ProxyInput[K]) => {
    setForm((prev) => ({ ...prev, [key]: value }));
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!form.name.trim() || !form.host.trim()) return;
    setSaving(true);
    setError(null);
    try {
      await onCreate(form);
      setForm(EMPTY);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async (id: string) => {
    if (!confirm(t("proxy.confirmDelete"))) return;
    await onDelete(id);
  };

  const setReenterField = (id: string, field: "username" | "password", value: string) => {
    setReenter((prev) => ({
      ...prev,
      [id]: { username: "", password: "", ...prev[id], [field]: value },
    }));
  };

  const handleReenter = async (id: string) => {
    const draft = reenter[id];
    if (!draft?.password) return;
    setReenterSavingId(id);
    setError(null);
    try {
      await onReenterCredentials(id, draft.username.trim() || null, draft.password);
      setReenter((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setReenterSavingId(null);
    }
  };

  return (
    <div className="p-6 max-w-2xl mx-auto space-y-6">
      <h2 className="text-lg font-semibold">{t("proxy.title")}</h2>

      {/* (W23b) Master key changed — some stored credentials no longer decrypt */}
      {anyInvalid && (
        <div
          role="alert"
          className="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-600 dark:text-amber-400"
        >
          <TriangleAlert className="h-4 w-4 shrink-0" aria-hidden="true" />
          <span>{t("proxy.credentialsBanner")}</span>
        </div>
      )}

      {/* Existing proxies */}
      {proxies.length === 0 ? (
        <p className="text-xs text-fg-muted">{t("proxy.empty")}</p>
      ) : (
        <ul className="space-y-2">
          {proxies.map((p) => (
            <li
              key={p.id}
              className="rounded-md border border-border bg-surface-1 px-3 py-2 space-y-2"
            >
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-sm font-medium">
                    {p.name}
                    {p.credentials_invalid && (
                      <span className="ml-2 inline-flex items-center gap-1 rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-normal text-amber-600 dark:text-amber-400">
                        <TriangleAlert className="h-3 w-3" aria-hidden="true" />
                        {t("proxy.credentialsInvalid")}
                      </span>
                    )}
                  </div>
                  <div className="text-xs text-fg-muted font-mono">
                    {p.protocol}://{p.username ? `${p.username}${p.has_password ? ":•••" : ""}@` : ""}{p.host}:{p.port}
                  </div>
                </div>
                <button
                  onClick={() => handleDelete(p.id)}
                  className="btn-danger px-2"
                  aria-label={`${t("proxy.delete")}: ${p.name}`}
                >
                  <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
                </button>
              </div>
              {/* (W23b) Re-enter credentials → backend re-encrypts with current key */}
              {p.credentials_invalid && (
                <form
                  onSubmit={(e) => {
                    e.preventDefault();
                    void handleReenter(p.id);
                  }}
                  className="flex items-center gap-2"
                >
                  <input
                    className="input flex-1 text-xs"
                    placeholder={t("proxy.username")}
                    aria-label={`${t("proxy.username")}: ${p.name}`}
                    value={reenter[p.id]?.username ?? ""}
                    onChange={(e) => setReenterField(p.id, "username", e.target.value)}
                    autoComplete="off"
                  />
                  <input
                    className="input flex-1 text-xs"
                    type="password"
                    placeholder={t("proxy.password")}
                    aria-label={`${t("proxy.password")}: ${p.name}`}
                    value={reenter[p.id]?.password ?? ""}
                    onChange={(e) => setReenterField(p.id, "password", e.target.value)}
                    autoComplete="new-password"
                    required
                  />
                  <button
                    type="submit"
                    className="btn-primary px-3"
                    disabled={!reenter[p.id]?.password || reenterSavingId === p.id}
                  >
                    {reenterSavingId === p.id ? t("form.saving") : t("proxy.reenterSave")}
                  </button>
                </form>
              )}
            </li>
          ))}
        </ul>
      )}

      {/* New proxy */}
      <form onSubmit={handleSubmit} className="space-y-3">
        <h3 className="text-xs font-semibold text-fg-muted uppercase tracking-wider">{t("proxy.newProxy")}</h3>
        <div className="grid grid-cols-2 gap-3">
          <div className="col-span-2">
            <label className="label" htmlFor="px-name">{t("proxy.name")}</label>
            <input id="px-name" className="input" value={form.name} onChange={(e) => set("name", e.target.value)} required />
          </div>
          <div>
            <label className="label" htmlFor="px-protocol">{t("proxy.protocol")}</label>
            <select
              id="px-protocol"
              className="input"
              value={form.protocol}
              onChange={(e) => set("protocol", e.target.value as ProxyInput["protocol"])}
            >
              <option value="http">HTTP</option>
              <option value="https">HTTPS</option>
              <option value="socks5">SOCKS5</option>
            </select>
          </div>
          <div>
            <label className="label" htmlFor="px-port">{t("proxy.port")}</label>
            <input
              id="px-port"
              className="input no-spin"
              type="number"
              min={1}
              max={65535}
              value={form.port}
              onChange={(e) => set("port", Number(e.target.value))}
              required
            />
          </div>
          <div className="col-span-2">
            <label className="label" htmlFor="px-host">{t("proxy.host")}</label>
            <input id="px-host" className="input" value={form.host} onChange={(e) => set("host", e.target.value)} required />
          </div>
          <div>
            <label className="label" htmlFor="px-user">{t("proxy.username")} ({t("proxy.optional")})</label>
            <input
              id="px-user"
              className="input"
              value={form.username ?? ""}
              onChange={(e) => set("username", e.target.value || null)}
              autoComplete="off"
            />
          </div>
          <div>
            <label className="label" htmlFor="px-pass">{t("proxy.password")} ({t("proxy.optional")})</label>
            <input
              id="px-pass"
              className="input"
              type="password"
              value={form.password ?? ""}
              onChange={(e) => set("password", e.target.value || null)}
              autoComplete="new-password"
            />
          </div>
        </div>
        {error && <p className="text-danger text-xs" role="alert">{error}</p>}
        <button type="submit" disabled={saving} className="btn-primary">
          {saving ? t("form.saving") : t("proxy.save")}
        </button>
      </form>
    </div>
  );
}
