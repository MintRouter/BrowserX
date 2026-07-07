import {
  ChevronDown,
  Globe,
  HelpCircle,
  Laptop,
  LifeBuoy,
  type LucideIcon,
  MonitorSmartphone,
  Network,
  Puzzle,
  Settings,
  Users,
} from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { DOCS_URL, openExternal } from "../lib/api";
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

/* (W50I) MLX pixel audit §2: icon-pill 44×28, radius 8, icon 20px;
 * active = #F0F6FF bg + accent icon, idle = gray icon + light hover. */
const pillBtn =
  "inline-flex h-7 w-11 shrink-0 items-center justify-center rounded-lg transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60";
const pillIdle = "text-fg/70 hover:bg-surface-3 hover:text-fg";
const pillActive = "bg-[#F0F6FF] text-accent";

/** Icon-pill row — one pill per top-level view (MLX app-switcher parity). */
const PILLS: {
  view: MainView;
  icon: LucideIcon;
  labelKey: string;
  /** Sidebar sub-views that keep this pill highlighted. */
  match: MainView[];
}[] = [
  {
    view: "profiles",
    icon: MonitorSmartphone,
    labelKey: "topbar.profiles",
    match: ["profiles", "running", "cloudSync", "favorites", "trash"],
  },
  { view: "proxies", icon: Globe, labelKey: "topbar.proxies", match: ["proxies"] },
  { view: "proxyTemplates", icon: Network, labelKey: "topbar.proxyTemplates", match: ["proxyTemplates"] },
  { view: "templates", icon: Laptop, labelKey: "topbar.profileTemplates", match: ["templates"] },
  { view: "extensions", icon: Puzzle, labelKey: "topbar.extensions", match: ["extensions"] },
  { view: "settings", icon: Settings, labelKey: "topbar.settings", match: ["settings"] },
];

export function TopBar(props: TopBarProps) {
  const { view, onNavigate } = props;
  const { t } = useTranslation();
  const [moreOpen, setMoreOpen] = useState(false);
  const [accountOpen, setAccountOpen] = useState(false);
  const initial = t("app.name").charAt(0).toUpperCase();

  const menuIcon = "h-4 w-4 shrink-0 text-fg-muted";

  return (
    <header className="flex h-14 shrink-0 items-center gap-2 bg-surface-0 px-2">
      {/* (W50I-fix) One white island holding logo + divider + pill row + chevron (MLX audit §2). */}
      <div className="flex h-10 items-center rounded-lg bg-surface-1 px-1.5">
        {/* Logo = home (Profiles), like the ML logo click. */}
        <button
          type="button"
          onClick={() => onNavigate?.("profiles")}
          aria-label={t("topbar.profiles")}
          title={t("topbar.profiles")}
          className="flex h-8 items-center gap-2.5 rounded-md px-1.5 transition-colors hover:bg-surface-3 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
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

        {/* Thin vertical divider between logo and pills. */}
        <span className="mx-2 h-5 w-px shrink-0 bg-border" aria-hidden="true" />

        {/* (W50I) Direct icon-pill row (MLX audit §2) — one pill per main view. */}
        <nav className="flex items-center gap-0.5" aria-label={t("topbar.appSwitcher")}>
          {PILLS.map(({ view: v, icon: Icon, labelKey, match }) => {
            const active = view !== undefined && match.includes(view);
            return (
              <button
                key={v}
                type="button"
                onClick={() => onNavigate?.(v)}
                aria-label={t(labelKey)}
                title={t(labelKey)}
                aria-current={active ? "page" : undefined}
                className={`${pillBtn} ${active ? pillActive : pillIdle}`}
              >
                <Icon className="h-5 w-5" aria-hidden="true" />
              </button>
            );
          })}

          {/* Chevron dropdown — only secondary items without a dedicated view. */}
          <Popover
            open={moreOpen}
            onClose={() => setMoreOpen(false)}
            label={t("topbar.appSwitcher")}
            trigger={
              <button
                type="button"
                aria-label={t("topbar.appSwitcher")}
                title={t("topbar.appSwitcher")}
                aria-haspopup="menu"
                aria-expanded={moreOpen}
                onClick={() => setMoreOpen((v) => !v)}
                className={`${pillBtn} !w-7 ${moreOpen ? pillActive : pillIdle}`}
              >
                <ChevronDown className="h-4 w-4" aria-hidden="true" />
              </button>
            }
            panelClassName="w-[252px]"
          >
            <div role="menu">
              <MenuItem
                icon={<Users className={menuIcon} aria-hidden="true" />}
                disabled
                title={t("toolbar.comingSoon")}
              >
                {t("topbar.team")}
              </MenuItem>
              {/* (W50F) Docs link — opens externally via the opener plugin. */}
              <MenuItem
                icon={<HelpCircle className={menuIcon} aria-hidden="true" />}
                onClick={() => {
                  setMoreOpen(false);
                  openExternal(DOCS_URL);
                }}
              >
                {t("topbar.help")}
              </MenuItem>
            </div>
          </Popover>
        </nav>
      </div>

      {/* (W50I) Right cluster — separate white islands (MLX audit §2). */}
      <div className="ml-auto flex items-center gap-2">
        <LanguageSwitcher />
        {/* Support island 40×40 radius 6 — opens the docs/support page (W50F). */}
        <button
          type="button"
          onClick={() => openExternal(DOCS_URL)}
          aria-label={t("topbar.support")}
          title={t("topbar.support")}
          className="inline-flex h-10 w-10 items-center justify-center rounded-md bg-surface-1 text-fg/80 transition-colors hover:bg-surface-2 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
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
              className="flex h-10 min-w-[76px] items-center justify-center gap-1 rounded-md bg-surface-1 px-2 transition-colors hover:bg-surface-2 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
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
