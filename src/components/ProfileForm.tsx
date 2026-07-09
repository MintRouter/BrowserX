import { ChevronLeft } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  api,
  engineVersionNewer,
  isTauri,
  type Extension,
  type Folder,
  type Platform,
  type Profile,
  type ProfileInput,
  type ProfileTemplate,
  type Proxy,
} from "../lib/api";
import { detectHostPlatform } from "../lib/host";
import { ConfirmDialog } from "./ConfirmDialog";
import { ExtraTab } from "./profile-form/ExtraTab";
import { FingerprintTab } from "./profile-form/FingerprintTab";
import { GeneralTab } from "./profile-form/GeneralTab";
import { OverviewPanel } from "./profile-form/OverviewPanel";
import { ProxyTab } from "./profile-form/ProxyTab";
import type { FormState, SetField } from "./profile-form/types";

interface ProfileFormProps {
  profile: Profile | null; // null = create mode
  proxies: Proxy[];
  onSave: (data: ProfileInput) => Promise<void>;
  onDelete?: () => Promise<void>;
  onCancel: () => void;
  /** Called after a successful move-to-trash so the parent can refetch. */
  onTrashed?: () => void | Promise<void>;
  /** Optional: pass folders to skip the internal api.listFolders() fetch. */
  folders?: Folder[];
  /** (P3-3b) Refetch proxies after "use template" creates one server-side. */
  onProxiesChanged?: () => void | Promise<void>;
  /** (W58c) Default engine version for new profiles — enables the per-profile
   * "outdated → Upgrade engine" affordance when this profile pins an older build. */
  defaultEngineVersion?: string | null;
  /** (W58c) True when this profile has a live session — upgrade is disabled. */
  isRunning?: boolean;
  /** (W58c) Refetch profiles after a successful engine upgrade. */
  onEngineUpgraded?: () => void | Promise<void>;
}

const TABS = ["general", "proxy", "fingerprint", "extra"] as const;
type TabId = (typeof TABS)[number];

const HOST_PLATFORM = detectHostPlatform();
const DEFAULT_FOLDER_NAME = "Default folder";

/** (W56) Random seed string for a new profile (same range as the 🎲 button). */
function randomSeed(): string {
  return String(Math.floor(Math.random() * 90000) + 10000);
}

function initialState(profile: Profile | null, defaultName: string): FormState {
  if (profile) {
    return {
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
      notes: profile.notes ?? "",
      proxy_id: profile.proxy_id,
      tags: profile.tags ?? [],
      folder_id: profile.folder_id,
      startup_behavior: profile.startup_behavior ?? "restore",
      startup_urls: profile.startup_urls ?? [],
      fp_noise: profile.fp_noise ?? true,
      webrtc_mode: profile.webrtc_mode ?? "real",
      webrtc_ip: profile.webrtc_ip,
      geolocation_mode: profile.geolocation_mode ?? "auto",
      geo_latitude: profile.geo_latitude,
      geo_longitude: profile.geo_longitude,
      store_history: profile.store_history ?? true,
      store_passwords: profile.store_passwords ?? true,
      store_sw_cache: profile.store_sw_cache ?? true,
      extensions: profile.extensions ?? [],
      nav_brand: profile.nav_brand ?? null,
      nav_brand_version: profile.nav_brand_version ?? null,
      platform_version: profile.platform_version ?? null,
      device_memory: profile.device_memory ?? null,
      fonts_dir: profile.fonts_dir ?? null,
      windows_font_metrics: profile.windows_font_metrics ?? false,
      storage_quota: profile.storage_quota ?? null,
      rotate_on_launch: profile.rotate_on_launch ?? false,
      taskbar_height: profile.taskbar_height ?? null,
    };
  }
  return {
    name: defaultName,
    // (W56) Create mode gets a visible random seed right away; GPU + screen
    // are then suggested from it on mount (deterministic per platform+seed).
    fingerprint_seed: randomSeed(),
    platform: HOST_PLATFORM,
    timezone: null,
    locale: null,
    screen_width: 1920,
    screen_height: 1080,
    gpu_vendor: null,
    gpu_renderer: null,
    hardware_concurrency: null,
    humanize: false,
    human_preset: "default",
    headless: false,
    geoip: false,
    color_scheme: null,
    notes: "",
    proxy_id: null,
    tags: [],
    folder_id: null,
    startup_behavior: "restore",
    startup_urls: [],
    fp_noise: true,
    webrtc_mode: "real",
    webrtc_ip: null,
    geolocation_mode: "auto",
    geo_latitude: null,
    geo_longitude: null,
    store_history: true,
    store_passwords: true,
    store_sw_cache: true,
    extensions: [],
    nav_brand: null,
    nav_brand_version: null,
    platform_version: null,
    device_memory: null,
    fonts_dir: null,
    windows_font_metrics: false,
    storage_quota: null,
    rotate_on_launch: false,
    taskbar_height: null,
  };
}

/** MLX-style section divider: muted label + hairline rule. */
function SectionHeading({ label }: { label: string }) {
  return (
    <div className="mb-4 flex items-center gap-3">
      <h2 className="text-sm font-medium text-fg-muted">{label}</h2>
      <div className="h-px flex-1 bg-border" aria-hidden="true" />
    </div>
  );
}

/** Parse the launch-args textarea: must be empty or a JSON array of strings. */
function parseLaunchArgs(text: string): { args: string[] } | { error: true } {
  const trimmed = text.trim();
  if (!trimmed) return { args: [] };
  try {
    const parsed: unknown = JSON.parse(trimmed);
    if (Array.isArray(parsed) && parsed.every((x) => typeof x === "string")) {
      return { args: parsed as string[] };
    }
  } catch {
    // fall through to error
  }
  return { error: true };
}

export function ProfileForm({
  profile,
  proxies,
  onSave,
  onDelete,
  onCancel,
  onTrashed,
  folders: foldersProp,
  onProxiesChanged,
  defaultEngineVersion,
  isRunning = false,
  onEngineUpgraded,
}: ProfileFormProps) {
  const { t } = useTranslation();
  const isEdit = profile !== null;

  const [form, setForm] = useState<FormState>(() =>
    initialState(profile, t("pform.ov.defaultName")),
  );
  const [argsText, setArgsText] = useState(() =>
    profile && profile.launch_args.length > 0
      ? JSON.stringify(profile.launch_args)
      : "",
  );
  const [activeTab, setActiveTab] = useState<TabId>("general");
  const [saving, setSaving] = useState(false);
  const [trashing, setTrashing] = useState(false);
  /** (W47) In-app confirmation before move-to-trash (window.confirm is a no-op in Tauri). */
  const [trashConfirm, setTrashConfirm] = useState(false);
  /** (W58c) Live engine version for this profile (updated after an upgrade). */
  const [engineVersion, setEngineVersion] = useState<string | null>(
    profile?.engine_version ?? null,
  );
  /** (W58c) Upgrade-engine confirmation dialog (fingerprint may change, one-way). */
  const [engineUpgradeConfirm, setEngineUpgradeConfirm] = useState(false);
  const [engineUpgrading, setEngineUpgrading] = useState(false);
  const [folders, setFolders] = useState<Folder[]>(foldersProp ?? []);
  const [allTags, setAllTags] = useState<string[]>([]);
  // (W20b) Profile templates: dropdown fills the form in create mode;
  // the "Save as a profile template" toggle snapshots the form on submit.
  const [templates, setTemplates] = useState<ProfileTemplate[]>([]);
  const [selectedTemplateId, setSelectedTemplateId] = useState("");
  const [saveAsTemplate, setSaveAsTemplate] = useState(false);
  // (P3-1b) Central extension store: tick-list in the Extra tab, saved via
  // assign_extensions (N-N) after the profile itself is saved.
  const [storeExtensions, setStoreExtensions] = useState<Extension[]>([]);
  const [assignedExtIds, setAssignedExtIds] = useState<Set<string>>(new Set());
  const extAssignDirty = useRef(false);
  const defaultFolderApplied = useRef(false);
  const tabRefs = useRef<Partial<Record<TabId, HTMLButtonElement | null>>>({});
  // (W50G) MLX parity: tabs are anchors into one long scroll page.
  const sectionRefs = useRef<Partial<Record<TabId, HTMLElement | null>>>({});
  const scrollRef = useRef<HTMLDivElement | null>(null);

  const set: SetField = (key, value) => {
    setForm((prev) => ({ ...prev, [key]: value }));
  };

  // (W56) Create mode: prefill GPU + screen consistent with (platform, seed).
  // Values land in the form so the user can see and override them.
  const suggestFingerprint = (platform: Platform, seed: string | null) => {
    if (!isTauri()) return;
    api
      .suggestFingerprint(platform, seed ?? "")
      .then((s) => {
        setForm((prev) => ({
          ...prev,
          screen_width: s.screen_width,
          screen_height: s.screen_height,
          gpu_vendor: s.gpu?.vendor ?? prev.gpu_vendor,
          gpu_renderer: s.gpu?.renderer ?? prev.gpu_renderer,
        }));
      })
      .catch(() => {
        // offline / non-Tauri: keep the current defaults
      });
  };

  // (W56) On mount (create mode only): suggest from the initial random seed.
  const suggestedOnMount = useRef(false);
  useEffect(() => {
    if (isEdit || suggestedOnMount.current) return;
    suggestedOnMount.current = true;
    suggestFingerprint(form.platform, form.fingerprint_seed);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- mount-only
  }, []);

  // (W56) 🎲 seed: new seed + (create mode) re-suggest GPU + screen so the
  // fingerprint stays consistent. Edit mode keeps the old set-seed-only flow.
  const handleRerollSeed = () => {
    const seed = randomSeed();
    set("fingerprint_seed", seed);
    if (!isEdit) suggestFingerprint(form.platform, seed);
  };

  // (W56) Platform switch in create mode re-suggests (a windows GPU pair is
  // impossible on macOS and vice versa). Edit mode is unchanged.
  const handlePlatformChange = (platform: Platform) => {
    set("platform", platform);
    if (!isEdit) suggestFingerprint(platform, form.fingerprint_seed);
  };

  // Re-sync when switching to a different profile.
  useEffect(() => {
    if (profile) {
      setForm(initialState(profile, ""));
      setArgsText(
        profile.launch_args.length > 0 ? JSON.stringify(profile.launch_args) : "",
      );
    }
  }, [profile?.id]);

  // Folders: prefer the optional prop, otherwise self-fetch.
  useEffect(() => {
    if (foldersProp) {
      setFolders(foldersProp);
      return;
    }
    let cancelled = false;
    api
      .listFolders()
      .then((f) => {
        if (!cancelled) setFolders(f);
      })
      .catch(() => {
        // offline / non-Tauri: keep empty list
      });
    return () => {
      cancelled = true;
    };
  }, [foldersProp]);

  // Tag suggestions.
  useEffect(() => {
    let cancelled = false;
    api
      .listTags()
      .then((tags) => {
        if (!cancelled) setAllTags(tags.map((t) => t.tag));
      })
      .catch(() => {
        // offline / non-Tauri: keep empty list
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // (W20b) Available templates for the create-mode dropdown.
  useEffect(() => {
    let cancelled = false;
    api
      .listTemplates()
      .then((list) => {
        if (!cancelled) setTemplates(list);
      })
      .catch(() => {
        // offline / non-Tauri: keep empty list
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // (P3-1b) Store extensions + the profile's current assignment (edit mode).
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [all, assigned] = await Promise.all([
          api.listExtensions(),
          profile ? api.getProfileExtensions(profile.id) : Promise.resolve([]),
        ]);
        if (cancelled) return;
        setStoreExtensions(all);
        setAssignedExtIds(new Set(assigned.map((e) => e.id)));
        extAssignDirty.current = false;
      } catch {
        // offline / non-Tauri: keep empty list
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [profile?.id]);

  const toggleExtension = (id: string) => {
    extAssignDirty.current = true;
    setAssignedExtIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  // Create mode: preselect the "Default folder" once folders arrive.
  useEffect(() => {
    if (isEdit || defaultFolderApplied.current || folders.length === 0) return;
    defaultFolderApplied.current = true;
    const def = folders.find((f) => f.name === DEFAULT_FOLDER_NAME) ?? folders[0];
    if (!def) return;
    setForm((prev) => (prev.folder_id ? prev : { ...prev, folder_id: def.id }));
  }, [folders, isEdit]);

  const parsedArgs = parseLaunchArgs(argsText);
  const argsInvalid = "error" in parsedArgs;
  const canSave = form.name.trim().length > 0 && !argsInvalid && !saving;

  /** Snapshot the form as a ProfileInput (shared by save and save-as-template). */
  const buildInput = (launchArgs: string[]): ProfileInput => ({
    name: form.name.trim(),
    fingerprint_seed: form.fingerprint_seed,
    platform: form.platform,
    timezone: form.timezone,
    locale: form.locale,
    screen_width: form.screen_width,
    screen_height: form.screen_height,
    gpu_vendor: form.gpu_vendor,
    gpu_renderer: form.gpu_renderer,
    hardware_concurrency: form.hardware_concurrency,
    humanize: form.humanize,
    human_preset: form.human_preset,
    headless: form.headless,
    geoip: form.geoip,
    color_scheme: form.color_scheme,
    launch_args: launchArgs,
    notes: form.notes.trim() || null,
    proxy_id: form.proxy_id,
    tags: form.tags,
    folder_id: form.folder_id,
    startup_behavior: form.startup_behavior,
    startup_urls: form.startup_urls,
    fp_noise: form.fp_noise,
    webrtc_mode: form.webrtc_mode,
    webrtc_ip: form.webrtc_ip?.trim() || null,
    geolocation_mode: form.geolocation_mode,
    geo_latitude: form.geo_latitude?.trim() || null,
    geo_longitude: form.geo_longitude?.trim() || null,
    store_history: form.store_history,
    store_passwords: form.store_passwords,
    store_sw_cache: form.store_sw_cache,
    extensions: form.extensions,
    nav_brand: form.nav_brand?.trim() || null,
    nav_brand_version: form.nav_brand_version?.trim() || null,
    platform_version: form.platform_version?.trim() || null,
    device_memory: form.device_memory,
    fonts_dir: form.fonts_dir?.trim() || null,
    windows_font_metrics: form.windows_font_metrics,
    storage_quota: form.storage_quota,
    rotate_on_launch: form.rotate_on_launch,
    taskbar_height: form.taskbar_height,
  });

  // (W20b) Fill the form from a template (create mode). Name is kept as typed;
  // fingerprint seed stays null so the new profile gets a fresh one.
  const applyTemplate = (id: string) => {
    setSelectedTemplateId(id);
    const tpl = templates.find((x) => x.id === id);
    if (!tpl) return;
    const c = tpl.config;
    setForm((prev) => ({
      ...prev,
      fingerprint_seed: null,
      platform: c.platform ?? prev.platform,
      timezone: c.timezone ?? null,
      locale: c.locale ?? null,
      screen_width: c.screen_width ?? 1920,
      screen_height: c.screen_height ?? 1080,
      gpu_vendor: c.gpu_vendor ?? null,
      gpu_renderer: c.gpu_renderer ?? null,
      hardware_concurrency: c.hardware_concurrency ?? null,
      humanize: c.humanize ?? false,
      human_preset: c.human_preset ?? "default",
      headless: c.headless ?? false,
      geoip: c.geoip ?? false,
      color_scheme: c.color_scheme ?? null,
      notes: c.notes ?? "",
      proxy_id: c.proxy_id ?? null,
      tags: c.tags ?? [],
      folder_id: c.folder_id !== undefined ? c.folder_id : prev.folder_id,
      startup_behavior: c.startup_behavior ?? "restore",
      startup_urls: c.startup_urls ?? [],
      fp_noise: c.fp_noise ?? true,
      webrtc_mode: c.webrtc_mode ?? "real",
      webrtc_ip: c.webrtc_ip ?? null,
      geolocation_mode: c.geolocation_mode ?? "auto",
      geo_latitude: c.geo_latitude ?? null,
      geo_longitude: c.geo_longitude ?? null,
      store_history: c.store_history ?? true,
      store_passwords: c.store_passwords ?? true,
      store_sw_cache: c.store_sw_cache ?? true,
      extensions: c.extensions ?? [],
      nav_brand: c.nav_brand ?? null,
      nav_brand_version: c.nav_brand_version ?? null,
      platform_version: c.platform_version ?? null,
      device_memory: c.device_memory ?? null,
      fonts_dir: c.fonts_dir ?? null,
      windows_font_metrics: c.windows_font_metrics ?? false,
      storage_quota: c.storage_quota ?? null,
      rotate_on_launch: c.rotate_on_launch ?? false,
      taskbar_height: c.taskbar_height ?? null,
    }));
    setArgsText(
      c.launch_args && c.launch_args.length > 0
        ? JSON.stringify(c.launch_args)
        : "",
    );
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSave || "error" in parsedArgs) return;
    setSaving(true);
    try {
      const input = buildInput(parsedArgs.args);
      // (W20b) Toggle on → also snapshot the form as a reusable template.
      if (saveAsTemplate) {
        try {
          await api.saveAsTemplate(
            form.name.trim() || t("pform.ov.defaultName"),
            input,
          );
        } catch {
          // non-fatal: the profile is still saved without the template
        }
      }
      if (isEdit && profile) {
        // Backend ProfileUpdate has no folder_id — persist the move explicitly.
        if (form.folder_id !== profile.folder_id) {
          try {
            await api.moveProfilesToFolder([profile.id], form.folder_id);
          } catch {
            // non-fatal: profile is still saved without the folder change
          }
        }
        // (P3-1b) Persist store-extension assignment when the tick-list changed.
        if (extAssignDirty.current) {
          try {
            await api.assignExtensions(profile.id, [...assignedExtIds]);
          } catch {
            // non-fatal: profile is still saved without the assignment change
          }
        }
        await onSave(input);
      } else {
        await onSave(input);
        // Backend ProfileInput has no folder_id — locate the created profile
        // by name and move it into the selected folder / assign extensions.
        if (form.folder_id || assignedExtIds.size > 0) {
          try {
            const list = await api.listProfiles();
            const created = list
              .filter((p) => p.name === input.name)
              .sort((a, b) => b.created_at.localeCompare(a.created_at))[0];
            if (created) {
              if (form.folder_id && created.folder_id !== form.folder_id) {
                await api.moveProfilesToFolder([created.id], form.folder_id);
              }
              // (P3-1b) New profile: assign the ticked store extensions.
              if (assignedExtIds.size > 0) {
                await api.assignExtensions(created.id, [...assignedExtIds]);
              }
            }
          } catch {
            // non-fatal: profile created without folder/extension assignment
          }
        }
      }
    } finally {
      setSaving(false);
    }
  };

  const handleTrash = async () => {
    if (!profile) return;
    setTrashConfirm(false);
    setTrashing(true);
    try {
      try {
        await api.trashProfiles([profile.id]);
        await onTrashed?.();
        onCancel();
      } catch {
        // trash command unavailable → fall back to hard delete
        if (onDelete) await onDelete();
        else onCancel();
      }
    } finally {
      setTrashing(false);
    }
  };

  // (W58c) Re-pin this profile to the current default engine after the
  // fingerprint-change warning is confirmed. Backend rejects a running profile.
  const handleEngineUpgrade = async () => {
    if (!profile || !defaultEngineVersion) return;
    setEngineUpgrading(true);
    try {
      const updated = await api.upgradeProfileEngine(
        profile.id,
        defaultEngineVersion,
      );
      setEngineVersion(updated.engine_version);
      setEngineUpgradeConfirm(false);
      await onEngineUpgraded?.();
    } catch {
      // Surface nothing extra here — the dialog stays open so the user retries.
    } finally {
      setEngineUpgrading(false);
    }
  };

  // (W58c) Profile pins an engine older than the current default → offer upgrade.
  const engineOutdated =
    isEdit &&
    engineVersion !== null &&
    !!defaultEngineVersion &&
    engineVersionNewer(defaultEngineVersion, engineVersion);

  // (W50G) Anchor-tab click: smooth-scroll the section to the top of the page.
  const scrollToSection = (tab: TabId) => {
    setActiveTab(tab);
    const el = sectionRefs.current[tab];
    const container = scrollRef.current;
    if (!el || !container) return;
    container.scrollTo({
      top:
        el.getBoundingClientRect().top -
        container.getBoundingClientRect().top +
        container.scrollTop -
        12,
      behavior: "smooth",
    });
  };

  // (W50G) Scroll-spy: underline the tab of the section nearest the top.
  const handleScroll = () => {
    const container = scrollRef.current;
    if (!container) return;
    const cTop = container.getBoundingClientRect().top;
    let current: TabId = "general";
    for (const tab of TABS) {
      const el = sectionRefs.current[tab];
      if (el && el.getBoundingClientRect().top - cTop <= 96) current = tab;
    }
    if (container.scrollTop + container.clientHeight >= container.scrollHeight - 4) {
      current = "extra";
    }
    setActiveTab(current);
  };

  const handleTabKeyDown = (e: React.KeyboardEvent) => {
    const idx = TABS.indexOf(activeTab);
    let next: TabId | null = null;
    if (e.key === "ArrowRight") next = TABS[(idx + 1) % TABS.length] ?? null;
    else if (e.key === "ArrowLeft") next = TABS[(idx - 1 + TABS.length) % TABS.length] ?? null;
    else if (e.key === "Home") next = TABS[0] ?? null;
    else if (e.key === "End") next = TABS[TABS.length - 1] ?? null;
    if (next) {
      e.preventDefault();
      scrollToSection(next);
      tabRefs.current[next]?.focus();
    }
  };

  return (
    <form onSubmit={handleSubmit} className="flex h-full min-h-0 flex-col p-4">
      {/* (R6 2.1) White card wrapper: full-height, 16px margin, 8px radius,
          footer inside the card. */}
      <div className="card flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Header */}
      <div className="px-6 pb-3 pt-5">
        <button
          type="button"
          onClick={onCancel}
          className="inline-flex items-center gap-0.5 rounded text-sm font-medium text-accent hover:text-accent-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
        >
          <ChevronLeft className="h-4 w-4" aria-hidden="true" />
          {t("pform.back")}
        </button>
        <h1 className="mt-1 text-lg font-medium leading-6 text-[#1D192B] dark:text-fg">
          {isEdit ? t("pform.editTitle") : t("pform.createTitle")}
        </h1>
      </div>

      {/* (W50G) Anchor-tab bar: MLX parity — tabs scroll to sections in one long page */}
      <nav
        aria-label={t("pform.tabsLabel")}
        onKeyDown={handleTabKeyDown}
        className="flex border-b border-border px-6"
      >
        {TABS.map((tab) => {
          const active = tab === activeTab;
          return (
            <button
              key={tab}
              ref={(el) => {
                tabRefs.current[tab] = el;
              }}
              type="button"
              id={`pf-tab-${tab}`}
              aria-current={active ? "true" : undefined}
              onClick={() => scrollToSection(tab)}
              className={[
                "-mb-px inline-flex h-12 items-center border-b-2 px-8 text-sm font-medium",
                "transition-colors motion-reduce:transition-none",
                "focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60",
                active
                  ? "border-accent text-accent"
                  : "border-transparent text-[#1D192B] hover:text-fg dark:text-fg",
              ].join(" ")}
            >
              {t(`pform.tabs.${tab}`)}
            </button>
          );
        })}
      </nav>

      {/* (W50G) Content: one long scroll page — all sections stacked in the
          ~560px form column, overview panel sticky on the right. */}
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="min-h-0 flex-1 overflow-y-auto"
      >
        <div className="flex flex-col gap-6 px-6 py-5 lg:flex-row lg:items-start">
          <div className="min-w-0 flex-1 space-y-8 lg:max-w-[560px]">
            <section
              id="pf-section-general"
              aria-label={t("pform.tabs.general")}
              ref={(el) => {
                sectionRefs.current.general = el;
              }}
            >
              <GeneralTab
                form={form}
                set={set}
                folders={folders}
                allTags={allTags}
                autoFocusName={!isEdit}
                templates={templates}
                selectedTemplateId={selectedTemplateId}
                onApplyTemplate={isEdit ? undefined : applyTemplate}
                saveAsTemplate={saveAsTemplate}
                onSaveAsTemplateChange={setSaveAsTemplate}
              />
            </section>
            <section
              id="pf-section-proxy"
              aria-label={t("pform.tabs.proxy")}
              ref={(el) => {
                sectionRefs.current.proxy = el;
              }}
            >
              <SectionHeading label={t("pform.tabs.proxy")} />
              <ProxyTab
                form={form}
                set={set}
                proxies={proxies}
                onProxiesChanged={onProxiesChanged}
              />
            </section>
            <section
              id="pf-section-fingerprint"
              aria-label={t("pform.tabs.fingerprint")}
              ref={(el) => {
                sectionRefs.current.fingerprint = el;
              }}
            >
              <SectionHeading label={t("pform.tabs.fingerprint")} />
              <FingerprintTab
                form={form}
                set={set}
                onRerollSeed={handleRerollSeed}
                onPlatformChange={handlePlatformChange}
              />
            </section>
            <section
              id="pf-section-extra"
              aria-label={t("pform.tabs.extra")}
              ref={(el) => {
                sectionRefs.current.extra = el;
              }}
            >
              <SectionHeading label={t("pform.tabs.extra")} />
              <ExtraTab
                form={form}
                set={set}
                argsText={argsText}
                onArgsChange={setArgsText}
                argsError={argsInvalid ? t("pform.launchArgsError") : null}
                storeExtensions={storeExtensions}
                assignedExtIds={assignedExtIds}
                onToggleExtension={toggleExtension}
              />
            </section>
          </div>
          <aside
            aria-label={t("pform.ov.title")}
            className="w-full shrink-0 lg:sticky lg:top-0 lg:w-[340px]"
          >
            <OverviewPanel form={form} proxies={proxies} />
            {isEdit && engineVersion !== null && (
              <div className="mt-4 rounded-lg border border-border bg-surface-1 px-4 py-3">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-sm font-medium text-fg-muted">
                    {t("engineUpgrade.label")}
                  </span>
                  <span className="rounded bg-surface-2 px-2 py-0.5 text-xs font-medium text-fg">
                    {engineVersion}
                  </span>
                </div>
                {engineOutdated && (
                  <>
                    <p className="mt-2 text-xs text-fg-muted">
                      {t("engineUpgrade.outdated", {
                        latest: defaultEngineVersion,
                      })}
                    </p>
                    <button
                      type="button"
                      onClick={() => setEngineUpgradeConfirm(true)}
                      disabled={isRunning}
                      title={isRunning ? t("engineUpgrade.runningHint") : ""}
                      className="btn-secondary mt-2 h-9 w-full px-3 text-sm disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      {t("engineUpgrade.button")}
                    </button>
                    {isRunning && (
                      <p className="mt-1.5 text-xs text-warning">
                        {t("engineUpgrade.runningHint")}
                      </p>
                    )}
                  </>
                )}
              </div>
            )}
          </aside>
        </div>
      </div>

      {/* Sticky footer */}
      <div className="flex items-center gap-3 border-t border-border bg-surface-1 px-6 py-3">
        <button type="submit" disabled={!canSave} className="btn-primary h-10 px-3">
          {saving
            ? t("pform.saving")
            : isEdit
              ? t("pform.save")
              : t("pform.create")}
        </button>
        <button
          type="button"
          onClick={onCancel}
          className="inline-flex h-10 items-center rounded px-3 text-sm font-medium text-accent hover:text-accent-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
        >
          {t("pform.cancel")}
        </button>
        {isEdit && (
          <button
            type="button"
            onClick={() => setTrashConfirm(true)}
            disabled={trashing}
            className="btn-danger ml-auto"
          >
            {trashing ? t("pform.trashing") : t("pform.moveToTrash")}
          </button>
        )}
      </div>
      </div>
      {trashConfirm && (
        <ConfirmDialog
          message={t("pform.confirmTrash")}
          confirmLabel={t("pform.moveToTrash")}
          busy={trashing}
          onConfirm={() => void handleTrash()}
          onCancel={() => setTrashConfirm(false)}
        />
      )}
      {engineUpgradeConfirm && (
        <ConfirmDialog
          title={t("engineUpgrade.confirmTitle")}
          message={t("engineUpgrade.confirmMessage", {
            latest: defaultEngineVersion ?? "",
          })}
          confirmLabel={t("engineUpgrade.button")}
          busy={engineUpgrading}
          onConfirm={() => void handleEngineUpgrade()}
          onCancel={() => setEngineUpgradeConfirm(false)}
        />
      )}
    </form>
  );
}
