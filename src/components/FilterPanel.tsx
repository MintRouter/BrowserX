import { Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Folder, type Platform, type ProfileFilter } from "../lib/api";

/**
 * (P3-2b) Toolbar filter state. os/hasProxy/tag/folderId map to the backend
 * ProfileFilter (P3-2a); name + runtime are FE-only — matched in ProfileList,
 * never sent to SQL.
 */
export interface ProfileFilters {
  name?: string;
  os?: Platform;
  hasProxy?: boolean;
  tag?: string;
  folderId?: string;
  runtime?: "running" | "stopped";
}

export const countActiveFilters = (f: ProfileFilters): number =>
  [f.name, f.os, f.hasProxy, f.tag, f.folderId, f.runtime].filter(
    (v) => v !== undefined,
  ).length;

/** Map panel state to the backend ProfileFilter shape (serde snake_case). */
export const toProfileFilter = (f: ProfileFilters): ProfileFilter => ({
  os: f.os,
  has_proxy: f.hasProxy,
  tag: f.tag,
  folder_id: f.folderId,
});

const OS_OPTIONS: { value: Platform; label: string }[] = [
  { value: "windows", label: "Windows" },
  { value: "macos", label: "macOS" },
  { value: "linux", label: "Linux" },
];

/** Attributes selectable in the filter-builder rows (MLX order). */
type FilterAttr = "name" | "os" | "hasProxy" | "tag" | "folderId" | "runtime";
const ATTRS: FilterAttr[] = ["name", "os", "hasProxy", "tag", "folderId", "runtime"];
const ATTR_LABEL_KEY: Record<FilterAttr, string> = {
  name: "toolbar.filterName",
  os: "toolbar.filterOs",
  hasProxy: "toolbar.filterProxy",
  tag: "toolbar.filterTag",
  folderId: "toolbar.filterFolder",
  runtime: "toolbar.filterStatus",
};

const selectCls =
  "h-9 w-full rounded-md border border-border bg-surface-2 px-2 text-sm text-fg focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/50";

/**
 * (W50H) Advanced filter panel rebuilt as the MLX horizontal filter-builder:
 * rows of [attribute select][value control][×] + "+ Add filter" + Reset/OK.
 * Draft state applies only on OK; Reset clears everything immediately.
 */
export function FilterPanel({
  filters,
  folders,
  onChange,
  onClose,
}: {
  filters: ProfileFilters;
  folders: Folder[];
  onChange: (filters: ProfileFilters) => void;
  /** Close the popover after OK/Reset. */
  onClose?: () => void;
}) {
  const { t } = useTranslation();
  const [tags, setTags] = useState<string[]>([]);
  const [draft, setDraft] = useState<ProfileFilters>(filters);
  const [rows, setRows] = useState<FilterAttr[]>(() => {
    const active = ATTRS.filter((a) => filters[a] !== undefined);
    return active.length > 0 ? active : ["name"];
  });

  useEffect(() => {
    api
      .listTags()
      .then((list) => setTags(list.map((t) => t.tag)))
      .catch(() => {});
  }, []);

  // Keep a stale selected tag visible so the select stays consistent.
  const tagOptions =
    draft.tag && !tags.includes(draft.tag) ? [draft.tag, ...tags] : tags;

  const setValue = (attr: FilterAttr, value: ProfileFilters[FilterAttr]) =>
    setDraft((d) => ({ ...d, [attr]: value }));

  const swapAttr = (index: number, next: FilterAttr) => {
    const prev = rows[index];
    if (!prev) return;
    setRows((r) => r.map((a, i) => (i === index ? next : a)));
    setDraft((d) => ({ ...d, [prev]: undefined }));
  };

  const removeRow = (index: number) => {
    const attr = rows[index];
    if (!attr) return;
    setDraft((d) => ({ ...d, [attr]: undefined }));
    setRows((r) =>
      r.length === 1 ? ["name"] : r.filter((_, i) => i !== index),
    );
  };

  const unused = ATTRS.filter((a) => !rows.includes(a));

  const apply = () => {
    const clean: ProfileFilters = { ...draft };
    if (!clean.name?.trim()) clean.name = undefined;
    onChange(clean);
    onClose?.();
  };

  const reset = () => {
    setDraft({});
    setRows(["name"]);
    onChange({});
    onClose?.();
  };

  const valueControl = (attr: FilterAttr, index: number) => {
    const id = `flt-${index}`;
    switch (attr) {
      case "name":
        return (
          <input
            id={id}
            type="text"
            value={draft.name ?? ""}
            placeholder={t("toolbar.filterEnterText")}
            onChange={(e) =>
              setValue("name", e.target.value === "" ? undefined : e.target.value)
            }
            className={selectCls}
          />
        );
      case "os":
        return (
          <select
            id={id}
            className={selectCls}
            value={draft.os ?? ""}
            onChange={(e) =>
              setValue(
                "os",
                e.target.value === "" ? undefined : (e.target.value as Platform),
              )
            }
          >
            <option value="">{t("toolbar.filterAny")}</option>
            {OS_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        );
      case "hasProxy":
        return (
          <select
            id={id}
            className={selectCls}
            value={draft.hasProxy === undefined ? "" : draft.hasProxy ? "yes" : "no"}
            onChange={(e) =>
              setValue(
                "hasProxy",
                e.target.value === "" ? undefined : e.target.value === "yes",
              )
            }
          >
            <option value="">{t("toolbar.filterAny")}</option>
            <option value="yes">{t("toolbar.filterWithProxy")}</option>
            <option value="no">{t("toolbar.filterWithoutProxy")}</option>
          </select>
        );
      case "tag":
        return (
          <select
            id={id}
            className={selectCls}
            value={draft.tag ?? ""}
            onChange={(e) =>
              setValue("tag", e.target.value === "" ? undefined : e.target.value)
            }
          >
            <option value="">{t("toolbar.filterAny")}</option>
            {tagOptions.map((tag) => (
              <option key={tag} value={tag}>
                {tag}
              </option>
            ))}
          </select>
        );
      case "folderId":
        return (
          <select
            id={id}
            className={selectCls}
            value={draft.folderId ?? ""}
            onChange={(e) =>
              setValue(
                "folderId",
                e.target.value === "" ? undefined : e.target.value,
              )
            }
          >
            <option value="">{t("toolbar.filterAny")}</option>
            {folders.map((f) => (
              <option key={f.id} value={f.id}>
                {f.name}
              </option>
            ))}
          </select>
        );
      case "runtime":
        return (
          <select
            id={id}
            className={selectCls}
            value={draft.runtime ?? ""}
            onChange={(e) =>
              setValue(
                "runtime",
                e.target.value === ""
                  ? undefined
                  : (e.target.value as "running" | "stopped"),
              )
            }
          >
            <option value="">{t("toolbar.filterAny")}</option>
            <option value="running">{t("table.running")}</option>
            <option value="stopped">{t("table.stopped")}</option>
          </select>
        );
    }
  };

  return (
    <div className="w-[550px] space-y-3 p-4">
      {rows.map((attr, index) => (
        <div key={`${attr}-${index}`} className="flex items-center gap-2">
          <select
            aria-label={t("toolbar.filters")}
            className={`${selectCls} w-[232px] shrink-0`}
            value={attr}
            onChange={(e) => swapAttr(index, e.target.value as FilterAttr)}
          >
            {ATTRS.filter((a) => a === attr || !rows.includes(a)).map((a) => (
              <option key={a} value={a}>
                {t(ATTR_LABEL_KEY[a])}
              </option>
            ))}
          </select>
          <div className="min-w-0 flex-1">{valueControl(attr, index)}</div>
          <button
            type="button"
            aria-label={t("toolbar.filterRemove")}
            onClick={() => removeRow(index)}
            className="grid h-8 w-8 shrink-0 place-items-center rounded-md text-fg-muted transition-colors hover:bg-surface-2 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
          >
            <X className="h-4 w-4" aria-hidden="true" />
          </button>
        </div>
      ))}

      <button
        type="button"
        disabled={unused.length === 0}
        onClick={() => {
          const next = unused[0];
          if (next) setRows((r) => [...r, next]);
        }}
        className="inline-flex items-center gap-1 rounded text-sm font-medium text-accent hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:no-underline"
      >
        <Plus className="h-4 w-4" aria-hidden="true" />
        {t("toolbar.filterAdd")}
      </button>

      <div className="flex gap-2 pt-1">
        <button
          type="button"
          onClick={reset}
          className="h-8 flex-1 rounded-md bg-[#F0F6FF] text-sm font-medium text-accent transition-colors hover:bg-[#E0EDFF] focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
        >
          {t("toolbar.filterReset")}
        </button>
        <button
          type="button"
          onClick={apply}
          className="h-8 flex-1 rounded-md bg-accent text-sm font-medium text-white transition-colors hover:bg-accent-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
        >
          {t("toolbar.filterOk")}
        </button>
      </div>
    </div>
  );
}
