import { Puzzle, X } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { Extension } from "../../lib/api";
import { Toggle } from "./controls";
import type { FormState, SetField } from "./types";

interface ExtraTabProps {
  form: FormState;
  set: SetField;
  argsText: string;
  onArgsChange: (text: string) => void;
  argsError: string | null;
  /** (P3-1b) Central extension store — tick to assign to this profile. */
  storeExtensions: Extension[];
  assignedExtIds: Set<string>;
  onToggleExtension: (id: string) => void;
}

export function ExtraTab({
  form,
  set,
  argsText,
  onArgsChange,
  argsError,
  storeExtensions,
  assignedExtIds,
  onToggleExtension,
}: ExtraTabProps) {
  const { t } = useTranslation();
  const [extInput, setExtInput] = useState("");

  const addExtension = () => {
    const path = extInput.trim();
    if (!path) return;
    if (!form.extensions.includes(path)) {
      set("extensions", [...form.extensions, path]);
    }
    setExtInput("");
  };

  return (
    <div className="space-y-5">
      <div className="flex items-center justify-between gap-3">
        <label htmlFor="pf-headless" className="text-sm text-fg cursor-pointer">
          {t("pform.headless")}
        </label>
        <Toggle
          id="pf-headless"
          checked={form.headless}
          onChange={(v) => set("headless", v)}
          label={t("pform.headless")}
        />
      </div>

      <div className="flex items-center justify-between gap-3">
        <label htmlFor="pf-geoip" className="text-sm text-fg cursor-pointer">
          {t("pform.geoip")}
        </label>
        <Toggle
          id="pf-geoip"
          checked={form.geoip}
          onChange={(v) => set("geoip", v)}
          label={t("pform.geoip")}
        />
      </div>

      {/* Storage options (W20b): disabled kinds are wiped from disk on session stop */}
      <div>
        <span className="label">{t("pstorage.title")}</span>
        <p className="mb-2 text-xs text-fg-muted">{t("pstorage.hint")}</p>
        <div className="space-y-3">
          {(
            [
              {
                key: "store_history",
                id: "pf-store-history",
                label: t("pstorage.history"),
              },
              {
                key: "store_passwords",
                id: "pf-store-passwords",
                label: t("pstorage.passwords"),
              },
              {
                key: "store_sw_cache",
                id: "pf-store-sw-cache",
                label: t("pstorage.swCache"),
              },
            ] as const
          ).map(({ key, id, label }) => (
            <div key={key} className="flex items-center justify-between gap-3">
              <label htmlFor={id} className="text-sm text-fg cursor-pointer">
                {label}
              </label>
              <Toggle
                id={id}
                checked={form[key]}
                onChange={(v) => set(key, v)}
                label={label}
              />
            </div>
          ))}
        </div>
      </div>

      {/* Store extensions (P3-1b): tick-list of the central extension store */}
      <div>
        <span className="label" id="pf-store-ext-label">
          {t("ext.formTitle")}
        </span>
        <p className="mb-2 text-xs text-fg-muted">{t("ext.formHint")}</p>
        {storeExtensions.length === 0 ? (
          <p className="rounded-md bg-surface-2 px-2.5 py-2 text-xs text-fg-muted">
            {t("ext.noneInStore")}
          </p>
        ) : (
          <ul
            aria-labelledby="pf-store-ext-label"
            className="max-h-48 space-y-0.5 overflow-auto rounded-md border border-border p-1"
          >
            {storeExtensions.map((ext) => (
              <li key={ext.id}>
                <label className="flex cursor-pointer items-center gap-2 rounded-md px-1.5 py-1.5 text-sm text-fg hover:bg-surface-2">
                  <input
                    type="checkbox"
                    checked={assignedExtIds.has(ext.id)}
                    onChange={() => onToggleExtension(ext.id)}
                    className="h-4 w-4 rounded border-border accent-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                  />
                  <Puzzle
                    className={`h-4 w-4 shrink-0 ${ext.enabled ? "text-accent" : "text-fg-muted"}`}
                    aria-hidden="true"
                  />
                  <span
                    className={`truncate ${ext.enabled ? "" : "text-fg-muted"}`}
                  >
                    {ext.name}
                  </span>
                  {!ext.enabled && (
                    <span className="ml-auto shrink-0 rounded bg-surface-3 px-1.5 py-0.5 text-[10px] text-fg-muted">
                      {t("ext.disabledBadge")}
                    </span>
                  )}
                </label>
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* Extensions (W24b): local unpacked extension dirs, passed as --load-extension */}
      <div>
        <span className="label" id="pf-ext-label">
          {t("pform.extTitle")}
        </span>
        <p className="mb-2 text-xs text-fg-muted">{t("pform.extHint")}</p>
        {form.extensions.length > 0 && (
          <ul aria-label={t("pform.extTitle")} className="mb-2 space-y-1.5">
            {form.extensions.map((path, i) => (
              <li
                key={path}
                className="flex items-center gap-2 rounded-md bg-surface-2 px-2.5 py-1.5 text-sm text-fg"
              >
                <span className="min-w-0 flex-1 truncate font-mono text-xs">
                  {path}
                </span>
                <button
                  type="button"
                  onClick={() =>
                    set(
                      "extensions",
                      form.extensions.filter((_, j) => j !== i),
                    )
                  }
                  className="rounded-full text-fg-muted hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                  aria-label={t("pform.extRemove", { path })}
                >
                  <X className="h-3 w-3" aria-hidden="true" />
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="flex gap-2">
          <input
            id="pf-ext-path"
            className="input flex-1 font-mono text-xs"
            type="text"
            value={extInput}
            onChange={(e) => setExtInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                addExtension();
              }
            }}
            placeholder={t("pform.extPlaceholder")}
            aria-labelledby="pf-ext-label"
            spellCheck={false}
          />
          <button
            type="button"
            onClick={addExtension}
            className="btn-secondary px-2.5"
          >
            {t("pform.extAdd")}
          </button>
        </div>
      </div>

      <div>
        <label className="label" htmlFor="pf-args">
          {t("pform.launchArgs")}
        </label>
        <textarea
          id="pf-args"
          className={[
            "input min-h-[96px] resize-y font-mono text-xs",
            argsError
              ? "border-danger focus:border-danger focus:ring-danger/40"
              : "",
          ].join(" ")}
          value={argsText}
          onChange={(e) => onArgsChange(e.target.value)}
          placeholder={'["--lang=en"]'}
          spellCheck={false}
          aria-invalid={argsError !== null}
          aria-describedby={argsError ? "pf-args-error" : "pf-args-hint"}
        />
        {argsError ? (
          <p
            id="pf-args-error"
            role="alert"
            className="mt-1 text-xs text-danger"
          >
            {argsError}
          </p>
        ) : (
          <p id="pf-args-hint" className="mt-1 text-xs text-fg-muted">
            {t("pform.launchArgsHint")}
          </p>
        )}
      </div>
    </div>
  );
}
