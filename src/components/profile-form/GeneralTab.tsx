import {
  Bold,
  Code,
  Italic,
  Redo2,
  RemoveFormatting,
  Search,
  Strikethrough,
  Underline,
  Undo2,
  X,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Folder, ProfileTemplate } from "../../lib/api";
import { Segmented, Toggle } from "./controls";
import { isValidStartupUrl } from "./types";
import type { FormState, SetField } from "./types";

const NOTE_MAX = 1500;

interface GeneralTabProps {
  form: FormState;
  set: SetField;
  folders: Folder[];
  allTags: string[];
  /** True in create mode: autofocus + select-all the name input. */
  autoFocusName: boolean;
  /** (W20b) Saved templates for the create-mode dropdown. */
  templates?: ProfileTemplate[];
  selectedTemplateId?: string;
  /** Undefined in edit mode → dropdown disabled. */
  onApplyTemplate?: (id: string) => void;
  /** (R6 2.2) Toggle state: save the form as a template on submit. */
  saveAsTemplate?: boolean;
  onSaveAsTemplateChange?: (next: boolean) => void;
}

export function GeneralTab({
  form,
  set,
  folders,
  allTags,
  autoFocusName,
  templates,
  selectedTemplateId,
  onApplyTemplate,
  saveAsTemplate,
  onSaveAsTemplateChange,
}: GeneralTabProps) {
  const { t } = useTranslation();
  const nameRef = useRef<HTMLInputElement>(null);
  const [tagQuery, setTagQuery] = useState("");
  const [tagOpen, setTagOpen] = useState(false);
  const [urlInput, setUrlInput] = useState("");
  const [urlError, setUrlError] = useState(false);

  useEffect(() => {
    if (autoFocusName && nameRef.current) {
      nameRef.current.focus();
      nameRef.current.select();
    }
  }, [autoFocusName]);

  const addTag = (raw: string) => {
    const tag = raw.trim();
    if (!tag || form.tags.includes(tag)) return;
    set("tags", [...form.tags, tag]);
    setTagQuery("");
  };

  const suggestions = allTags.filter(
    (tag) =>
      !form.tags.includes(tag) &&
      tag.toLowerCase().includes(tagQuery.trim().toLowerCase()),
  );

  const addStartupUrl = () => {
    const url = urlInput.trim();
    if (!url) return;
    if (!isValidStartupUrl(url)) {
      setUrlError(true);
      return;
    }
    if (!form.startup_urls.includes(url)) {
      set("startup_urls", [...form.startup_urls, url]);
    }
    setUrlInput("");
    setUrlError(false);
  };

  const comingSoon = t("pform.comingSoon");

  return (
    <div className="space-y-5">
      {/* (R6 2.2) Save as template: 44×20 toggle switch, label left / switch right */}
      <div className="flex items-center justify-between gap-3">
        <span className="text-xs font-medium text-[#1D192B] dark:text-fg">{t("pform.saveTemplate")}</span>
        <Toggle
          id="pf-save-template"
          checked={saveAsTemplate ?? false}
          onChange={onSaveAsTemplateChange}
          disabled={!onSaveAsTemplateChange}
          label={t("pform.saveTemplate")}
        />
      </div>

      {/* Profile name */}
      <div>
        <label className="label" htmlFor="pf-name">{t("pform.profileName")}</label>
        <input
          ref={nameRef}
          id="pf-name"
          className="input"
          value={form.name}
          onChange={(e) => set("name", e.target.value)}
          placeholder={t("pform.namePlaceholder")}
          required
        />
      </div>

      {/* Template + Folder */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div>
          <label className="label" htmlFor="pf-template">{t("pform.profileTemplate")}</label>
          <select
            id="pf-template"
            className="input"
            value={selectedTemplateId ?? ""}
            onChange={(e) => onApplyTemplate?.(e.target.value)}
            disabled={!onApplyTemplate}
          >
            <option value="">{t("ptpl.none")}</option>
            {(templates ?? []).map((tp) => (
              <option key={tp.id} value={tp.id}>{tp.name}</option>
            ))}
          </select>
        </div>
        <div>
          <label className="label" htmlFor="pf-folder">{t("pform.folder")}</label>
          <select
            id="pf-folder"
            className="input"
            value={form.folder_id ?? ""}
            onChange={(e) => set("folder_id", e.target.value || null)}
          >
            <option value="">{t("pform.noFolder")}</option>
            {folders.map((f) => (
              <option key={f.id} value={f.id}>{f.name}</option>
            ))}
          </select>
        </div>
      </div>

      {/* Tags combobox */}
      <div>
        <label className="label" htmlFor="pf-tags">{t("pform.tags")}</label>
        {form.tags.length > 0 && (
          <div className="mb-2 flex flex-wrap gap-1.5">
            {form.tags.map((tag) => (
              <span
                key={tag}
                className="inline-flex items-center gap-1 rounded-full bg-surface-2 px-2.5 py-1 text-xs text-fg"
              >
                {tag}
                <button
                  type="button"
                  onClick={() => set("tags", form.tags.filter((x) => x !== tag))}
                  className="rounded-full text-fg-muted hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                  aria-label={t("pform.removeTag", { tag })}
                >
                  <X className="h-3 w-3" aria-hidden="true" />
                </button>
              </span>
            ))}
          </div>
        )}
        <div className="relative">
          <Search
            className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-muted"
            aria-hidden="true"
          />
          <input
            id="pf-tags"
            className="input pl-8"
            role="combobox"
            aria-expanded={tagOpen && suggestions.length > 0}
            aria-controls="pf-tags-listbox"
            aria-autocomplete="list"
            value={tagQuery}
            onChange={(e) => { setTagQuery(e.target.value); setTagOpen(true); }}
            onFocus={() => setTagOpen(true)}
            onBlur={() => setTagOpen(false)}
            onKeyDown={(e) => {
              if (e.key === "Enter") { e.preventDefault(); addTag(tagQuery); }
              if (e.key === "Escape") setTagOpen(false);
            }}
            placeholder={t("pform.tagsPlaceholder")}
          />
          {tagOpen && suggestions.length > 0 && (
            <ul
              id="pf-tags-listbox"
              role="listbox"
              aria-label={t("pform.tagSuggestions")}
              className="absolute z-10 mt-1 max-h-40 w-full overflow-y-auto rounded-md border border-border bg-surface-1 py-1 shadow-lg"
            >
              {suggestions.map((tag) => (
                <li key={tag} role="option" aria-selected={false}>
                  <button
                    type="button"
                    className="block w-full px-3 py-1.5 text-left text-sm text-fg hover:bg-surface-2 focus:outline-none focus-visible:bg-surface-2"
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => addTag(tag)}
                  >
                    {tag}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
        <p className="mt-1 text-xs text-fg-muted">{t("pform.tagsHint")}</p>
      </div>

      {/* Note with (coming soon) toolbar + counter */}
      <div>
        <label className="label" htmlFor="pf-note">{t("pform.note")}</label>
        <div className="rounded-md border border-border bg-surface-1 focus-within:border-accent focus-within:ring-2 focus-within:ring-accent/50">
          <div
            className="flex items-center gap-0.5 border-b border-border px-1.5 py-1"
            role="toolbar"
            aria-label={t("pform.noteToolbar")}
          >
            {/* (R6 2.3) 8 buttons like ML: B/I/U/S/code/clear/undo/redo */}
            {[
              { Icon: Bold, key: "bold", fallback: "Bold" },
              { Icon: Italic, key: "italic", fallback: "Italic" },
              { Icon: Underline, key: "underline", fallback: "Underline" },
              { Icon: Strikethrough, key: "strikethrough", fallback: "Strikethrough" },
              { Icon: Code, key: "code", fallback: "Code" },
              { Icon: RemoveFormatting, key: "clear", fallback: "Clear formatting" },
              { Icon: Undo2, key: "undo", fallback: "Undo" },
              { Icon: Redo2, key: "redo", fallback: "Redo" },
            ].map(({ Icon, key, fallback }) => (
              <span key={key} title={comingSoon}>
                <button
                  type="button"
                  disabled
                  title={comingSoon}
                  aria-label={`${t(`pform.fmt.${key}`, fallback)} — ${comingSoon}`}
                  className="rounded p-1.5 text-fg-muted opacity-50 cursor-not-allowed"
                >
                  <Icon className="h-3.5 w-3.5" aria-hidden="true" />
                </button>
              </span>
            ))}
          </div>
          <textarea
            id="pf-note"
            className="block min-h-[96px] w-full resize-y rounded-b-md bg-transparent px-2.5 py-2 text-sm text-fg placeholder:text-fg-muted/60 focus:outline-none"
            maxLength={NOTE_MAX}
            value={form.notes}
            onChange={(e) => set("notes", e.target.value)}
            placeholder={t("pform.notePlaceholder")}
            aria-describedby="pf-note-counter"
          />
          <div className="flex justify-end px-2.5 pb-1.5">
            <span id="pf-note-counter" className="text-xs text-fg-muted">
              {form.notes.length} / {NOTE_MAX}
            </span>
          </div>
        </div>
      </div>

      {/* Startup behavior: restore previous session or open custom URLs */}
      <div>
        <span className="label" id="pf-startup-label">{t("pform.startupBehavior")}</span>
        <Segmented
          options={[
            { value: "restore", label: t("pform.startupRestore") },
            { value: "custom", label: t("pform.startupCustom") },
          ]}
          value={form.startup_behavior}
          onChange={(v) => set("startup_behavior", v)}
          label={t("pform.startupBehavior")}
        />
        <p className="mt-1.5 text-xs text-fg-muted">{t("pform.startupHelp")}</p>
        {form.startup_behavior === "custom" && (
          <div className="mt-3">
            {form.startup_urls.length > 0 && (
              <ul aria-label={t("pform.startupUrlsLabel")} className="mb-2 space-y-1.5">
                {form.startup_urls.map((url, i) => (
                  <li
                    key={url}
                    className="flex items-center gap-2 rounded-md bg-surface-2 px-2.5 py-1.5 text-sm text-fg"
                  >
                    <span className="min-w-0 flex-1 truncate">{url}</span>
                    <button
                      type="button"
                      onClick={() =>
                        set("startup_urls", form.startup_urls.filter((_, j) => j !== i))
                      }
                      className="rounded-full text-fg-muted hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                      aria-label={t("pform.startupUrlRemove", { url })}
                    >
                      <X className="h-3 w-3" aria-hidden="true" />
                    </button>
                  </li>
                ))}
              </ul>
            )}
            <div className="flex gap-2">
              <input
                id="pf-startup-url"
                className="input flex-1"
                type="text"
                inputMode="url"
                value={urlInput}
                onChange={(e) => {
                  setUrlInput(e.target.value);
                  setUrlError(false);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    addStartupUrl();
                  }
                }}
                placeholder={t("pform.startupUrlPlaceholder")}
                aria-labelledby="pf-startup-label"
                aria-invalid={urlError}
                aria-describedby={urlError ? "pf-startup-url-error" : undefined}
              />
              <button type="button" onClick={addStartupUrl} className="btn-secondary px-2.5">
                {t("pform.startupUrlAdd")}
              </button>
            </div>
            {urlError && (
              <p id="pf-startup-url-error" role="alert" className="mt-1 text-xs text-danger">
                {t("pform.startupUrlInvalid")}
              </p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
