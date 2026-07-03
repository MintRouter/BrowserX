import {
  EllipsisVertical,
  Laptop,
  Pencil,
  Plus,
  Search,
  SearchX,
  Trash2,
} from "lucide-react";
import { type ReactNode, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  DEFAULT_TEMPLATE_SETTING,
  type Platform,
  type ProfileInput,
  type ProfileTemplate,
  type Proxy,
} from "../lib/api";
import { MenuItem, Popover } from "./Popover";
import { TableFooter } from "./TableFooter";

interface TemplatesViewProps {
  templates: ProfileTemplate[];
  proxies: Proxy[];
  /** Total non-trashed profile count — feeds the footer stats. */
  profileCount: number;
  settings: Record<string, string> | null;
  onCreate: (name: string, config: ProfileInput) => Promise<void>;
  onUpdate: (id: string, name: string, config: ProfileInput) => Promise<void>;
  onDelete: (ids: string[]) => Promise<void>;
  onSetDefault: (id: string) => Promise<void>;
}

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

export function TemplatesView(props: TemplatesViewProps) {
  const { templates } = props;
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(100);
  /** null = closed · "new" = create dialog · ProfileTemplate = edit dialog. */
  const [dialog, setDialog] = useState<ProfileTemplate | "new" | null>(null);
  const [menuId, setMenuId] = useState<string | null>(null);

  const defaultId = props.settings?.[DEFAULT_TEMPLATE_SETTING] || null;

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return templates;
    return templates.filter(
      (x) =>
        x.name.toLowerCase().includes(q) ||
        (x.config.notes ?? "").toLowerCase().includes(q),
    );
  }, [templates, search]);

  useEffect(() => {
    setPage(0);
  }, [search, rowsPerPage, templates.length]);

  // Drop selected ids that no longer exist (e.g. after delete).
  useEffect(() => {
    setSelected((prev) => {
      const next = new Set(
        [...prev].filter((id) => templates.some((x) => x.id === id)),
      );
      return next.size === prev.size ? prev : next;
    });
  }, [templates]);

  const totalPages = Math.max(1, Math.ceil(filtered.length / rowsPerPage));
  const safePage = Math.min(page, totalPages - 1);
  const paged = filtered.slice(safePage * rowsPerPage, (safePage + 1) * rowsPerPage);
  const pageIds = paged.map((x) => x.id);
  const allChecked = paged.length > 0 && paged.every((x) => selected.has(x.id));
  const someChecked = paged.some((x) => selected.has(x.id));

  const singleSelected =
    selected.size === 1
      ? (templates.find((x) => selected.has(x.id)) ?? null)
      : null;

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
        ? t("tpl.confirmDelete")
        : t("tpl.confirmDeleteMany", { count: ids.length });
    if (!confirm(message)) return;
    void props.onDelete(ids);
  };

  const th = "h-10 px-3 text-left align-middle text-xs font-medium text-fg";

  return (
    <div className="flex h-full flex-col p-4">
      {/* Toolbar lives inside the table's white card (ML table-header parity, F2b) */}
      <div className="card flex min-h-0 flex-1 flex-col overflow-hidden">
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
                label={t("tpl.editTemplate")}
                disabled={!singleSelected}
                onClick={() => singleSelected && setDialog(singleSelected)}
              >
                <Pencil className="h-4 w-4" aria-hidden="true" />
              </ToolButton>
              <ToolButton
                label={t("tpl.delete")}
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
              placeholder={t("tpl.searchPlaceholder")}
              aria-label={t("tpl.searchPlaceholder")}
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
            <p className="text-xl font-medium text-fg">{t("tpl.emptyTitle")}</p>
            <p className="max-w-xs text-sm text-fg-muted">{t("tpl.emptyHint")}</p>
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
                  <th scope="col" className={th}>{t("tpl.name")}</th>
                  <th scope="col" className={th}>{t("tpl.storage")}</th>
                  <th scope="col" className={th}>{t("tpl.notes")}</th>
                  <th scope="col" className="w-40 px-3 align-middle">
                    <span className="sr-only">{t("tpl.setDefault")}</span>
                  </th>
                  <th scope="col" className="w-10 px-1 align-middle">
                    <span className="sr-only">{t("table.rowMenu")}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {paged.map((x) => {
                  const isSelected = selected.has(x.id);
                  const isDefault = defaultId === x.id;
                  return (
                    <tr
                      key={x.id}
                      onClick={() => setDialog(x)}
                      className={`h-[49px] cursor-pointer border-b border-border transition-colors [&>td]:align-middle ${
                        isSelected ? "bg-[#F0F6FF]" : "hover:bg-accent/[0.03]"
                      }`}
                    >
                      <td className="px-3 py-2" onClick={(e) => e.stopPropagation()}>
                        <input
                          type="checkbox"
                          aria-label={`${t("table.selectRow")}: ${x.name}`}
                          checked={isSelected}
                          onChange={() => toggleRow(x.id)}
                          className="h-4 w-4 cursor-pointer rounded border-border accent-accent"
                        />
                      </td>
                      <td className="max-w-0 px-3 py-2">
                        <div className="flex items-center gap-2">
                          <span className="truncate font-medium text-fg">{x.name}</span>
                          <Laptop className="h-4 w-4 shrink-0 text-accent" aria-hidden="true" />
                        </div>
                      </td>
                      <td className="whitespace-nowrap px-3 py-2 text-fg-muted">{t("tpl.storageLocal")}</td>
                      <td className="max-w-0 truncate px-3 py-2 text-fg-muted">
                        {x.config.notes?.trim() || "—"}
                      </td>
                      <td className="px-3 text-right" onClick={(e) => e.stopPropagation()}>
                        {isDefault ? (
                          <span className="inline-flex h-8 items-center rounded-md px-3 align-middle text-sm font-medium text-fg-muted">
                            {t("tpl.default")}
                          </span>
                        ) : (
                          <button
                            type="button"
                            onClick={() => void props.onSetDefault(x.id)}
                            className="inline-flex h-8 items-center rounded-md bg-[#F0F6FF] px-3 align-middle text-sm font-medium text-accent transition-colors hover:bg-[#E0EDFF] focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                          >
                            {t("tpl.setDefault")}
                          </button>
                        )}
                      </td>
                      <td className="px-1 py-2" onClick={(e) => e.stopPropagation()}>
                        <Popover
                          open={menuId === x.id}
                          onClose={() => setMenuId(null)}
                          align="end"
                          label={t("table.rowMenu")}
                          trigger={
                            <button
                              type="button"
                              aria-label={`${t("table.rowMenu")}: ${x.name}`}
                              aria-haspopup="menu"
                              aria-expanded={menuId === x.id}
                              onClick={() => setMenuId(menuId === x.id ? null : x.id)}
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
                                setDialog(x);
                              }}
                            >
                              {t("tpl.editTemplate")}
                            </MenuItem>
                            {!isDefault && (
                              <MenuItem
                                icon={<Laptop className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
                                onClick={() => {
                                  setMenuId(null);
                                  void props.onSetDefault(x.id);
                                }}
                              >
                                {t("tpl.setDefault")}
                              </MenuItem>
                            )}
                            <MenuItem
                              danger
                              icon={<Trash2 className="h-4 w-4" aria-hidden="true" />}
                              onClick={() => {
                                setMenuId(null);
                                handleDelete([x.id]);
                              }}
                            >
                              {t("tpl.delete")}
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
        <TemplateDialog
          template={dialog === "new" ? null : dialog}
          proxies={props.proxies}
          onClose={() => setDialog(null)}
          onCreate={props.onCreate}
          onUpdate={props.onUpdate}
        />
      )}
    </div>
  );
}

interface TemplateDialogProps {
  /** null = create a new template. */
  template: ProfileTemplate | null;
  proxies: Proxy[];
  onClose: () => void;
  onCreate: (name: string, config: ProfileInput) => Promise<void>;
  onUpdate: (id: string, name: string, config: ProfileInput) => Promise<void>;
}

/** Create/edit template modal — same pattern as ProxyDialog (F2a). */
function TemplateDialog({
  template,
  proxies,
  onClose,
  onCreate,
  onUpdate,
}: TemplateDialogProps) {
  const { t } = useTranslation();
  const cfg = template?.config;
  const [name, setName] = useState(template?.name ?? "");
  const [platform, setPlatform] = useState<Platform>(cfg?.platform ?? "windows");
  const [proxyId, setProxyId] = useState<string>(cfg?.proxy_id ?? "");
  const [startUrl, setStartUrl] = useState(cfg?.startup_urls?.[0] ?? "");
  const [notes, setNotes] = useState(cfg?.notes ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    setSaving(true);
    setError(null);
    try {
      const url = startUrl.trim();
      // Keep any config fields the dialog does not expose (fingerprint etc.).
      const config: ProfileInput = {
        ...(cfg ?? {}),
        name: "",
        platform,
        proxy_id: proxyId || null,
        startup_behavior: url ? "custom" : (cfg?.startup_behavior ?? "restore"),
        startup_urls: url ? [url] : [],
        notes: notes.trim() || null,
      };
      if (template) {
        await onUpdate(template.id, name.trim(), config);
      } else {
        await onCreate(name.trim(), config);
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
      aria-label={template ? t("tpl.editTemplate") : t("tpl.newTemplate")}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !saving) onClose();
      }}
    >
      <form onSubmit={handleSubmit} className="card w-full max-w-md p-5">
        <h2 className="text-base font-semibold text-fg">
          {template ? t("tpl.editTemplate") : t("tpl.newTemplate")}
        </h2>

        <div className="mt-4 grid grid-cols-2 gap-3">
          <div className="col-span-2">
            <label className="label" htmlFor="tpl-name">{t("tpl.templateName")}</label>
            <input
              id="tpl-name"
              className="input"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
              required
            />
          </div>
          <div>
            <label className="label" htmlFor="tpl-os">{t("tpl.os")}</label>
            <select
              id="tpl-os"
              className="input"
              value={platform}
              onChange={(e) => setPlatform(e.target.value as Platform)}
            >
              <option value="windows">Windows</option>
              <option value="macos">macOS</option>
              <option value="linux">Linux</option>
            </select>
          </div>
          <div>
            <label className="label" htmlFor="tpl-proxy">{t("tpl.proxy")}</label>
            <select
              id="tpl-proxy"
              className="input"
              value={proxyId}
              onChange={(e) => setProxyId(e.target.value)}
            >
              <option value="">{t("tpl.noProxy")}</option>
              {proxies.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </div>
          <div className="col-span-2">
            <label className="label" htmlFor="tpl-url">
              {t("tpl.startUrl")} ({t("tpl.optional")})
            </label>
            <input
              id="tpl-url"
              className="input"
              type="url"
              value={startUrl}
              onChange={(e) => setStartUrl(e.target.value)}
              placeholder="https://example.com"
            />
          </div>
          <div className="col-span-2">
            <label className="label" htmlFor="tpl-notes">
              {t("tpl.notes")} ({t("tpl.optional")})
            </label>
            <textarea
              id="tpl-notes"
              className="input min-h-[72px] resize-y"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
            />
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
            {saving ? t("form.saving") : t("tpl.save")}
          </button>
        </div>
      </form>
    </div>
  );
}
