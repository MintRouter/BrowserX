import {
  ChevronDown,
  HelpCircle,
  Laptop,
  LifeBuoy,
  MonitorSmartphone,
  Network,
  Puzzle,
  Settings,
  Users,
} from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { LanguageSwitcher } from "./LanguageSwitcher";
import { MenuItem, Popover } from "./Popover";
import type { MainView } from "./Sidebar";

interface TopBarProps {
  /** Kept for App.tsx compatibility — the search box moves to the table toolbar (W13). */
  search: string;
  onSearchChange: (value: string) => void;
  onNewProfile: () => void;
  view?: MainView;
  onNavigate?: (view: MainView) => void;
}

const iconBtn =
  "inline-flex h-9 w-9 items-center justify-center rounded-md transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60";
const iconIdle = "text-fg/80 hover:bg-surface-3 hover:text-fg";
const iconActive = "bg-accent/10 text-accent";

export function TopBar(props: TopBarProps) {
  const { view, onNavigate } = props;
  const { t } = useTranslation();
  const [appsOpen, setAppsOpen] = useState(false);
  const [accountOpen, setAccountOpen] = useState(false);
  const initial = t("app.name").charAt(0).toUpperCase();
  /** Views reachable from the app-switcher dropdown (ML parity, GAP 1/2). */
  const appsActive =
    view === "proxyTemplates" ||
    view === "templates" ||
    view === "extensions" ||
    view === "settings";

  const go = (v: MainView) => {
    setAppsOpen(false);
    onNavigate?.(v);
  };

  const menuIcon = "h-4 w-4 shrink-0 text-fg-muted";

  return (
    <header className="flex h-14 shrink-0 items-center gap-2 bg-surface-0 px-2">
      {/* Logo card = home (Profiles), like the ML logo click. */}
      <button
        type="button"
        onClick={() => onNavigate?.("profiles")}
        aria-label={t("topbar.profiles")}
        title={t("topbar.profiles")}
        aria-current={!appsActive ? "page" : undefined}
        className="flex h-10 items-center gap-2.5 rounded-lg bg-surface-1 px-2.5 transition-colors hover:bg-surface-2 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
      >
        <span
          className="grid h-7 w-7 shrink-0 place-items-center rounded-md bg-accent text-xs font-bold text-white"
          aria-hidden="true"
        >
          {initial}
        </span>
        <span className="text-sm font-bold uppercase tracking-wider text-fg">
          {t("app.name")}
        </span>
      </button>

      {/* App-switcher dropdown (devices icon + chevron) replaces the flat icon row. */}
      <Popover
        open={appsOpen}
        onClose={() => setAppsOpen(false)}
        label={t("topbar.appSwitcher")}
        trigger={
          <button
            type="button"
            aria-label={t("topbar.appSwitcher")}
            title={t("topbar.appSwitcher")}
            aria-haspopup="menu"
            aria-expanded={appsOpen}
            onClick={() => setAppsOpen((v) => !v)}
            className={`flex h-10 items-center gap-1 rounded-lg px-2 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 ${
              appsActive ? iconActive : "bg-surface-1 text-fg/80 hover:bg-surface-2 hover:text-fg"
            }`}
          >
            <MonitorSmartphone className="h-5 w-5" strokeWidth={2} aria-hidden="true" />
            <ChevronDown className="h-4 w-4" aria-hidden="true" />
          </button>
        }
      >
        <div role="menu" className="w-52">
          <MenuItem
            icon={<Network className={menuIcon} aria-hidden="true" />}
            onClick={() => go("proxyTemplates")}
          >
            {t("topbar.proxyTemplates")}
          </MenuItem>
          <MenuItem
            icon={<Laptop className={menuIcon} aria-hidden="true" />}
            onClick={() => go("templates")}
          >
            {t("topbar.profileTemplates")}
          </MenuItem>
          <MenuItem
            icon={<Puzzle className={menuIcon} aria-hidden="true" />}
            onClick={() => go("extensions")}
          >
            {t("topbar.extensions")}
          </MenuItem>
          <MenuItem
            icon={<Users className={menuIcon} aria-hidden="true" />}
            disabled
            title={t("toolbar.comingSoon")}
          >
            {t("topbar.team")}
          </MenuItem>
          {/* Docs link pending an external-open capability (no opener plugin yet). */}
          <MenuItem
            icon={<HelpCircle className={menuIcon} aria-hidden="true" />}
            disabled
            title={t("topbar.linkUnavailable")}
          >
            {t("topbar.help")}
          </MenuItem>
          <MenuItem
            icon={<Settings className={menuIcon} aria-hidden="true" />}
            onClick={() => go("settings")}
          >
            {t("topbar.accountSettings")}
          </MenuItem>
        </div>
      </Popover>

      <div className="ml-auto flex h-11 items-center gap-2 rounded-lg bg-surface-1 px-2">
        <LanguageSwitcher />
        {/* Support slot (ML parity) — disabled until external links can open. */}
        <button
          type="button"
          disabled
          aria-label={t("topbar.support")}
          title={t("topbar.linkUnavailable")}
          className={`${iconBtn} ${iconIdle} disabled:cursor-not-allowed disabled:opacity-40`}
        >
          <LifeBuoy className="h-[18px] w-[18px]" aria-hidden="true" />
        </button>
        <Popover
          open={accountOpen}
          onClose={() => setAccountOpen(false)}
          align="end"
          label={t("topbar.accountMenu")}
          trigger={
            <button
              type="button"
              aria-label={t("topbar.account")}
              title={t("topbar.account")}
              aria-haspopup="menu"
              aria-expanded={accountOpen}
              onClick={() => setAccountOpen((v) => !v)}
              className="flex items-center gap-1 rounded-md px-1 py-0.5 transition-colors hover:bg-surface-3 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
            >
              <span
                className="grid h-8 w-8 place-items-center rounded-full bg-accent text-xs font-semibold text-white"
                aria-hidden="true"
              >
                {initial}
              </span>
              <ChevronDown className="h-4 w-4 text-fg/80" aria-hidden="true" />
            </button>
          }
        >
          <div role="menu" className="w-44">
            <MenuItem
              icon={<Settings className="h-4 w-4 shrink-0 text-fg-muted" aria-hidden="true" />}
              onClick={() => {
                setAccountOpen(false);
                onNavigate?.("settings");
              }}
            >
              {t("topbar.settings")}
            </MenuItem>
          </div>
        </Popover>
      </div>
    </header>
  );
}
