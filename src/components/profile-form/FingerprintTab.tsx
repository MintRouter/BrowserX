import { Dice5 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { GeolocationMode, Platform, WebrtcMode } from "../../lib/api";
import { Segmented, Toggle } from "./controls";
import {
  HARDWARE_CONCURRENCY_OPTIONS,
  RESOLUTION_PRESETS,
  type FormState,
  type SetField,
} from "./types";

interface FingerprintTabProps {
  form: FormState;
  set: SetField;
}

export function FingerprintTab({ form, set }: FingerprintTabProps) {
  const { t } = useTranslation();

  const presetLabel =
    RESOLUTION_PRESETS.find(
      (p) => p.width === form.screen_width && p.height === form.screen_height,
    )?.label ?? "custom";

  const hcOptions: Array<number> = [...HARDWARE_CONCURRENCY_OPTIONS];
  if (
    form.hardware_concurrency !== null &&
    !hcOptions.includes(form.hardware_concurrency)
  ) {
    hcOptions.push(form.hardware_concurrency);
    hcOptions.sort((a, b) => a - b);
  }

  return (
    <div className="space-y-5">
      {/* Seed */}
      <div>
        <label className="label" htmlFor="pf-seed">{t("pform.fingerprintSeed")}</label>
        <div className="flex gap-2">
          <input
            id="pf-seed"
            className="input flex-1"
            value={form.fingerprint_seed ?? ""}
            onChange={(e) => set("fingerprint_seed", e.target.value || null)}
            placeholder={t("pform.seedPlaceholder")}
          />
          <button
            type="button"
            onClick={() =>
              set("fingerprint_seed", String(Math.floor(Math.random() * 90000) + 10000))
            }
            className="btn-secondary px-2.5"
            aria-label={t("pform.randomizeSeed")}
            title={t("pform.randomizeSeed")}
          >
            <Dice5 className="h-4 w-4" aria-hidden="true" />
          </button>
        </div>
      </div>

      {/* OS */}
      <div>
        <span className="label">{t("pform.operatingSystem")}</span>
        <Segmented<Platform>
          options={[
            { value: "windows", label: "Windows" },
            { value: "macos", label: "macOS" },
            { value: "linux", label: "Linux" },
          ]}
          value={form.platform}
          onChange={(v) => set("platform", v)}
          label={t("pform.operatingSystem")}
        />
      </div>

      {/* Screen resolution */}
      <div>
        <label className="label" htmlFor="pf-res">{t("pform.screenResolution")}</label>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          <select
            id="pf-res"
            className="input"
            value={presetLabel}
            onChange={(e) => {
              const preset = RESOLUTION_PRESETS.find((p) => p.label === e.target.value);
              if (preset) {
                set("screen_width", preset.width);
                set("screen_height", preset.height);
              }
            }}
          >
            {RESOLUTION_PRESETS.map((p) => (
              <option key={p.label} value={p.label}>{p.label}</option>
            ))}
            <option value="custom">{t("pform.custom")}</option>
          </select>
          <div className="flex items-center gap-2">
            <input
              className="input no-spin"
              type="number"
              min={320}
              value={form.screen_width}
              onChange={(e) => set("screen_width", Number(e.target.value) || 0)}
              aria-label={t("pform.width")}
            />
            <span className="text-fg-muted" aria-hidden="true">×</span>
            <input
              className="input no-spin"
              type="number"
              min={320}
              value={form.screen_height}
              onChange={(e) => set("screen_height", Number(e.target.value) || 0)}
              aria-label={t("pform.height")}
            />
          </div>
        </div>
      </div>

      {/* Timezone + Locale */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div>
          <label className="label" htmlFor="pf-tz">{t("pform.timezone")}</label>
          <input
            id="pf-tz"
            className="input"
            value={form.timezone ?? ""}
            onChange={(e) => set("timezone", e.target.value || null)}
            placeholder={t("pform.timezonePlaceholder")}
          />
        </div>
        <div>
          <label className="label" htmlFor="pf-locale">{t("pform.locale")}</label>
          <input
            id="pf-locale"
            className="input"
            value={form.locale ?? ""}
            onChange={(e) => set("locale", e.target.value || null)}
            placeholder={t("pform.localePlaceholder")}
          />
        </div>
      </div>

      {/* GPU */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div>
          <label className="label" htmlFor="pf-gpu-v">{t("pform.gpuVendor")}</label>
          <input
            id="pf-gpu-v"
            className="input"
            value={form.gpu_vendor ?? ""}
            onChange={(e) => set("gpu_vendor", e.target.value || null)}
            placeholder={t("pform.autoFromSeed")}
          />
        </div>
        <div>
          <label className="label" htmlFor="pf-gpu-r">{t("pform.gpuRenderer")}</label>
          <input
            id="pf-gpu-r"
            className="input"
            value={form.gpu_renderer ?? ""}
            onChange={(e) => set("gpu_renderer", e.target.value || null)}
            placeholder={t("pform.autoFromSeed")}
          />
        </div>
      </div>

      {/* Hardware concurrency + Color scheme */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div>
          <label className="label" htmlFor="pf-hc">{t("pform.hardwareConcurrency")}</label>
          <select
            id="pf-hc"
            className="input"
            value={form.hardware_concurrency ?? ""}
            onChange={(e) =>
              set("hardware_concurrency", e.target.value ? Number(e.target.value) : null)
            }
          >
            <option value="">{t("pform.autoFromSeed")}</option>
            {hcOptions.map((n) => (
              <option key={n} value={n}>{n}</option>
            ))}
          </select>
        </div>
        <div>
          <label className="label" htmlFor="pf-cs">{t("pform.colorScheme")}</label>
          <select
            id="pf-cs"
            className="input"
            value={form.color_scheme ?? ""}
            onChange={(e) => set("color_scheme", e.target.value || null)}
          >
            <option value="">{t("pform.system")}</option>
            <option value="light">{t("pform.light")}</option>
            <option value="dark">{t("pform.dark")}</option>
          </select>
        </div>
      </div>

      {/* Humanize */}
      <div className="flex items-center justify-between gap-3">
        <label htmlFor="pf-humanize" className="text-sm text-fg cursor-pointer">
          {t("pform.humanize")}
        </label>
        <Toggle
          id="pf-humanize"
          checked={form.humanize}
          onChange={(v) => set("humanize", v)}
          label={t("pform.humanize")}
        />
      </div>
      {form.humanize && (
        <div>
          <label className="label" htmlFor="pf-human-preset">{t("pform.humanPreset")}</label>
          <select
            id="pf-human-preset"
            className="input"
            value={form.human_preset}
            onChange={(e) => set("human_preset", e.target.value)}
          >
            <option value="default">{t("pform.presetDefault")}</option>
            <option value="careful">{t("pform.presetCareful")}</option>
          </select>
        </div>
      )}

      {/* Noise injection (W19c): single master switch mapping to --fingerprint-noise=false. */}
      <div className="flex items-center justify-between gap-3">
        <label htmlFor="pf-fp-noise" className="text-sm text-fg cursor-pointer">
          {t("pform.fpNoise")}
        </label>
        <Toggle
          id="pf-fp-noise"
          checked={form.fp_noise}
          onChange={(v) => set("fp_noise", v)}
          label={t("pform.fpNoise")}
        />
      </div>

      {/* WebRTC (W19c) */}
      <div>
        <span className="label">{t("pform.webrtc")}</span>
        <Segmented<WebrtcMode>
          options={[
            { value: "real", label: t("pform.webrtcReal") },
            { value: "masked", label: t("pform.webrtcMasked") },
          ]}
          value={form.webrtc_mode}
          onChange={(v) => set("webrtc_mode", v)}
          label={t("pform.webrtc")}
        />
        {form.webrtc_mode === "masked" && (
          <div className="mt-2">
            <label className="label" htmlFor="pf-webrtc-ip">{t("pform.webrtcIp")}</label>
            <input
              id="pf-webrtc-ip"
              className="input"
              value={form.webrtc_ip ?? ""}
              onChange={(e) => set("webrtc_ip", e.target.value || null)}
              placeholder={t("pform.webrtcIpPlaceholder")}
            />
          </div>
        )}
      </div>

      {/* Geolocation (W19c) */}
      <div>
        <span className="label">{t("pform.geolocation")}</span>
        <Segmented<GeolocationMode>
          options={[
            { value: "auto", label: t("pform.geoAuto") },
            { value: "manual", label: t("pform.geoManual") },
          ]}
          value={form.geolocation_mode}
          onChange={(v) => set("geolocation_mode", v)}
          label={t("pform.geolocation")}
        />
        {form.geolocation_mode === "manual" && (
          <div className="mt-2 grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div>
              <label className="label" htmlFor="pf-geo-lat">{t("pform.geoLatitude")}</label>
              <input
                id="pf-geo-lat"
                className="input"
                value={form.geo_latitude ?? ""}
                onChange={(e) => set("geo_latitude", e.target.value || null)}
                placeholder="52.5"
              />
            </div>
            <div>
              <label className="label" htmlFor="pf-geo-lon">{t("pform.geoLongitude")}</label>
              <input
                id="pf-geo-lon"
                className="input"
                value={form.geo_longitude ?? ""}
                onChange={(e) => set("geo_longitude", e.target.value || null)}
                placeholder="13.4"
              />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
