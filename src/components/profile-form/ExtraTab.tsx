import { useTranslation } from "react-i18next";
import { Toggle } from "./controls";
import type { FormState, SetField } from "./types";

interface ExtraTabProps {
  form: FormState;
  set: SetField;
  argsText: string;
  onArgsChange: (text: string) => void;
  argsError: string | null;
}

export function ExtraTab({ form, set, argsText, onArgsChange, argsError }: ExtraTabProps) {
  const { t } = useTranslation();

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
              { key: "store_history", id: "pf-store-history", label: t("pstorage.history") },
              { key: "store_passwords", id: "pf-store-passwords", label: t("pstorage.passwords") },
              { key: "store_sw_cache", id: "pf-store-sw-cache", label: t("pstorage.swCache") },
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

      <div>
        <label className="label" htmlFor="pf-args">{t("pform.launchArgs")}</label>
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
          <p id="pf-args-error" role="alert" className="mt-1 text-xs text-danger">
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
