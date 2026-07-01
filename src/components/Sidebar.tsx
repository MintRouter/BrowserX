import { Folder, Globe, Play, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

export type MainView = "profiles" | "running" | "trash" | "proxies";

interface SidebarProps {
  view: MainView;
  runningCount: number;
  onNavigate: (view: MainView) => void;
}

export function Sidebar({ view, runningCount, onNavigate }: SidebarProps) {
  const { t } = useTranslation();

  const items: { key: MainView; label: string; icon: React.ReactNode; badge?: number }[] = [
    { key: "profiles", label: t("sidebar.allProfiles"), icon: <Folder className="h-4 w-4" aria-hidden="true" /> },
    { key: "running", label: t("sidebar.running"), icon: <Play className="h-4 w-4" aria-hidden="true" />, badge: runningCount },
    { key: "proxies", label: t("sidebar.proxies"), icon: <Globe className="h-4 w-4" aria-hidden="true" /> },
    { key: "trash", label: t("sidebar.trash"), icon: <Trash2 className="h-4 w-4" aria-hidden="true" /> },
  ];

  return (
    <nav
      className="w-52 shrink-0 border-r border-border bg-surface-1 p-2 space-y-0.5"
      aria-label={t("sidebar.folders")}
    >
      {items.map((item) => (
        <button
          key={item.key}
          onClick={() => onNavigate(item.key)}
          aria-current={view === item.key ? "page" : undefined}
          className={`w-full flex items-center gap-2 px-3 py-2 rounded-md text-sm text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 ${
            view === item.key
              ? "bg-surface-3 font-medium"
              : "text-fg-muted hover:bg-surface-2 hover:text-fg"
          }`}
        >
          {item.icon}
          <span className="flex-1 truncate">{item.label}</span>
          {item.badge !== undefined && item.badge > 0 && (
            <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-accent/15 text-accent font-medium">
              {item.badge}
            </span>
          )}
        </button>
      ))}
    </nav>
  );
}
