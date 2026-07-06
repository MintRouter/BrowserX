import {
  EllipsisVertical,
  Pencil,
  Plus,
  Search,
  SearchX,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { type ReactNode, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  ProxyTemplate,
  ProxyTemplateCreate,
  ProxyTemplatePatch,
} from "../lib/api";
import { MenuItem, Popover } from "./Popover";
import { Toggle } from "./profile-form/controls";
import { TableFooter } from "./TableFooter";

interface ProxyTemplatesViewProps {
  templates: ProxyTemplate[];
  settings: Record<string, string> | null;
  profileCount: number;
  onCreate: (input: ProxyTemplateCreate) => Promise<void>;
  onUpdate: (id: string, patch: ProxyTemplatePatch) => Promise<void>;
  onDelete: (ids: string[]) => Promise<void>;
}

const PROTOCOL_LABELS: Record<ProxyTemplate["protocol"], string> = {
  http: "HTTP",
  https: "HTTPS",
  socks5: "SOCKS5",
};

function ToolButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick?: () => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      disabled={disabled}
      className="inline-flex h-[30px] w-[30px] shrink-0 items-center justify-center rounded-md text-[#1D192B] transition-colors hover:bg-surface-2 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent"
    >
      {children}
    </button>
  );
}

export function ProxyTemplatesView(props: ProxyTemplatesViewProps) {
  const { templates } = props;
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(100);
  /** null = closed · "new" = create dialog · ProxyTemplate = edit dialog. */
  const [dialog, setDialog] = useState<ProxyTemplate | "new" | null>(null);
  const [menuId, setMenuId] = useState<string | null>(null);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return templates;
    return templates.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        p.host.toLowerCase().includes(q) ||
        `${p.host}:${p.port}`.includes(q) ||
        p.protocol.includes(q),
    );
  }, [templates, search]);

  useEffect(() => {
    setPage(0);
  }, [search, rowsPerPage, templates.length]);

  // Drop selected ids that no longer exist (e.g. after delete).
  useEffect(() => {
    setSelected((prev) => {
      const next = new Set(
        [...prev].filter((id) => templates.some((p) => p.id === id)),
      );
      return next.size === prev.size ? prev : next;
    });
  }, [templates]);

  const totalPages = Math.max(1, Math.ceil(filtered.length / rowsPerPage));
  const safePage = Math.min(page, totalPages - 1);
  const paged = filtered.slice(safePage * rowsPerPage, (safePage + 1) * rowsPerPage);
  const pageIds = paged.map((p) => p.id);
  const allChecked = paged.length > 0 && paged.every((p) => selected.has(p.id));
  const someChecked = paged.some((p) => selected.has(p.id));

  const singleSelected =
    selected.size === 1
      ? (templates.find((p) => selected.has(p.id)) ?? null)
      : null;
  const anyInvalid = templates.some((p) => p.credentials_invalid);

  const toggleRow = (id: string) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setSelected(next);
  };

  const togglePage = (select: boolean) => {
    const next = new Set(selected);
    for (const id of pageIds) {
      if (select) next.add(id);
      else next.delete(id);
    }
    setSelected(next);
  };

  const handleDelete = (ids: string[]) => {
    if (ids.length === 0) return;
    const message =
      ids.length === 1
        ? t("pxtpl.confirmDelete")
        : t("pxtpl.confirmDeleteMany", { count: ids.length });
    if (!confirm(message)) return;
    void props.onDelete(ids);
  };

  const th = "h-10 px-3 text-left align-middle text-xs font-medium text-fg";

  return (
    <div className="flex h-full flex-col p-4">
      <div className="card flex min-h-0 flex-1 flex-col overflow-hidden">
        {/* (W23b) Master key changed — some stored credentials no longer decrypt */}
        {anyInvalid && (
          <div
            role="alert"
            className="flex items-start gap-2 border-b border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-600 dark:text-amber-400"
          >
            <TriangleAlert className="h-4 w-4 shrink-0" aria-hidden="true" />
            <span>{t("proxy.credentialsBanner")}</span>
          </div>
        )}

        <div className="flex min-h-[60px] flex-wrap items-center gap-3 p-3">
          <button
            type="button"
            onClick={() => setDialog("new")}
            className="btn-primary h-9 py-1.5"
          >
            <Plus className="h-4 w-4" aria-hidden="true" />
            <span>{t("toolbar.create")}</span>
          </button>

          {/* Action icons only appear while rows are selected (ML parity) */}
          {selected.size > 0 && (
            <>
              <span className="mx-1 h-5 w-px bg-border" aria-hidden="true" />
              <ToolButton
                label={t("pxtpl.editTemplate")}
                disabled={!singleSelected}
                onClick={() => singleSelected && setDialog(singleSelected)}
              >
                <Pencil className="h-4 w-4" aria-hidden="true" />
              </ToolButton>
              <ToolButton
                label={t("pxtpl.delete")}
                onClick={() => handleDelete([...selected])}
              >
                <Trash2 className="h-4 w-4" aria-hidden="true" />
              </ToolButton>
            </>
          )}

          <div className="relative ml-auto w-[225px]">
            <Search
              className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-muted"
              aria-hidden="true"
            />
            <input
              type="search"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("pxtpl.searchPlaceholder")}
              aria-label={t("pxtpl.searchPlaceholder")}
              className="h-9 w-full rounded-md border border-border bg-surface-2 pl-9 pr-3 text-sm text-fg placeholder:text-fg-muted focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/50"
            />
          </div>
        </div>

        {templates.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 p-12 text-center">
            <svg
              width="140"
              height="100"
              viewBox="0 0 140 100"
              fill="none"
              aria-hidden="true"
            >
              <rect x="26" y="22" width="62" height="72" rx="6" fill="#F1EDED" transform="rotate(-8 26 22)" />
              <rect x="50" y="10" width="62" height="80" rx="6" fill="#FFFFFF" stroke="#E5E1E1" strokeWidth="1.5" />
              <rect x="60" y="24" width="34" height="5" rx="2.5" fill="#E5E1E1" />
              <rect x="60" y="36" width="42" height="5" rx="2.5" fill="#F1EDED" />
              <rect x="60" y="48" width="26" height="5" rx="2.5" fill="#F1EDED" />
              <circle cx="106" cy="72" r="16" fill="#F0F6FF" />
              <path d="M106 66v12M100 72h12" stroke="#055FF0" strokeWidth="2.5" strokeLinecap="round" />
            </svg>
            <p className="text-xl font-medium text-fg">{t("pxtpl.emptyTitle")}</p>
            <p className="max-w-xs text-sm text-fg-muted">{t("pxtpl.emptyHint")}</p>
            <button
              type="button"
              onClick={() => setDialog("new")}
              className="btn-primary mt-1"
            >
              <Plus className="h-4 w-4" aria-hidden="true" />
              <span>{t("toolbar.create")}</span>
            </button>
          </div>
        ) : filtered.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 p-12 text-center">
            <SearchX className="h-8 w-8 text-fg-muted/50" aria-hidden="true" />
            <p className="text-sm text-fg-muted">{t("table.noMatches")}</p>
          </div>
        ) : (
          <div className="flex-1 overflow-auto">
            <table className="w-full text-sm">
              <thead className="sticky top-0 z-10 border-b border-border bg-surface-2">
                <tr className="h-10 border-b border-border">
                  <th scope="col" className="w-10 px-3 align-middle">
                    <input
                      type="checkbox"
                      aria-label={t("table.selectAll")}
                      checked={allChecked}
                      ref={(el) => {
                        if (el) el.indeterminate = someChecked && !allChecked;
                      }}
                      onChange={() => togglePage(!allChecked)}
                      className="h-4 w-4 cursor-pointer rounded border-border accent-accent"
                    />
                  </th>
                  <th scope="col" className={th}>{t("proxy.name")}</th>
                  <th scope="col" className={th}>{t("proxy.details")}</th>
                  <th scope="col" className={th}>{t("proxy.protocol")}</th>
                  <th scope="col" className={th}>{t("pxtpl.stickySession")}</th>
                  <th scope="col" className={th}>{t("pxtpl.trafficSaver")}</th>
                  <th scope="col" className="w-10 px-1 align-middle">
                    <span className="sr-only">{t("table.rowMenu")}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {paged.map((p) => {
                  const isSelected = selected.has(p.id);
                  return (
                    <tr
                      key={p.id}
                      className={`h-[49px] border-b border-border transition-colors [&>td]:align-middle ${
                        isSelected ? "bg-[#F0F6FF]" : "hover:bg-accent/[0.03]"
                      }`}
                    >
                      <td className="px-3 py-2">
                        <input
                          type="checkbox"
                          aria-label={`${t("table.selectRow")}: ${p.name}`}
                          checked={isSelected}
                          onChange={() => toggleRow(p.id)}
                          className="h-4 w-4 cursor-pointer rounded border-border accent-accent"
                        />
                      </td>
                      <td className="max-w-0 px-3 py-2">
                        <div className="flex items-center gap-2">
                          <span className="truncate font-medium text-fg">{p.name}</span>
                          {p.credentials_invalid && (
                            <span className="inline-flex shrink-0 items-center gap-1 rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-600 dark:text-amber-400">
                              <TriangleAlert className="h-3 w-3" aria-hidden="true" />
                              {t("proxy.credentialsInvalid")}
                            </span>
                          )}
                        </div>
                      </td>
                      <td className="max-w-0 truncate px-3 py-2 text-fg-muted">
                        {p.masked_username ? `${p.masked_username}@` : ""}
                        {p.host}:{p.port}
                      </td>
                      <td className="px-3 py-2 text-fg-muted">{PROTOCOL_LABELS[p.protocol]}</td>
                      <td className="px-3 py-2 text-fg-muted">
                        {p.sticky_session ? t("pxtpl.on") : "—"}
                      </td>
                      <td className="px-3 py-2 text-fg-muted">
                        {p.traffic_saver ? t("pxtpl.on") : "—"}
                      </td>
                      <td className="px-1 py-2">
                        <Popover
                          open={menuId === p.id}
                          onClose={() => setMenuId(null)}
                          align="end"
                          label={t("table.rowMenu")}
                          trigger={
                            <button
                              type="button"
                              aria-label={`${t("table.rowMenu")}: ${p.name}`}
                              aria-haspopup="menu"
                              aria-expanded={menuId === p.id}
                              onClick={() => setMenuId(menuId === p.id ? null : p.id)}
                              className="grid h-8 w-8 place-items-center rounded-full text-fg-muted transition-colors hover:bg-surface-2 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                            >
                              <EllipsisVertical className="h-4 w-4" aria-hidden="true" />
                            </button>
                          }
                        >
                          <div role="menu" className="w-44">
                            <MenuItem
                              icon={<Pencil className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
                              onClick={() => {
                                setMenuId(null);
                                setDialog(p);
                              }}
                            >
                              {t("pxtpl.editTemplate")}
                            </MenuItem>
                            <MenuItem
                              danger
                              icon={<Trash2 className="h-4 w-4" aria-hidden="true" />}
                              onClick={() => {
                                setMenuId(null);
                                handleDelete([p.id]);
                              }}
                            >
                              {t("pxtpl.delete")}
                            </MenuItem>
                          </div>
                        </Popover>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}

        <TableFooter
          total={filtered.length}
          page={safePage}
          rowsPerPage={rowsPerPage}
          onPageChange={setPage}
          onRowsPerPageChange={setRowsPerPage}
          profileCount={props.profileCount}
          settings={props.settings}
        />
      </div>

      {dialog !== null && (
        <ProxyTemplateDialog
          template={dialog === "new" ? null : dialog}
          onClose={() => setDialog(null)}
          onCreate={props.onCreate}
          onUpdate={props.onUpdate}
        />
      )}
    </div>
  );
}

interface ProxyTemplateDialogProps {
  /** null = create a new template. */
  template: ProxyTemplate | null;
  onClose: () => void;
  onCreate: (input: ProxyTemplateCreate) => Promise<void>;
  onUpdate: (id: string, patch: ProxyTemplatePatch) => Promise<void>;
}

/** Create/edit proxy-template modal — mirrors ProxyDialog (W5c/W23b masked
 * credentials) and adds the sticky-session / traffic-saver metadata toggles. */
function ProxyTemplateDialog({
  template,
  onClose,
  onCreate,
  onUpdate,
}: ProxyTemplateDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState(template?.name ?? "");
  const [protocol, setProtocol] = useState<ProxyTemplate["protocol"]>(
    template?.protocol ?? "http",
  );
  const [host, setHost] = useState(template?.host ?? "");
  const [port, setPort] = useState(template?.port ?? 8080);
  // (W5c) Plaintext credentials never cross IPC — the form starts blank and
  // blank means "keep the stored value" (same semantics as ProxyDialog).
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [clearCreds, setClearCreds] = useState(false);
  const [stickySession, setStickySession] = useState(
    template?.sticky_session ?? false,
  );
  const [trafficSaver, setTrafficSaver] = useState(
    template?.traffic_saver ?? false,
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const hasStoredCreds = Boolean(
    template && (template.masked_username || template.has_password),
  );

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || !host.trim()) return;
    setSaving(true);
    setError(null);
    try {
      if (template) {
        // Blank fields = keep the stored credentials; typed values re-encrypt
        // (covers the W23b re-enter-credentials flow for invalid templates).
        const patch: ProxyTemplatePatch = {
          name: name.trim(),
          protocol,
          host: host.trim(),
          port,
          sticky_session: stickySession,
          traffic_saver: trafficSaver,
        };
        if (clearCreds) {
          patch.clear_credentials = true;
        } else {
          const nextUsername = username.trim();
          if (nextUsername) patch.username = nextUsername;
          if (password) patch.password = password;
        }
        await onUpdate(template.id, patch);
      } else {
        await onCreate({
          name: name.trim(),
          protocol,
          host: host.trim(),
          port,
          username: username.trim() || null,
          password: password || null,
          sticky_session: stickySession,
          traffic_saver: trafficSaver,
        });
      }
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={template ? t("pxtpl.editTemplate") : t("pxtpl.newTemplate")}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !saving) onClose();
      }}
    >
      <form onSubmit={handleSubmit} className="card w-full max-w-md p-5">
        <h2 className="text-base font-semibold text-fg">
          {template ? t("pxtpl.editTemplate") : t("pxtpl.newTemplate")}
        </h2>

        {/* (W23b) Credentials no longer decrypt — re-enter the password here */}
        {template?.credentials_invalid && (
          <p
            role="alert"
            className="mt-3 flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-600 dark:text-amber-400"
          >
            <TriangleAlert className="h-4 w-4 shrink-0" aria-hidden="true" />
            <span>{t("proxy.credentialsBanner")}</span>
          </p>
        )}

        <div className="mt-4 grid grid-cols-2 gap-3">
          <div className="col-span-2">
            <label className="label" htmlFor="pxt-name">{t("proxy.name")}</label>
            <input
              id="pxt-name"
              className="input"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
            />
          </div>
          <div>
            <label className="label" htmlFor="pxt-protocol">{t("proxy.protocol")}</label>
            <select
              id="pxt-protocol"
              className="input"
              value={protocol}
              onChange={(e) =>
                setProtocol(e.target.value as ProxyTemplate["protocol"])
              }
            >
              <option value="http">HTTP</option>
              <option value="https">HTTPS</option>
              <option value="socks5">SOCKS5</option>
            </select>
          </div>
          <div>
            <label className="label" htmlFor="pxt-port">{t("proxy.port")}</label>
            <input
              id="pxt-port"
              className="input no-spin"
              type="number"
              min={1}
              max={65535}
              value={port}
              onChange={(e) => setPort(Number(e.target.value))}
              required
            />
          </div>
          <div className="col-span-2">
            <label className="label" htmlFor="pxt-host">{t("proxy.host")}</label>
            <input
              id="pxt-host"
              className="input"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              required
            />
          </div>
          <div>
            <label className="label" htmlFor="pxt-user">
              {t("proxy.username")} ({t("proxy.optional")})
            </label>
            <input
              id="pxt-user"
              className="input"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder={template?.masked_username ?? undefined}
              disabled={clearCreds}
              autoComplete="off"
            />
            {template?.masked_username && !template.credentials_invalid && (
              <p className="mt-1 text-xs text-fg-muted">{t("proxy.usernameKeep")}</p>
            )}
          </div>
          <div>
            <label className="label" htmlFor="pxt-pass">
              {t("proxy.password")} ({t("proxy.optional")})
            </label>
            <input
              id="pxt-pass"
              className="input"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={template?.has_password ? "•••" : undefined}
              disabled={clearCreds}
              autoComplete="new-password"
            />
            {template?.has_password && !template.credentials_invalid && (
              <p className="mt-1 text-xs text-fg-muted">{t("proxy.passwordKeep")}</p>
            )}
          </div>
          {hasStoredCreds && (
            <label className="col-span-2 flex items-center gap-2 text-xs text-fg-muted">
              <input
                type="checkbox"
                checked={clearCreds}
                onChange={(e) => setClearCreds(e.target.checked)}
                className="h-4 w-4 rounded border-border accent-accent"
              />
              {t("proxy.clearCredentials")}
            </label>
          )}

          {/* Provider metadata (P3-3a): not applied at launch */}
          <div className="col-span-2 space-y-3 rounded-md bg-surface-2 px-3 py-3">
            <div className="flex items-center justify-between gap-3">
              <label htmlFor="pxt-sticky" className="cursor-pointer text-sm text-fg">
                {t("pxtpl.stickySession")}
              </label>
              <Toggle
                id="pxt-sticky"
                checked={stickySession}
                onChange={setStickySession}
                label={t("pxtpl.stickySession")}
              />
            </div>
            <div className="flex items-center justify-between gap-3">
              <label htmlFor="pxt-saver" className="cursor-pointer text-sm text-fg">
                {t("pxtpl.trafficSaver")}
              </label>
              <Toggle
                id="pxt-saver"
                checked={trafficSaver}
                onChange={setTrafficSaver}
                label={t("pxtpl.trafficSaver")}
              />
            </div>
            <p className="text-xs text-fg-muted">{t("pxtpl.metadataHint")}</p>
          </div>
        </div>

        {error && (
          <p className="mt-3 text-xs text-danger" role="alert">
            {error}
          </p>
        )}

        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            className="btn-secondary px-3 py-1.5 text-sm"
            disabled={saving}
            onClick={onClose}
          >
            {t("form.cancel")}
          </button>
          <button
            type="submit"
            className="btn-primary px-3 py-1.5 text-sm"
            disabled={saving}
          >
            {saving ? t("form.saving") : t("pxtpl.save")}
          </button>
        </div>
      </form>
    </div>
  );
}
