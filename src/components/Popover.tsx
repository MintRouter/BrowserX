import { Folder, Loader2, Puzzle, X } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Extension, type Folder as ApiFolder } from "../lib/api";

interface PopoverProps {
  open: boolean;
  onClose: () => void;
  trigger: ReactNode;
  children: ReactNode;
  align?: "start" | "end";
  label?: string;
  panelClassName?: string;
}

/** Anchored popover/menu shell: closes on outside click and Escape. */
export function Popover({
  open,
  onClose,
  trigger,
  children,
  align = "start",
  label,
  panelClassName = "",
}: PopoverProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, onClose]);

  return (
    <div ref={ref} className="relative inline-flex">
      {trigger}
      {open && (
        <div
          role="dialog"
          aria-label={label}
          className={`absolute top-full z-30 mt-1 min-w-[180px] card p-1 text-sm ${
            align === "end" ? "right-0" : "left-0"
          } ${panelClassName}`}
        >
          {children}
        </div>
      )}
    </div>
  );
}

interface MenuItemProps {
  icon?: ReactNode;
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  danger?: boolean;
  title?: string;
}

export function MenuItem({ icon, children, onClick, disabled, danger, title }: MenuItemProps) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      title={title}
      onClick={onClick}
      className={`flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-sm transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-40 ${
        danger ? "text-danger hover:bg-danger/10" : "text-fg hover:bg-surface-2"
      }`}
    >
      {icon}
      <span className="flex-1 truncate">{children}</span>
    </button>
  );
}

/** Folder picker list: "Default folder" (null) + user folders. */
export function FolderPanel({
  folders,
  onPick,
}: {
  folders: ApiFolder[];
  onPick: (folderId: string | null) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="max-h-64 w-56 overflow-auto" role="menu">
      <MenuItem
        icon={<Folder className="h-4 w-4 shrink-0 text-fg-muted" aria-hidden="true" />}
        onClick={() => onPick(null)}
      >
        {t("toolbar.defaultFolder")}
      </MenuItem>
      {folders.map((f) => (
        <MenuItem
          key={f.id}
          icon={<Folder className="h-4 w-4 shrink-0 text-accent" aria-hidden="true" />}
          onClick={() => onPick(f.id)}
        >
          {f.name}
        </MenuItem>
      ))}
    </div>
  );
}

/**
 * (P3-1b) Per-profile extension assignment panel for the row menu.
 * Self-contained: loads the store list + current assignment, saves on apply.
 */
export function ExtensionsPanel({
  profileId,
  onDone,
}: {
  profileId: string;
  /** Called after a successful save — close the menu. */
  onDone: () => void;
}) {
  const { t } = useTranslation();
  const [extensions, setExtensions] = useState<Extension[] | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    Promise.all([api.listExtensions(), api.getProfileExtensions(profileId)])
      .then(([all, assigned]) => {
        if (cancelled) return;
        setExtensions(all);
        setChecked(new Set(assigned.map((e) => e.id)));
      })
      .catch((err) => {
        if (!cancelled) {
          setExtensions([]);
          setError(err instanceof Error ? err.message : String(err));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [profileId]);

  const toggle = (id: string) => {
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.assignExtensions(profileId, [...checked]);
      onDone();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  };

  if (extensions === null) {
    return (
      <div className="grid w-56 place-items-center p-4">
        <Loader2 className="h-4 w-4 animate-spin text-fg-muted" aria-hidden="true" />
        <span className="sr-only">{t("ext.loading")}</span>
      </div>
    );
  }

  if (extensions.length === 0) {
    return (
      <div className="w-56 p-3 text-center text-xs">
        {error && (
          <p role="alert" className="mb-1 text-danger">
            {error}
          </p>
        )}
        <p className="text-fg-muted">{t("ext.noneInStore")}</p>
      </div>
    );
  }

  return (
    <div className="w-60 p-2">
      <ul className="max-h-56 space-y-0.5 overflow-auto" aria-label={t("ext.title")}>
      {extensions.map((ext) => (
        <li key={ext.id}>
          <label className="flex cursor-pointer items-center gap-2 rounded-md px-1.5 py-1.5 text-sm text-fg hover:bg-surface-2">
            <input
              type="checkbox"
              checked={checked.has(ext.id)}
              onChange={() => toggle(ext.id)}
              disabled={busy}
              className="h-4 w-4 rounded border-border accent-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
            />
            <Puzzle
              className={`h-4 w-4 shrink-0 ${ext.enabled ? "text-accent" : "text-fg-muted"}`}
              aria-hidden="true"
            />
            <span className={`truncate ${ext.enabled ? "" : "text-fg-muted"}`}>
              {ext.name}
            </span>
          </label>
        </li>
      ))}
      </ul>
      {error && (
        <p role="alert" className="mt-2 text-xs text-danger">
          {error}
        </p>
      )}
      <button
        type="button"
        className="btn-primary mt-2 inline-flex w-full items-center justify-center gap-1.5 py-1 text-xs"
        disabled={busy}
        onClick={() => void apply()}
      >
        {busy && <Loader2 className="h-3 w-3 animate-spin" aria-hidden="true" />}
        {t("toolbar.apply")}
      </button>
    </div>
  );
}

/** Small tag composer: type + Enter to stack tags, then apply the batch. */
export function TagPanel({ onApply }: { onApply: (tags: string[]) => void }) {
  const { t } = useTranslation();
  const [value, setValue] = useState("");
  const [tags, setTags] = useState<string[]>([]);

  const pending = () => {
    const v = value.trim();
    return v && !tags.includes(v) ? [...tags, v] : tags;
  };

  return (
    <div className="w-60 p-2">
      <input
        autoFocus
        className="input py-1 text-xs"
        value={value}
        placeholder={t("toolbar.tagsPlaceholder")}
        aria-label={t("toolbar.addTags")}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            setTags(pending());
            setValue("");
          }
        }}
      />
      {tags.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1">
          {tags.map((tag) => (
            <span
              key={tag}
              className="inline-flex items-center gap-1 rounded-full bg-surface-2 px-2 py-0.5 text-xs text-fg"
            >
              {tag}
              <button
                type="button"
                aria-label={`${t("toolbar.removeTag")}: ${tag}`}
                onClick={() => setTags(tags.filter((x) => x !== tag))}
                className="rounded-full text-fg-muted hover:text-danger focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
              >
                <X className="h-3 w-3" aria-hidden="true" />
              </button>
            </span>
          ))}
        </div>
      )}
      <button
        type="button"
        className="btn-primary mt-2 w-full py-1 text-xs"
        disabled={pending().length === 0}
        onClick={() => onApply(pending())}
      >
        {t("toolbar.apply")}
      </button>
    </div>
  );
}
