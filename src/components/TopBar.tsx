import {
  Bot,
  ChevronDown,
  HelpCircle,
  Laptop,
  LayoutGrid,
  Network,
  Puzzle,
  Send,
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
  const [accountOpen, setAccountOpen] = useState(false);
  const initial = t("app.name").charAt(0).toUpperCase();
  const profilesActive =
    view !== "proxies" &&
    view !== "proxyTemplates" &&
    view !== "templates" &&
    view !== "extensions" &&
    view !== "settings";

  const navItems: {
    key: string;
    label: string;
    icon: React.ReactNode;
    active?: boolean;
    onClick?: () => void;
  }[] = [
    {
      key: "profiles",
      label: t("topbar.profiles"),
      icon: <LayoutGrid className="h-5 w-5" strokeWidth={2} aria-hidden="true" />,
      active: profilesActive,
      onClick: () => onNavigate?.("profiles"),
    },
    {
      key: "proxies",
      label: t("topbar.proxies"),
      icon: <Network className="h-5 w-5" strokeWidth={2} aria-hidden="true" />,
      active: view === "proxies" || view === "proxyTemplates",
      onClick: () => onNavigate?.("proxies"),
    },
    {
      key: "templates",
      label: t("topbar.templates"),
      icon: <Laptop className="h-5 w-5" strokeWidth={2} aria-hidden="true" />,
      active: view === "templates",
      onClick: () => onNavigate?.("templates"),
    },
    {
      key: "automation",
      label: t("topbar.automation"),
      icon: <Bot className="h-5 w-5" strokeWidth={2} aria-hidden="true" />,
    },
    {
      key: "extensions",
      label: t("topbar.extensions"),
      icon: <Puzzle className="h-5 w-5" strokeWidth={2} aria-hidden="true" />,
      active: view === "extensions",
      onClick: () => onNavigate?.("extensions"),
    },
    {
      key: "team",
      label: t("topbar.team"),
      icon: <Users className="h-5 w-5" strokeWidth={2} aria-hidden="true" />,
    },
    {
      key: "help",
      label: t("topbar.help"),
      icon: <HelpCircle className="h-5 w-5" strokeWidth={2} aria-hidden="true" />,
    },
    {
      key: "more",
      label: t("topbar.more"),
      icon: <ChevronDown className="h-4 w-4" aria-hidden="true" />,
    },
  ];

  return (
    <header className="flex h-14 shrink-0 items-center gap-2 bg-surface-0 px-2">
      {/* Workspace/logo card — separate from the nav cluster (ML parity, 1.11) */}
      <div className="flex h-10 items-center gap-2.5 rounded-lg bg-surface-1 px-2.5">
        <span
          className="grid h-7 w-7 shrink-0 place-items-center rounded-md bg-accent text-xs font-bold text-white"
          aria-hidden="true"
        >
          {initial}
        </span>
        <span className="text-sm font-bold uppercase tracking-wider text-fg">
          {t("app.name")}
        </span>
        <ChevronDown className="h-4 w-4 text-fg/80" aria-hidden="true" />
      </div>

      <div className="flex h-10 items-center rounded-lg bg-surface-1 px-1.5">
        <nav className="flex items-center gap-1" aria-label={t("topbar.mainNav")}>
          {navItems.map((item) => (
            <button
              key={item.key}
              type="button"
              onClick={item.onClick}
              aria-label={item.label}
              title={item.label}
              aria-current={item.active ? "page" : undefined}
              className={`${iconBtn} ${item.active ? iconActive : iconIdle}`}
            >
              {item.icon}
            </button>
          ))}
        </nav>
      </div>

      <div className="ml-auto flex h-11 items-center gap-2 rounded-lg bg-surface-1 px-2">
        <LanguageSwitcher />
        <button
          type="button"
          aria-label={t("topbar.share")}
          title={t("topbar.share")}
          className={`${iconBtn} ${iconIdle}`}
        >
          <Send className="h-[18px] w-[18px]" aria-hidden="true" />
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
