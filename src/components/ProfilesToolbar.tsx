import {
  ArrowUpDown,
  ChevronDown,
  Cookie,
  Copy,
  Download,
  EllipsisVertical,
  Eraser,
  FolderInput,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Search,
  SlidersHorizontal,
  Square,
  Tag,
  Trash2,
  Upload,
  Zap,
} from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Folder } from "../lib/api";
import { FolderPanel, MenuItem, Popover, TagPanel } from "./Popover";
import type { ProfilesSort } from "./ProfileTable";

interface ProfilesToolbarProps {
  search: string;
  onSearchChange: (value: string) => void;
  selectedCount: number;
  hasRunningSelected: boolean;
  folders: Folder[];
  sort: ProfilesSort;
  onSortChange: (sort: ProfilesSort) => void;
  onNewProfile: () => void;
  onQuickProfile: () => void;
  onImport: (file: File) => void;
  onLaunchSelected: () => void;
  onStopSelected: () => void;
  onRefresh: () => void;
  onEditSelected: () => void;
  onAddTags: (tags: string[]) => void;
  onMoveToFolder: (folderId: string | null) => void;
  onCloneSelected: () => void;
  /** (W25a) Bulk export the selected profiles to .bxprofile files. */
  onExportSelected: () => void;
  /** (W24a) Bulk cookie export for the selected profiles. */
  onExportCookiesSelected: () => void;
  onClearCacheSelected: () => void;
  onTrashSelected: () => void;
  onClearSelection: () => void;
  /** (W20a) Increment to open the move-to-folder popover (⌘/Ctrl+Shift+M). */
  moveSignal?: number;
}

type MenuKey = "create" | "tags" | "move" | "sort" | "more" | null;

function ToolButton({
  label,
  title,
  onClick,
  disabled,
  expanded,
  children,
}: {
  label: string;
  title?: string;
  onClick?: () => void;
  disabled?: boolean;
  expanded?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={title ?? label}
      onClick={onClick}
      disabled={disabled}
      aria-expanded={expanded}
      aria-haspopup={expanded !== undefined ? "menu" : undefined}
      className="inline-flex h-[30px] w-[30px] shrink-0 items-center justify-center rounded-md text-[#1D192B] transition-colors hover:bg-surface-2 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent"
    >
      {children}
    </button>
  );
}

export function ProfilesToolbar(props: ProfilesToolbarProps) {
  const { t } = useTranslation();
  const [menu, setMenu] = useState<MenuKey>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const toggle = (key: Exclude<MenuKey, null>) =>
    setMenu((m) => (m === key ? null : key));
  const close = () => setMenu(null);

  const {
    selectedCount,
    hasRunningSelected,
    sort,
    onSortChange,
  } = props;
  const none = selectedCount === 0;
  const notSingle = selectedCount !== 1;

  // (W20a) Shortcut ⌘/Ctrl+Shift+M bumps moveSignal → open the move popover.
  const { moveSignal } = props;
  useEffect(() => {
    if (moveSignal) setMenu("move");
  }, [moveSignal]);

  const pickSort = (s: ProfilesSort) => {
    onSortChange(s);
    close();
  };

  return (
    <div className="flex min-h-[60px] flex-wrap items-center gap-3 p-3">
      {/* Split "+ Create" */}
      <div className="inline-flex">
        <button
          type="button"
          onClick={props.onNewProfile}
          className="btn-primary h-9 rounded-r-none py-1.5"
        >
          <Plus className="h-4 w-4" aria-hidden="true" />
          <span>{t("toolbar.create")}</span>
        </button>
        <Popover
          open={menu === "create"}
          onClose={close}
          label={t("toolbar.createMenu")}
          trigger={
            <button
              type="button"
              aria-label={t("toolbar.createMenu")}
              aria-haspopup="menu"
              aria-expanded={menu === "create"}
              onClick={() => toggle("create")}
              className="btn-primary h-9 rounded-l-none border-l border-white/30 px-1.5 py-1.5"
            >
              <ChevronDown className="h-4 w-4" aria-hidden="true" />
            </button>
          }
        >
          <MenuItem
            icon={<Plus className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
            onClick={() => {
              close();
              props.onNewProfile();
            }}
          >
            {t("toolbar.newProfile")}
          </MenuItem>
          <MenuItem
            icon={<Zap className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
            onClick={() => {
              close();
              props.onQuickProfile();
            }}
          >
            {t("toolbar.quickProfile")}
          </MenuItem>
        </Popover>
      </div>

      <button
        type="button"
        onClick={props.onQuickProfile}
        className="btn inline-flex h-9 bg-[#F1EDED] px-3 py-2 text-[#1D192B] hover:bg-[#E8E2E2]"
      >
        <Zap className="h-4 w-4" aria-hidden="true" />
        <span>{t("toolbar.quick")}</span>
      </button>

      {/* Action icons only appear while rows are selected (ML parity, 1.2) */}
      {selectedCount > 0 && (
        <>
          <span className="mx-1 h-5 w-px bg-border" aria-hidden="true" />

          <ToolButton
            label={t("toolbar.launchSelected")}
            disabled={none}
            onClick={props.onLaunchSelected}
          >
            <Play className="h-4 w-4" aria-hidden="true" />
          </ToolButton>
          <ToolButton
            label={t("toolbar.stopSelected")}
            disabled={!hasRunningSelected}
            onClick={props.onStopSelected}
          >
            <Square className="h-4 w-4" aria-hidden="true" />
          </ToolButton>
          <ToolButton label={t("toolbar.refresh")} onClick={props.onRefresh}>
            <RefreshCw className="h-4 w-4" aria-hidden="true" />
          </ToolButton>
          <ToolButton
            label={t("toolbar.editSelected")}
            disabled={notSingle}
            onClick={props.onEditSelected}
          >
            <Pencil className="h-4 w-4" aria-hidden="true" />
          </ToolButton>

          <Popover
            open={menu === "tags"}
            onClose={close}
            label={t("toolbar.addTags")}
            trigger={
              <ToolButton
                label={t("toolbar.addTags")}
                disabled={none}
                expanded={menu === "tags"}
                onClick={() => toggle("tags")}
              >
                <Tag className="h-4 w-4" aria-hidden="true" />
              </ToolButton>
            }
          >
            <TagPanel
              onApply={(tags) => {
                close();
                props.onAddTags(tags);
              }}
            />
          </Popover>

          <Popover
            open={menu === "move"}
            onClose={close}
            label={t("toolbar.moveToFolder")}
            trigger={
              <ToolButton
                label={t("toolbar.moveToFolder")}
                disabled={none}
                expanded={menu === "move"}
                onClick={() => toggle("move")}
              >
                <FolderInput className="h-4 w-4" aria-hidden="true" />
              </ToolButton>
            }
          >
            <FolderPanel
              folders={props.folders}
              onPick={(folderId) => {
                close();
                props.onMoveToFolder(folderId);
              }}
            />
          </Popover>

          <Popover
            open={menu === "sort"}
            onClose={close}
            label={t("toolbar.sort")}
            trigger={
              <ToolButton
                label={t("toolbar.sort")}
                expanded={menu === "sort"}
                onClick={() => toggle("sort")}
              >
                <ArrowUpDown className="h-4 w-4" aria-hidden="true" />
              </ToolButton>
            }
          >
            <MenuItem
              onClick={() => pickSort({ key: "name", dir: "asc" })}
              icon={sortDot(sort.key === "name" && sort.dir === "asc")}
            >
              {t("toolbar.sortNameAsc")}
            </MenuItem>
            <MenuItem
              onClick={() => pickSort({ key: "name", dir: "desc" })}
              icon={sortDot(sort.key === "name" && sort.dir === "desc")}
            >
              {t("toolbar.sortNameDesc")}
            </MenuItem>
            <MenuItem
              onClick={() => pickSort({ key: "updated", dir: "desc" })}
              icon={sortDot(sort.key === "updated")}
            >
              {t("toolbar.sortUpdated")}
            </MenuItem>
          </Popover>

          <ToolButton label={t("toolbar.import")} title={t("toolbar.comingSoon")} disabled>
            <Upload className="h-4 w-4" aria-hidden="true" />
          </ToolButton>
          {/* (W25a) Bulk export selection → one .bxprofile file per profile */}
          <ToolButton
            label={t("toolbar.export")}
            disabled={none}
            onClick={props.onExportSelected}
          >
            <Download className="h-4 w-4" aria-hidden="true" />
          </ToolButton>

          <ToolButton
            label={t("toolbar.clone")}
            disabled={notSingle}
            onClick={props.onCloneSelected}
          >
            <Copy className="h-4 w-4" aria-hidden="true" />
          </ToolButton>
          <ToolButton
            label={t("cookies.bulkExport")}
            disabled={none}
            onClick={props.onExportCookiesSelected}
          >
            <Cookie className="h-4 w-4" aria-hidden="true" />
          </ToolButton>
          <ToolButton
            label={t("toolbar.clearCacheSelected")}
            title={
              hasRunningSelected
                ? t("table.clearCacheRunning")
                : t("toolbar.clearCacheSelected")
            }
            disabled={none || hasRunningSelected}
            onClick={props.onClearCacheSelected}
          >
            <Eraser className="h-4 w-4" aria-hidden="true" />
          </ToolButton>
          <ToolButton
            label={t("toolbar.trash")}
            disabled={none}
            onClick={props.onTrashSelected}
          >
            <Trash2 className="h-4 w-4" aria-hidden="true" />
          </ToolButton>
        </>
      )}

      {/* Right: kebab + search */}
      <div className="ml-auto flex items-center gap-3">
        <Popover
          open={menu === "more"}
          onClose={close}
          label={t("toolbar.more")}
          align="end"
          trigger={
            <ToolButton
              label={t("toolbar.more")}
              expanded={menu === "more"}
              onClick={() => toggle("more")}
            >
              <EllipsisVertical className="h-4 w-4" aria-hidden="true" />
            </ToolButton>
          }
        >
          {/* Import .bxprofile (W19a) — triggers the hidden file input */}
          <MenuItem
            icon={<Upload className="h-4 w-4 text-fg-muted" aria-hidden="true" />}
            onClick={() => {
              close();
              fileRef.current?.click();
            }}
          >
            {t("exchange.importButton")}
          </MenuItem>
          <MenuItem
            disabled={none}
            onClick={() => {
              close();
              props.onClearSelection();
            }}
          >
            {t("toolbar.clearSelection")}
          </MenuItem>
        </Popover>

        <div className="relative w-[225px]">
          <Search
            className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-muted"
            aria-hidden="true"
          />
          <input
            type="search"
            value={props.search}
            onChange={(e) => props.onSearchChange(e.target.value)}
            placeholder={t("toolbar.searchPlaceholder")}
            aria-label={t("toolbar.searchPlaceholder")}
            className="h-9 w-full rounded-md border border-border bg-surface-2 pl-9 pr-9 text-sm text-fg placeholder:text-fg-muted focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/50"
          />
          <button
            type="button"
            disabled
            aria-label={t("toolbar.filters")}
            title={t("toolbar.comingSoon")}
            className="absolute right-2 top-1/2 grid h-6 w-6 -translate-y-1/2 place-items-center rounded-full text-fg-muted disabled:cursor-not-allowed disabled:opacity-50"
          >
            <SlidersHorizontal className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
        </div>
      </div>

      <input
        ref={fileRef}
        type="file"
        accept=".bxprofile,.json,application/json"
        className="hidden"
        tabIndex={-1}
        aria-label={t("exchange.importButton")}
        onChange={(e) => {
          const file = e.currentTarget.files?.[0];
          e.currentTarget.value = "";
          if (file) props.onImport(file);
        }}
      />
    </div>
  );
}

function sortDot(active: boolean) {
  return (
    <span
      className={`h-1.5 w-1.5 rounded-full ${active ? "bg-accent" : "bg-transparent"}`}
      aria-hidden="true"
    />
  );
}
