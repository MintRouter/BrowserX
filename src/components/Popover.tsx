import { Folder, X } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Folder as ApiFolder } from "../lib/api";

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
}

export function MenuItem({ icon, children, onClick, disabled, danger }: MenuItemProps) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
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
