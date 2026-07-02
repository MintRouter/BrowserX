import { Check, ChevronDown, Globe } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { SUPPORTED_LOCALES, setLocale } from "../i18n";
import { MenuItem, Popover } from "./Popover";

/** Top bar language dropdown (🌐 EN | Tiếng Việt) — persists via setLocale. */
export function LanguageSwitcher() {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const current = i18n.language;

  return (
    <Popover
      open={open}
      onClose={() => setOpen(false)}
      align="end"
      label={t("topbar.changeLanguage")}
      trigger={
        <button
          type="button"
          aria-label={t("topbar.changeLanguage")}
          title={t("topbar.changeLanguage")}
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
          className="inline-flex h-9 items-center gap-1 rounded-md px-2 text-fg/80 transition-colors hover:bg-surface-3 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
        >
          <Globe className="h-[18px] w-[18px]" aria-hidden="true" />
          <span className="text-xs font-semibold uppercase">{current}</span>
          <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
        </button>
      }
    >
      <div role="menu" className="w-44">
        {SUPPORTED_LOCALES.map((l) => (
          <MenuItem
            key={l.code}
            icon={
              <Check
                className={`h-4 w-4 shrink-0 ${
                  current === l.code ? "text-accent" : "invisible"
                }`}
                aria-hidden="true"
              />
            }
            onClick={() => {
              setLocale(l.code);
              setOpen(false);
            }}
          >
            {l.label}
          </MenuItem>
        ))}
      </div>
    </Popover>
  );
}
