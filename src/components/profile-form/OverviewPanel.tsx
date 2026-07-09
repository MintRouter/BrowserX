import { useTranslation } from "react-i18next";
import type { Proxy } from "../../lib/api";
import type { FormState } from "./types";

interface OverviewPanelProps {
  form: FormState;
  proxies: Proxy[];
}

const OS_LABELS: Record<string, string> = {
  windows: "Windows",
  macos: "macOS",
  linux: "Linux",
};

/** Live "Profile overview" summary card (Multilogin-style, pale rose). */
export function OverviewPanel({ form, proxies }: OverviewPanelProps) {
  const { t } = useTranslation();
  const masked = t("pform.ov.masked");
  const proxyName = proxies.find((p) => p.id === form.proxy_id)?.name;

  const rows: Array<[string, string]> = [
    [t("pform.ov.name"), form.name.trim() || t("pform.ov.defaultName")],
    [t("pform.ov.proxy"), proxyName ?? t("pform.ov.none")],
    [t("pform.ov.os"), OS_LABELS[form.platform] ?? form.platform],
    [t("pform.ov.browser"), "Chromium"],
    [t("pform.ov.storage"), t("pform.ov.local")],
    [t("pform.ov.webrtc"), masked],
    [t("pform.ov.timezone"), form.timezone || masked],
    [t("pform.ov.geoAccess"), t("pform.ov.prompt")],
    [t("pform.ov.geoData"), masked],
    [t("pform.ov.languages"), form.locale || masked],
    [t("pform.ov.resolution"), `${form.screen_width} × ${form.screen_height}`],
    [t("pform.ov.fontData"), masked],
    [t("pform.ov.media"), t("pform.ov.real")],
    [t("pform.ov.userAgent"), masked],
    [t("pform.ov.platform"), masked],
    [
      t("pform.ov.hardwareConcurrency"),
      form.hardware_concurrency !== null
        ? String(form.hardware_concurrency)
        : masked,
    ],
  ];

  return (
    <section
      aria-labelledby="pf-overview-title"
      className="rounded-lg bg-[#fbf7f7] px-4 pb-4 pt-3.5 dark:bg-surface-2"
    >
      <h2
        id="pf-overview-title"
        className="mb-3 text-base font-medium text-[#1D192B] dark:text-fg"
      >
        {t("pform.ov.title")}
      </h2>
      <dl>
        {rows.map(([label, value]) => (
          <div
            key={label}
            className="flex items-baseline justify-between gap-4 py-1.5"
          >
            <dt className="shrink-0 text-xs text-[#1D192B] dark:text-fg">
              {label}
            </dt>
            <dd
              className="truncate text-right text-xs text-[#1D192B] dark:text-fg"
              title={value}
            >
              {value}
            </dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
