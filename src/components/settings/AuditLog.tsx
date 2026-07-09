import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AuditEntry, api, isTauri } from "../../lib/api";

const PAGE_SIZE = 50;

/** Action-prefix groups for the filter dropdown (dotted backend action names). */
const ACTION_PREFIXES = [
  "profile.",
  "proxy.",
  "proxy_template.",
  "folder.",
  "template.",
  "extension.",
  "cookies.",
  "cookierobot.",
  "backup.",
];

/** Compact single-line preview of the meta JSON (null-safe). */
function metaPreview(meta: unknown): string {
  if (meta === null || meta === undefined) return "";
  try {
    return JSON.stringify(meta);
  } catch {
    return String(meta);
  }
}

/**
 * (W26a) Audit-log viewer for Settings: time/action/target/meta table with an
 * action-prefix filter and cursor-based "Load more" (id descending, no offset).
 */
export function AuditLog() {
  const { t, i18n } = useTranslation();
  const [entries, setEntries] = useState<AuditEntry[]>([]);
  const [prefix, setPrefix] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());

  const load = useCallback(
    async (beforeId?: number) => {
      if (!isTauri()) return;
      setLoading(true);
      setError(false);
      try {
        // (W27) Fetch one extra row so hasMore is exact (no stray "Load more"
        // when the total is a multiple of PAGE_SIZE).
        const raw = await api.listAudit({
          actionPrefix: prefix || null,
          beforeId: beforeId ?? null,
          limit: PAGE_SIZE + 1,
        });
        const page = raw.slice(0, PAGE_SIZE);
        setEntries((prev) => (beforeId ? [...prev, ...page] : page));
        setHasMore(raw.length > PAGE_SIZE);
      } catch {
        setError(true);
      } finally {
        setLoading(false);
      }
    },
    [prefix],
  );

  useEffect(() => {
    setExpanded(new Set());
    load();
  }, [load]);

  const toggleExpand = (id: number) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const formatTs = (ts: string) => {
    const d = new Date(ts);
    return Number.isNaN(d.getTime())
      ? ts
      : d.toLocaleString(i18n.language, {
          dateStyle: "short",
          timeStyle: "medium",
        });
  };

  if (!isTauri()) {
    return <p className="text-sm text-fg-muted">{t("audit.desktopOnly")}</p>;
  }

  return (
    <div className="space-y-3">
      <select
        aria-label={t("audit.filterLabel")}
        className="input w-56 py-1.5 text-sm"
        value={prefix}
        onChange={(e) => setPrefix(e.target.value)}
      >
        <option value="">{t("audit.filterAll")}</option>
        {ACTION_PREFIXES.map((p) => (
          <option key={p} value={p}>
            {p.replace(/\.$/, "")}
          </option>
        ))}
      </select>

      <table className="w-full table-fixed text-sm">
        <thead>
          <tr className="border-b border-border text-left text-xs font-medium text-fg-muted">
            <th className="w-[150px] py-2 pr-3">{t("audit.colTime")}</th>
            <th className="w-[170px] py-2 pr-3">{t("audit.colAction")}</th>
            <th className="w-[110px] py-2 pr-3">{t("audit.colTarget")}</th>
            <th className="py-2">{t("audit.colMeta")}</th>
          </tr>
        </thead>
        <tbody>
          {entries.map((e) => {
            const open = expanded.has(e.id);
            const preview = metaPreview(e.meta);
            return (
              <tr key={e.id} className="border-b border-border align-top">
                <td className="py-3.5 pr-3 text-fg-muted">{formatTs(e.ts)}</td>
                <td
                  className="truncate py-3.5 pr-3 font-medium text-fg"
                  title={e.action}
                >
                  {e.action}
                </td>
                <td
                  className="truncate py-3.5 pr-3 text-fg-muted"
                  title={e.target_id ?? ""}
                >
                  {e.target_id ?? "—"}
                </td>
                <td className="py-3.5 text-fg-muted">
                  {preview ? (
                    <button
                      type="button"
                      onClick={() => toggleExpand(e.id)}
                      aria-expanded={open}
                      title={t(
                        open ? "audit.metaCollapse" : "audit.metaExpand",
                      )}
                      className="block w-full text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                    >
                      {open ? (
                        <pre className="whitespace-pre-wrap break-all font-mono text-xs">
                          {JSON.stringify(e.meta, null, 2)}
                        </pre>
                      ) : (
                        <span className="block truncate font-mono text-xs">
                          {preview}
                        </span>
                      )}
                    </button>
                  ) : (
                    "—"
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

      {!loading && entries.length === 0 && !error && (
        <p className="py-2 text-sm text-fg-muted">{t("audit.empty")}</p>
      )}
      {error && (
        <p className="text-xs text-danger" role="alert">
          {t("audit.loadFailed")}
        </p>
      )}
      {hasMore && (
        <button
          type="button"
          disabled={loading}
          onClick={() => load(entries[entries.length - 1]?.id)}
          className="btn-secondary h-9 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {loading ? t("audit.loading") : t("audit.loadMore")}
        </button>
      )}
    </div>
  );
}
