import { Dices, Save, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Platform, Profile, ProfileInput, Proxy } from "../lib/api";
import { detectHostPlatform } from "../lib/host";

interface ProfileFormProps {
  profile: Profile | null; // null = create mode
  proxies: Proxy[];
  onSave: (data: ProfileInput) => Promise<void>;
  onDelete?: () => Promise<void>;
  onCancel: () => void;
}

const RESOLUTION_PRESETS: Record<string, { width: number; height: number }> = {
  "1920 × 1080 (Full HD)": { width: 1920, height: 1080 },
  "2560 × 1440 (QHD)": { width: 2560, height: 1440 },
  "1366 × 768 (HD)": { width: 1366, height: 768 },
  "1440 × 900": { width: 1440, height: 900 },
  "1536 × 864": { width: 1536, height: 864 },
  "1280 × 720 (720p)": { width: 1280, height: 720 },
};

const GPU_PRESETS: Record<string, { vendor: string; renderer: string }> = {
  "NVIDIA RTX 3070": {
    vendor: "Google Inc. (NVIDIA)",
    renderer: "ANGLE (NVIDIA, NVIDIA GeForce RTX 3070 (0x00002484) Direct3D11 vs_5_0 ps_5_0, D3D11)",
  },
  "NVIDIA RTX 4070": {
    vendor: "Google Inc. (NVIDIA)",
    renderer: "ANGLE (NVIDIA, NVIDIA GeForce RTX 4070 (0x00002786) Direct3D11 vs_5_0 ps_5_0, D3D11)",
  },
  "AMD RX 6800 XT": {
    vendor: "Google Inc. (AMD)",
    renderer: "ANGLE (AMD, AMD Radeon RX 6800 XT (0x000073BF) Direct3D11 vs_5_0 ps_5_0, D3D11)",
  },
  "Intel UHD 770": {
    vendor: "Google Inc. (Intel)",
    renderer: "ANGLE (Intel, Intel(R) UHD Graphics 770 (0x00004680) Direct3D11 vs_5_0 ps_5_0, D3D11)",
  },
  "Apple M3 (macOS)": {
    vendor: "Google Inc. (Apple)",
    renderer: "ANGLE (Apple, ANGLE Metal Renderer: Apple M3, Unspecified Version)",
  },
};

const HOST_PLATFORM = detectHostPlatform();

export function ProfileForm({ profile, proxies, onSave, onDelete, onCancel }: ProfileFormProps) {
  const { t } = useTranslation();
  const isEdit = profile !== null;

  const [form, setForm] = useState<ProfileInput>({
    name: "",
    platform: HOST_PLATFORM,
    screen_width: 1920,
    screen_height: 1080,
    humanize: false,
    human_preset: "default",
    headless: false,
    geoip: false,
    launch_args: [],
    tags: [],
    proxy_id: null,
  });

  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [tagInput, setTagInput] = useState("");
  const [launchArgInput, setLaunchArgInput] = useState("");

  useEffect(() => {
    if (profile) {
      setForm({
        name: profile.name,
        fingerprint_seed: profile.fingerprint_seed,
        platform: profile.platform,
        timezone: profile.timezone,
        locale: profile.locale,
        screen_width: profile.screen_width,
        screen_height: profile.screen_height,
        gpu_vendor: profile.gpu_vendor,
        gpu_renderer: profile.gpu_renderer,
        hardware_concurrency: profile.hardware_concurrency,
        humanize: profile.humanize,
        human_preset: profile.human_preset ?? "default",
        headless: profile.headless,
        geoip: profile.geoip,
        color_scheme: profile.color_scheme,
        launch_args: profile.launch_args ?? [],
        notes: profile.notes,
        proxy_id: profile.proxy_id,
        tags: profile.tags ?? [],
      });
    }
  }, [profile?.id]);

  const set = <K extends keyof ProfileInput>(key: K, value: ProfileInput[K]) => {
    setForm((prev) => ({ ...prev, [key]: value }));
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!form.name.trim()) return;
    setSaving(true);
    try {
      await onSave(form);
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!onDelete) return;
    if (!confirm(t("form.confirmDelete"))) return;
    setDeleting(true);
    try {
      await onDelete();
    } finally {
      setDeleting(false);
    }
  };

  const applyGpuPreset = (name: string) => {
    const preset = GPU_PRESETS[name];
    if (preset) {
      setForm((prev) => ({ ...prev, gpu_vendor: preset.vendor, gpu_renderer: preset.renderer }));
    }
  };

  const randomizeSeed = () => {
    set("fingerprint_seed", String(Math.floor(Math.random() * 90000) + 10000));
  };

  const currentResolution =
    Object.entries(RESOLUTION_PRESETS).find(
      ([, v]) => v.width === form.screen_width && v.height === form.screen_height,
    )?.[0] ?? "custom";

  const addTag = () => {
    const tag = tagInput.trim();
    if (!tag || form.tags?.includes(tag)) return;
    set("tags", [...(form.tags ?? []), tag]);
    setTagInput("");
  };

  const removeTag = (tag: string) => {
    set("tags", (form.tags ?? []).filter((x) => x !== tag));
  };

  const addLaunchArg = () => {
    const arg = launchArgInput.trim();
    if (!arg || (form.launch_args ?? []).includes(arg)) return;
    set("launch_args", [...(form.launch_args ?? []), arg]);
    setLaunchArgInput("");
  };

  const removeLaunchArg = (idx: number) => {
    set("launch_args", (form.launch_args ?? []).filter((_, i) => i !== idx));
  };

  const platformMismatch = form.platform !== HOST_PLATFORM;

  return (
    <form onSubmit={handleSubmit} className="p-6 max-w-2xl mx-auto">
      <div className="flex items-center justify-between mb-6">
        <div className="flex items-center gap-2">
          <h2 className="text-lg font-semibold">
            {isEdit ? t("form.editProfile") : t("form.newProfile")}
          </h2>
          {isEdit && onDelete && (
            <button type="button" onClick={handleDelete} disabled={deleting} className="btn-danger">
              <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
              <span>{deleting ? t("form.deleting") : t("form.delete")}</span>
            </button>
          )}
        </div>
        <div className="flex items-center gap-2">
          <button type="button" onClick={onCancel} className="btn-secondary">
            {t("form.cancel")}
          </button>
          <button type="submit" disabled={saving} className="btn-primary">
            <Save className="h-3.5 w-3.5" aria-hidden="true" />
            <span>{saving ? t("form.saving") : isEdit ? t("form.save") : t("form.create")}</span>
          </button>
        </div>
      </div>

      <div className="space-y-5">
        {/* Basic */}
        <section>
          <h3 className="text-xs font-semibold text-fg-muted uppercase tracking-wider mb-3">{t("form.basic")}</h3>
          <div className="grid grid-cols-2 gap-3">
            <div className="col-span-2">
              <label className="label" htmlFor="pf-name">{t("form.profileName")}</label>
              <input
                id="pf-name"
                className="input"
                value={form.name}
                onChange={(e) => set("name", e.target.value)}
                placeholder={t("form.namePlaceholder")}
                required
              />
            </div>
            <div>
              <label className="label" htmlFor="pf-platform">{t("form.platform")}</label>
              <select
                id="pf-platform"
                className="input"
                value={form.platform}
                onChange={(e) => set("platform", e.target.value as Platform)}
              >
                <option value="windows">Windows</option>
                <option value="macos">macOS</option>
                <option value="linux">Linux</option>
              </select>
            </div>
            <div>
              <label className="label" htmlFor="pf-seed">{t("form.fingerprintSeed")}</label>
              <div className="flex gap-2">
                <input
                  id="pf-seed"
                  className="input flex-1"
                  value={form.fingerprint_seed ?? ""}
                  onChange={(e) => set("fingerprint_seed", e.target.value || null)}
                  placeholder={t("form.seedPlaceholder")}
                />
                <button
                  type="button"
                  onClick={randomizeSeed}
                  className="btn-secondary px-2.5"
                  aria-label={t("form.randomizeSeed")}
                  title={t("form.randomizeSeed")}
                >
                  <Dices className="h-4 w-4" aria-hidden="true" />
                </button>
              </div>
            </div>
            {platformMismatch && (
              <p
                className="col-span-2 text-xs text-warning bg-warning/10 border border-warning/30 rounded-md px-3 py-2"
                role="alert"
              >
                {t("form.platformMismatch", { target: form.platform, host: HOST_PLATFORM })}
              </p>
            )}
          </div>
        </section>

        {/* Network */}
        <section>
          <h3 className="text-xs font-semibold text-fg-muted uppercase tracking-wider mb-3">{t("form.network")}</h3>
          <div className="space-y-3">
            <div>
              <label className="label" htmlFor="pf-proxy">{t("form.proxy")}</label>
              <select
                id="pf-proxy"
                className="input"
                value={form.proxy_id ?? ""}
                onChange={(e) => set("proxy_id", e.target.value || null)}
              >
                <option value="">{t("form.noProxy")}</option>
                {proxies.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name} ({p.protocol}://{p.host}:{p.port})
                  </option>
                ))}
              </select>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="label" htmlFor="pf-tz">{t("form.timezone")}</label>
                <input
                  id="pf-tz"
                  className="input"
                  value={form.timezone ?? ""}
                  onChange={(e) => set("timezone", e.target.value || null)}
                  placeholder="Asia/Ho_Chi_Minh"
                />
              </div>
              <div>
                <label className="label" htmlFor="pf-locale">{t("form.locale")}</label>
                <input
                  id="pf-locale"
                  className="input"
                  value={form.locale ?? ""}
                  onChange={(e) => set("locale", e.target.value || null)}
                  placeholder="vi-VN"
                />
              </div>
            </div>
            <label className="flex items-center gap-2 text-sm cursor-pointer">
              <input
                type="checkbox"
                checked={form.geoip ?? false}
                onChange={(e) => set("geoip", e.target.checked)}
                className="rounded border-border bg-surface-2"
              />
              {t("form.geoip")}
            </label>
          </div>
        </section>

        {/* Hardware */}
        <section>
          <h3 className="text-xs font-semibold text-fg-muted uppercase tracking-wider mb-3">{t("form.hardware")}</h3>
          <div className="space-y-3">
            <div>
              <label className="label" htmlFor="pf-res">{t("form.resolution")}</label>
              <select
                id="pf-res"
                className="input"
                value={currentResolution}
                onChange={(e) => {
                  const preset = RESOLUTION_PRESETS[e.target.value];
                  if (preset) {
                    setForm((prev) => ({ ...prev, screen_width: preset.width, screen_height: preset.height }));
                  }
                }}
              >
                {Object.keys(RESOLUTION_PRESETS).map((name) => (
                  <option key={name} value={name}>{name}</option>
                ))}
                <option value="custom">{t("form.custom")}</option>
              </select>
            </div>
            {currentResolution === "custom" && (
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="label" htmlFor="pf-w">{t("form.width")}</label>
                  <input
                    id="pf-w"
                    className="input"
                    type="number"
                    value={form.screen_width ?? 1920}
                    onChange={(e) => set("screen_width", Number(e.target.value))}
                  />
                </div>
                <div>
                  <label className="label" htmlFor="pf-h">{t("form.height")}</label>
                  <input
                    id="pf-h"
                    className="input"
                    type="number"
                    value={form.screen_height ?? 1080}
                    onChange={(e) => set("screen_height", Number(e.target.value))}
                  />
                </div>
              </div>
            )}
            <div>
              <label className="label" htmlFor="pf-hc">{t("form.hardwareConcurrency")}</label>
              <input
                id="pf-hc"
                className="input no-spin"
                type="number"
                value={form.hardware_concurrency ?? ""}
                onChange={(e) => set("hardware_concurrency", e.target.value ? Number(e.target.value) : null)}
                placeholder={t("form.auto")}
              />
            </div>
            <div>
              <label className="label" htmlFor="pf-gpu-preset">{t("form.gpuPreset")}</label>
              <select
                id="pf-gpu-preset"
                className="input"
                value=""
                onChange={(e) => {
                  if (e.target.value) applyGpuPreset(e.target.value);
                }}
              >
                <option value="">{t("form.selectPreset")}</option>
                {Object.keys(GPU_PRESETS).map((name) => (
                  <option key={name} value={name}>{name}</option>
                ))}
              </select>
            </div>
            <div>
              <label className="label" htmlFor="pf-gpu-v">{t("form.gpuVendor")}</label>
              <input
                id="pf-gpu-v"
                className="input"
                value={form.gpu_vendor ?? ""}
                onChange={(e) => set("gpu_vendor", e.target.value || null)}
                placeholder={t("form.auto")}
              />
            </div>
            <div>
              <label className="label" htmlFor="pf-gpu-r">{t("form.gpuRenderer")}</label>
              <input
                id="pf-gpu-r"
                className="input"
                value={form.gpu_renderer ?? ""}
                onChange={(e) => set("gpu_renderer", e.target.value || null)}
                placeholder={t("form.auto")}
              />
            </div>
          </div>
        </section>

        {/* Behavior */}
        <section>
          <h3 className="text-xs font-semibold text-fg-muted uppercase tracking-wider mb-3">{t("form.behavior")}</h3>
          <div className="space-y-3">
            <label className="flex items-center gap-2 text-sm cursor-pointer">
              <input
                type="checkbox"
                checked={form.humanize ?? false}
                onChange={(e) => set("humanize", e.target.checked)}
                className="rounded border-border bg-surface-2"
              />
              {t("form.humanize")}
            </label>
            {form.humanize && (
              <div>
                <label className="label" htmlFor="pf-human">{t("form.humanPreset")}</label>
                <select
                  id="pf-human"
                  className="input"
                  value={form.human_preset ?? "default"}
                  onChange={(e) => set("human_preset", e.target.value)}
                >
                  <option value="default">{t("form.humanDefault")}</option>
                  <option value="careful">{t("form.humanCareful")}</option>
                </select>
              </div>
            )}
            <label className="flex items-center gap-2 text-sm cursor-pointer">
              <input
                type="checkbox"
                checked={form.headless ?? false}
                onChange={(e) => set("headless", e.target.checked)}
                className="rounded border-border bg-surface-2"
              />
              {t("form.headless")}
            </label>
            <div>
              <label className="label" htmlFor="pf-cs">{t("form.colorScheme")}</label>
              <select
                id="pf-cs"
                className="input"
                value={form.color_scheme ?? ""}
                onChange={(e) => set("color_scheme", e.target.value || null)}
              >
                <option value="">{t("form.systemDefault")}</option>
                <option value="light">{t("form.light")}</option>
                <option value="dark">{t("form.dark")}</option>
              </select>
            </div>
          </div>
        </section>

        {/* Tags */}
        <section>
          <h3 className="text-xs font-semibold text-fg-muted uppercase tracking-wider mb-3">{t("form.tags")}</h3>
          {(form.tags ?? []).length > 0 && (
            <div className="flex flex-wrap gap-1.5 mb-3">
              {(form.tags ?? []).map((tag) => (
                <span key={tag} className="inline-flex items-center gap-1 text-xs px-2 py-1 rounded-full bg-surface-3">
                  {tag}
                  <button
                    type="button"
                    onClick={() => removeTag(tag)}
                    className="hover:opacity-70"
                    aria-label={`${t("form.delete")}: ${tag}`}
                  >
                    <X className="h-3 w-3" aria-hidden="true" />
                  </button>
                </span>
              ))}
            </div>
          )}
          <div className="flex gap-2">
            <input
              className="input flex-1"
              value={tagInput}
              onChange={(e) => setTagInput(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); addTag(); } }}
              placeholder={t("form.addTag")}
              aria-label={t("form.addTag")}
            />
            <button type="button" onClick={addTag} className="btn-secondary text-xs">
              {t("form.add")}
            </button>
          </div>
        </section>

        {/* Launch args */}
        <section>
          <h3 className="text-xs font-semibold text-fg-muted uppercase tracking-wider mb-3">{t("form.launchArgs")}</h3>
          <p className="text-xs text-fg-muted mb-2">{t("form.launchArgsHint")}</p>
          {(form.launch_args ?? []).length > 0 && (
            <div className="flex flex-wrap gap-1.5 mb-3">
              {(form.launch_args ?? []).map((arg, idx) => (
                <span key={idx} className="inline-flex items-center gap-1 text-xs px-2 py-1 rounded-full bg-surface-3 font-mono">
                  {arg}
                  <button
                    type="button"
                    onClick={() => removeLaunchArg(idx)}
                    className="hover:opacity-70"
                    aria-label={`${t("form.delete")}: ${arg}`}
                  >
                    <X className="h-3 w-3" aria-hidden="true" />
                  </button>
                </span>
              ))}
            </div>
          )}
          <div className="flex gap-2">
            <input
              className="input flex-1 font-mono"
              value={launchArgInput}
              onChange={(e) => setLaunchArgInput(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); addLaunchArg(); } }}
              placeholder="--lang=vi"
              aria-label={t("form.launchArgs")}
            />
            <button type="button" onClick={addLaunchArg} className="btn-secondary text-xs">
              {t("form.add")}
            </button>
          </div>
        </section>

        {/* Notes */}
        <section>
          <h3 className="text-xs font-semibold text-fg-muted uppercase tracking-wider mb-3">{t("form.notes")}</h3>
          <textarea
            className="input min-h-[80px] resize-y"
            value={form.notes ?? ""}
            onChange={(e) => set("notes", e.target.value || null)}
            placeholder={t("form.notesPlaceholder")}
            aria-label={t("form.notes")}
          />
        </section>
      </div>
    </form>
  );
}
