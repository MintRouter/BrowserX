import { type ReactNode, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Folder, type Platform, type ProfileFilter } from "../lib/api";

/**
 * (P3-2b) Toolbar filter state. os/hasProxy/tag/folderId map to the backend
 * ProfileFilter (P3-2a); runtime is FE-only — filtered against runningIds,
 * never sent to SQL.
 */
export interface ProfileFilters {
  os?: Platform;
  hasProxy?: boolean;
  tag?: string;
  folderId?: string;
  runtime?: "running" | "stopped";
}

export const countActiveFilters = (f: ProfileFilters): number =>
  [f.os, f.hasProxy, f.tag, f.folderId, f.runtime].filter((v) => v !== undefined)
    .length;

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

function Field({
  id,
  label,
  value,
  onChange,
  children,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  children: ReactNode;
}) {
  return (
    <div>
      <label className="label" htmlFor={id}>
        {label}
      </label>
      <select
        id={id}
        className="input py-1.5"
        value={value}
        onChange={(e) => onChange(e.target.value)}
      >
        {children}
      </select>
    </div>
  );
}

/** Advanced filter panel for the profiles toolbar (opens from the tune button). */
export function FilterPanel({
  filters,
  folders,
  onChange,
}: {
  filters: ProfileFilters;
  folders: Folder[];
  onChange: (filters: ProfileFilters) => void;
}) {
  const { t } = useTranslation();
  const [tags, setTags] = useState<string[]>([]);

  useEffect(() => {
    api.listTags().then(setTags).catch(() => {});
  }, []);

  // Keep a stale selected tag visible so the select stays consistent.
  const tagOptions =
    filters.tag && !tags.includes(filters.tag) ? [filters.tag, ...tags] : tags;

  return (
    <div className="w-64 space-y-3 p-3">
      <Field
        id="flt-os"
        label={t("toolbar.filterOs")}
        value={filters.os ?? ""}
        onChange={(v) =>
          onChange({ ...filters, os: v === "" ? undefined : (v as Platform) })
        }
      >
        <option value="">{t("toolbar.filterAny")}</option>
        {OS_OPTIONS.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </Field>
      <Field
        id="flt-proxy"
        label={t("toolbar.filterProxy")}
        value={filters.hasProxy === undefined ? "" : filters.hasProxy ? "yes" : "no"}
        onChange={(v) =>
          onChange({ ...filters, hasProxy: v === "" ? undefined : v === "yes" })
        }
      >
        <option value="">{t("toolbar.filterAny")}</option>
        <option value="yes">{t("toolbar.filterWithProxy")}</option>
        <option value="no">{t("toolbar.filterWithoutProxy")}</option>
      </Field>
      <Field
        id="flt-tag"
        label={t("toolbar.filterTag")}
        value={filters.tag ?? ""}
        onChange={(v) => onChange({ ...filters, tag: v === "" ? undefined : v })}
      >
        <option value="">{t("toolbar.filterAny")}</option>
        {tagOptions.map((tag) => (
          <option key={tag} value={tag}>
            {tag}
          </option>
        ))}
      </Field>
      <Field
        id="flt-folder"
        label={t("toolbar.filterFolder")}
        value={filters.folderId ?? ""}
        onChange={(v) => onChange({ ...filters, folderId: v === "" ? undefined : v })}
      >
        <option value="">{t("toolbar.filterAny")}</option>
        {folders.map((f) => (
          <option key={f.id} value={f.id}>
            {f.name}
          </option>
        ))}
      </Field>
      <Field
        id="flt-status"
        label={t("toolbar.filterStatus")}
        value={filters.runtime ?? ""}
        onChange={(v) =>
          onChange({
            ...filters,
            runtime: v === "" ? undefined : (v as "running" | "stopped"),
          })
        }
      >
        <option value="">{t("toolbar.filterAny")}</option>
        <option value="running">{t("table.running")}</option>
        <option value="stopped">{t("table.stopped")}</option>
      </Field>
      <button
        type="button"
        className="btn-secondary w-full py-1 text-xs"
        disabled={countActiveFilters(filters) === 0}
        onClick={() => onChange({})}
      >
        {t("toolbar.filterClear")}
      </button>
    </div>
  );
}
