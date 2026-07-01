import { Trash2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { Proxy, ProxyInput } from "../lib/api";

interface ProxyFormProps {
  proxies: Proxy[];
  onCreate: (input: ProxyInput) => Promise<void>;
  onDelete: (id: string) => Promise<void>;
}

const EMPTY: ProxyInput = {
  name: "",
  protocol: "http",
  host: "",
  port: 8080,
  username: null,
  password: null,
};

export function ProxyForm({ proxies, onCreate, onDelete }: ProxyFormProps) {
  const { t } = useTranslation();
  const [form, setForm] = useState<ProxyInput>(EMPTY);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

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

  return (
    <div className="p-6 max-w-2xl mx-auto space-y-6">
      <h2 className="text-lg font-semibold">{t("proxy.title")}</h2>

      {/* Existing proxies */}
      {proxies.length === 0 ? (
        <p className="text-xs text-fg-muted">{t("proxy.empty")}</p>
      ) : (
        <ul className="space-y-2">
          {proxies.map((p) => (
            <li
              key={p.id}
              className="flex items-center justify-between rounded-md border border-border bg-surface-1 px-3 py-2"
            >
              <div>
                <div className="text-sm font-medium">{p.name}</div>
                <div className="text-xs text-fg-muted font-mono">
                  {p.protocol}://{p.username ? `${p.username}@` : ""}{p.host}:{p.port}
                </div>
              </div>
              <button
                onClick={() => handleDelete(p.id)}
                className="btn-danger px-2"
                aria-label={`${t("proxy.delete")}: ${p.name}`}
              >
                <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
              </button>
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
