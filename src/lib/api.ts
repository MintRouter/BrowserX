/**
 * API client for BrowserX — wraps Tauri `invoke()` per the command contract in the spec.
 *
 * NOTE for backend (W3a): invoke argument keys use Tauri v2's default camelCase
 * mapping of Rust snake_case parameters (e.g. Rust `profile_id` → JS `profileId`).
 * If commands use `#[tauri::command(rename_all = "snake_case")]`, only this file
 * needs to change.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// --- Types (mirror src-tauri/src/models.rs) ---

export type Platform = "windows" | "macos" | "linux";

/** What the browser opens on launch: previous session or a custom URL list. */
export type StartupBehavior = "restore" | "custom";

/** WebRTC handling: "real" leaves it untouched; "masked" spoofs the ICE public IP. */
export type WebrtcMode = "real" | "masked";

/** Geolocation: "auto" follows IP/system; "manual" uses geo_latitude/geo_longitude. */
export type GeolocationMode = "auto" | "manual";

export interface Profile {
  id: string;
  name: string;
  fingerprint_seed: string;
  platform: Platform;
  timezone: string | null;
  locale: string | null;
  screen_width: number;
  screen_height: number;
  gpu_vendor: string | null;
  gpu_renderer: string | null;
  hardware_concurrency: number;
  humanize: boolean;
  human_preset: string | null;
  headless: boolean;
  geoip: boolean;
  color_scheme: string | null;
  launch_args: string[];
  user_data_dir: string;
  notes: string | null;
  folder_id: string | null;
  favorite: boolean;
  /** Quick (use-and-discard) profile — stopping prompts Save as regular / Close & delete. */
  is_quick: boolean;
  proxy_id: string | null;
  tags: string[];
  created_at: string;
  updated_at: string;
  /** Last successful launch (RFC3339 UTC). Null = never started. */
  last_start_at: string | null;
  /** "restore" reopens the previous session; "custom" opens `startup_urls`. */
  startup_behavior: StartupBehavior;
  /** URLs opened on launch when startup_behavior = "custom". */
  startup_urls: string[];
  /** (W19c) Canvas/WebGL/audio noise injection master switch (default true). */
  fp_noise: boolean;
  /** (W19c) WebRTC mode; "masked" spoofs ICE public IP via `webrtc_ip`. */
  webrtc_mode: WebrtcMode;
  /** (W19c) Public IP to spoof when webrtc_mode = "masked". Null = auto (from proxy/network). */
  webrtc_ip: string | null;
  /** (W19c) Geolocation mode; "manual" uses geo_latitude/geo_longitude. */
  geolocation_mode: GeolocationMode;
  geo_latitude: string | null;
  geo_longitude: string | null;
  /** (W20b) Keep browsing history. false = History files wiped on session stop. */
  store_history: boolean;
  /** (W20b) Keep saved passwords. false = Login Data wiped on session stop. */
  store_passwords: boolean;
  /** (W20b) Keep service worker cache. false = Service Worker dir wiped on session stop. */
  store_sw_cache: boolean;
  /** (W24b) Local unpacked extension paths — passed as --load-extension on launch. */
  extensions: string[];
  /** (P3-5a) Browser brand (Chrome/Edge/Opera/Vivaldi). Null = auto. → --fingerprint-brand. */
  nav_brand: string | null;
  /** (P3-5a) Brand version (UA + Client Hints). Null = auto. → --fingerprint-brand-version. */
  nav_brand_version: string | null;
  /** (P3-5a) Client Hints platform version. Null = auto. → --fingerprint-platform-version. */
  platform_version: string | null;
  /** (P3-5a) navigator.deviceMemory in GB. Null/0 = auto. → --fingerprint-device-memory. */
  device_memory: number | null;
  /** (P3-5a) Target-platform fonts directory. Null = skip. → --fingerprint-fonts-dir. */
  fonts_dir: string | null;
  /** (P3-5a) Align font metrics with Windows (Chromium 148+). → --fingerprint-windows-font-metrics. */
  windows_font_metrics: boolean;
  /** (P3-5a) Override storage quota in MB. Null = auto. → --fingerprint-storage-quota. */
  storage_quota: number | null;
  /** (W42) Auto-rotate the assigned proxy (round-robin) on each launch. */
  rotate_on_launch: boolean;
  /** (W44) Taskbar height in px (affects screen.availHeight). Null = binary default (Win 48 / Mac 95 / Linux 0). → --fingerprint-taskbar-height. */
  taskbar_height: number | null;
}

export interface Folder {
  id: string;
  name: string;
  /** Number of non-trashed profiles in this folder. */
  profile_count: number;
  created_at: string;
}

export interface ProfileInput {
  name: string;
  fingerprint_seed?: string | null;
  platform?: Platform;
  timezone?: string | null;
  locale?: string | null;
  screen_width?: number;
  screen_height?: number;
  gpu_vendor?: string | null;
  gpu_renderer?: string | null;
  hardware_concurrency?: number | null;
  humanize?: boolean;
  human_preset?: string | null;
  headless?: boolean;
  geoip?: boolean;
  color_scheme?: string | null;
  launch_args?: string[];
  notes?: string | null;
  proxy_id?: string | null;
  tags?: string[];
  /** Mark as quick (use-and-discard) profile. Default: false. */
  is_quick?: boolean;
  /** "restore" (default) | "custom". */
  startup_behavior?: StartupBehavior;
  /** URLs opened on launch when startup_behavior = "custom". */
  startup_urls?: string[];
  /** (W19c) Noise injection master switch. Default: true. */
  fp_noise?: boolean;
  /** (W19c) "real" (default) | "masked". */
  webrtc_mode?: WebrtcMode;
  /** (W19c) Public IP to spoof when masked; null = auto. */
  webrtc_ip?: string | null;
  /** (W19c) "auto" (default) | "manual". */
  geolocation_mode?: GeolocationMode;
  geo_latitude?: string | null;
  geo_longitude?: string | null;
  /** (W20b) Keep browsing history. Default: true. */
  store_history?: boolean;
  /** (W20b) Keep saved passwords. Default: true. */
  store_passwords?: boolean;
  /** (W20b) Keep service worker cache. Default: true. */
  store_sw_cache?: boolean;
  /** (W24b) Local unpacked extension paths. Default: []. */
  extensions?: string[];
  /** (P3-5a) Browser brand (Chrome/Edge/Opera/Vivaldi). Null/omitted = auto. */
  nav_brand?: string | null;
  /** (P3-5a) Brand version (UA + Client Hints). Null/omitted = auto. */
  nav_brand_version?: string | null;
  /** (P3-5a) Client Hints platform version. Null/omitted = auto. */
  platform_version?: string | null;
  /** (P3-5a) navigator.deviceMemory in GB. Null/0 = auto. */
  device_memory?: number | null;
  /** (P3-5a) Target-platform fonts directory. Null/omitted = skip. */
  fonts_dir?: string | null;
  /** (P3-5a) Align font metrics with Windows (Chromium 148+). Default: false. */
  windows_font_metrics?: boolean;
  /** (P3-5a) Override storage quota in MB. Null/omitted = auto. */
  storage_quota?: number | null;
  /** (W42) Auto-rotate the assigned proxy on each launch. Default: false. */
  rotate_on_launch?: boolean;
  /** (W44) Taskbar height in px. Null/omitted = binary default (Win 48 / Mac 95 / Linux 0). */
  taskbar_height?: number | null;
  /**
   * Target folder. NOTE: the Rust ProfileInput/ProfileUpdate structs do not
   * (yet) read this field — serde ignores it. Callers must follow up with
   * `api.moveProfilesToFolder([id], folderId)` to persist the assignment.
   */
  folder_id?: string | null;
}

/**
 * (P3-2a) Advanced filter for searchProfiles — mirrors src-tauri/src/db.rs
 * ProfileFilter (serde snake_case, no rename). Omitted fields skip that
 * criterion; an empty/omitted filter behaves like the old name-only search.
 */
export interface ProfileFilter {
  /** Target OS fingerprint (profiles.platform). */
  os?: Platform;
  /** true = only profiles with a proxy assigned, false = only without. */
  has_proxy?: boolean;
  /** Only profiles carrying this exact tag. */
  tag?: string;
  /** Only profiles in this folder. */
  folder_id?: string;
}

/** (P3-1a) Extension in the central store, assigned N-N to profiles. */
export interface Extension {
  id: string;
  /** Display name (read from manifest.json when added). */
  name: string;
  /** "folder" (local unpacked) | "store" (downloaded from Chrome Web Store). */
  source_type: "folder" | "store";
  /** Source folder path, or the 32-char Web Store extension id. */
  source_ref: string;
  /** Unpacked dir passed to --load-extension at launch (only when enabled). */
  unpacked_path: string;
  /** Disabled = kept in store/assignments but not loaded at launch. */
  enabled: boolean;
  created_at: string;
}

/** (W20b) Saved profile template — `config` is a ProfileInput-shaped snapshot. */
export interface ProfileTemplate {
  id: string;
  name: string;
  config: ProfileInput;
  created_at: string;
}

export interface Proxy {
  id: string;
  name: string;
  protocol: "http" | "https" | "socks5";
  host: string;
  port: number;
  /** (W5c) Username never crosses IPC in plaintext — masked (first char + "***"). */
  masked_username: string | null;
  /** Password never crosses IPC in plaintext — backend only reports presence. */
  has_password: boolean;
  /** (W23b) Credentials can't be decrypted (master key changed) — re-enter password. */
  credentials_invalid: boolean;
  created_at: string;
  updated_at: string;
}

export interface ProxyInput {
  name: string;
  protocol: "http" | "https" | "socks5";
  host: string;
  port: number;
  username?: string | null;
  password?: string | null;
}

/** (P3-3a) Reusable proxy template — same credential policy as Proxy: encrypted
 * at rest, only masked username + has_password cross IPC. sticky_session /
 * traffic_saver are provider-level metadata (not applied at launch). */
export interface ProxyTemplate {
  id: string;
  name: string;
  protocol: "http" | "https" | "socks5";
  host: string;
  port: number;
  /** (W5c) Username never crosses IPC in plaintext — masked (first char + "***"). */
  masked_username: string | null;
  /** Password never crosses IPC in plaintext — backend only reports presence. */
  has_password: boolean;
  /** (W23b) Credentials can't be decrypted (master key changed) — re-enter password. */
  credentials_invalid: boolean;
  sticky_session: boolean;
  traffic_saver: boolean;
  created_at: string;
  updated_at: string;
}

/** Input for create_proxy_template (plaintext credentials — encrypted at rest). */
export interface ProxyTemplateCreate {
  name: string;
  protocol: "http" | "https" | "socks5";
  host: string;
  port: number;
  username?: string | null;
  password?: string | null;
  sticky_session?: boolean;
  traffic_saver?: boolean;
}

/** Partial update for update_proxy_template (blank credential = keep stored). */
export interface ProxyTemplatePatch {
  name?: string;
  protocol?: "http" | "https" | "socks5";
  host?: string;
  port?: number;
  username?: string | null;
  password?: string | null;
  sticky_session?: boolean;
  traffic_saver?: boolean;
  clear_credentials?: boolean;
}

/** Input for check_proxy: either a stored proxy id, or inline params. */
export interface ProxyCheckInput {
  proxy_id?: string | null;
  protocol?: "http" | "https" | "socks5";
  host?: string;
  port?: number;
  username?: string | null;
  password?: string | null;
}

/** Result of an on-demand proxy check (W19b). */
export interface ProxyCheckResult {
  ok: boolean;
  external_ip: string | null;
  country: string | null;
  latency_ms: number | null;
  error: string | null;
}

export interface RunningSession {
  profile_id: string;
  pid: number;
  cdp_port: number;
  cdp_url: string;
  started_at: string;
}

export interface LaunchResult {
  pid: number;
  cdp_url: string;
  cdp_port: number;
}

export interface ProfileStatusEvent {
  profile_id: string;
  status: "starting" | "running" | "stopped" | "crashed" | "error";
  pid?: number;
  cdp_url?: string;
}

/** (W23a) Payload of `app://exit-requested` — quit attempted with sessions running. */
export interface ExitRequestedEvent {
  count: number;
}

export interface BinaryProgressEvent {
  phase: string;
  pct: number;
  /** Bytes downloaded so far (0 outside the "download" phase). */
  downloadedBytes: number;
  /** Total bytes to download (0 when the server sends no Content-Length). */
  totalBytes: number;
}

/** (W25a) Payload of `backup://progress` during create/restore backup. */
export interface BackupProgressEvent {
  /** create: compress | encrypt | write | done — restore: decrypt | unpack | swap | done */
  phase: string;
  pct: number;
}

/** (W25a) Result of create_backup. */
export interface BackupResult {
  /** Full path of the written .browserx-backup file. */
  path: string;
  bytes: number;
}

/** (W25a) Result of restore_backup — the app must be restarted afterwards. */
export interface RestoreResult {
  /** Where the previous data dir was kept (null when none existed). */
  previousDataDir: string | null;
}

export interface ProfileStorageSize {
  profile_id: string;
  bytes: number;
}

export interface ClearCacheResult {
  profile_id: string;
  freed_bytes: number;
  /** Present when clearing was refused (profile running) or failed. */
  error?: string;
}

/** (W24a) Result of a cookie export: serialized content + cookie count. */
export interface CookieExportResult {
  data: string;
  count: number;
}

/** (W24a) Cookie export formats supported by the backend. */
export type CookieFormat = "json" | "netscape";

/** (P3-4a) Options for start_cookie_robot. */
export interface CookieRobotOptions {
  /** URLs to visit sequentially (scheme-less entries get https:// prepended). */
  urls: string[];
  /** Seconds per site; 0 = random 20–40s per site (values are clamped to 3–120). */
  dwellSecs?: number;
  /** Shuffle the URL list before visiting. */
  randomOrder?: boolean;
  /** Try to click common cookie-consent "Accept" buttons on each page. */
  processConsent?: boolean;
  /** Softly close the browser session when the run finishes. */
  closeWhenDone?: boolean;
}

/** (P3-4a) Payload of `cookierobot://progress`. */
export interface CookieRobotProgressEvent {
  profileId: string;
  /** 1-based index of the site being processed; 0 during "starting". */
  current: number;
  total: number;
  /** URL being processed; empty on global phases (starting/done/…). */
  url: string;
  phase:
    | "starting"
    | "proxy_check"
    | "goto"
    | "consent"
    | "dwell"
    | "closing"
    | "done"
    | "cancelled"
    | "error";
  error: string | null;
}

/** (W26a) One audit-log row — append-only trail of every state-changing action. */
export interface AuditEntry {
  id: number;
  /** RFC3339 UTC timestamp. */
  ts: string;
  /** Dotted action name, e.g. "profile.create", "proxy.check". */
  action: string;
  target_id: string | null;
  /** Free-form JSON details (never credentials); null when the action has none. */
  meta: unknown;
}

/** (W26a) Query for listAudit — cursor pagination via beforeId (newest first). */
export interface AuditQuery {
  /** Only actions starting with this prefix (e.g. "profile."). */
  actionPrefix?: string | null;
  /** Exact target id match. */
  targetId?: string | null;
  /** Only rows with id < beforeId — pass the last row's id to load the next page. */
  beforeId?: number | null;
  /** Page size; backend clamps to 1..=200 (default 50). */
  limit?: number;
}

/** (W26b) System metrics snapshot for the Settings "System" panel.
 * Launch counters + p95 are in-memory since app start (reset on restart);
 * RAM is the main browser process RSS only (null/missing = not measurable). */
export interface SystemMetrics {
  live_sessions: number;
  /** Sum of measured session RSS in MB; null when sessions exist but RSS is unavailable (e.g. Windows). */
  ram_total_mb: number | null;
  /** Per-session RSS in MB — may have fewer entries than live_sessions. */
  ram_per_session_mb: number[];
  /** p95 of successful launch durations (last 100 samples); null when no samples. */
  launch_p95_ms: number | null;
  launch_success: number;
  launch_fail: number;
}

/** (W48) One tag + its color (mirrors db.rs `TagInfo`, returned by `list_tags`). */
export interface TagInfo {
  tag: string;
  color: string | null;
}

/** Settings key: auto-clear a profile's cache when its session stops ("true"/"false"). */
export const AUTO_CLEAR_CACHE_SETTING = "auto_clear_cache_on_stop";

/** (F2b) Settings key: template id pre-selected as the default one. */
export const DEFAULT_TEMPLATE_SETTING = "default_template_id";

// --- Commands ---

export const api = {
  // Profiles
  listProfiles: () => invoke<Profile[]>("list_profiles"),
  getProfile: (id: string) => invoke<Profile>("get_profile", { id }),
  createProfile: (input: ProfileInput) =>
    invoke<Profile>("create_profile", { input }),
  updateProfile: (id: string, input: ProfileInput) =>
    invoke<Profile>("update_profile", { id, input }),
  deleteProfile: (id: string) => invoke<void>("delete_profile", { id }),
  searchProfiles: (query: string, filter?: ProfileFilter | null) =>
    invoke<Profile[]>("search_profiles", { query, filter: filter ?? null }),

  // Proxies
  listProxies: () => invoke<Proxy[]>("list_proxies"),
  createProxy: (input: ProxyInput) => invoke<Proxy>("create_proxy", { input }),
  updateProxy: (id: string, input: Partial<ProxyInput> & { clear_credentials?: boolean }) =>
    invoke<Proxy>("update_proxy", { id, input }),
  /** (W23b) Call once per app open — backend detects a changed master key and logs it. */
  masterKeyStatus: () => invoke<{ changed: boolean }>("master_key_status"),
  deleteProxy: (id: string) => invoke<void>("delete_proxy", { id }),
  assignProxy: (profileId: string, proxyId?: string | null) =>
    invoke<void>("assign_proxy", { profileId, proxyId: proxyId ?? null }),
  /** (W39) Rotate to the next healthy proxy in the profile's pool. */
  rotateProxy: (profileId: string) =>
    invoke<Proxy>("rotate_proxy", { profileId }),
  rotateProxies: (profileIds: string[]) =>
    invoke<Proxy[]>("rotate_proxies", { profileIds }),
  checkProxy: (input: ProxyCheckInput) =>
    invoke<ProxyCheckResult>("check_proxy", { input }),

  // Proxy templates (P3-3a)
  listProxyTemplates: () => invoke<ProxyTemplate[]>("list_proxy_templates"),
  createProxyTemplate: (input: ProxyTemplateCreate) =>
    invoke<ProxyTemplate>("create_proxy_template", { input }),
  updateProxyTemplate: (id: string, input: ProxyTemplatePatch) =>
    invoke<ProxyTemplate>("update_proxy_template", { id, input }),
  deleteProxyTemplate: (id: string) =>
    invoke<boolean>("delete_proxy_template", { id }),
  /** Copies the encrypted credentials server-side — nothing is decrypted. */
  createProxyFromTemplate: (templateId: string, name?: string | null) =>
    invoke<Proxy>("create_proxy_from_template", {
      templateId,
      name: name ?? null,
    }),

  // Session
  launchProfile: (id: string) =>
    invoke<LaunchResult>("launch_profile", { profileId: id }),
  stopProfile: (id: string) => invoke<void>("stop_profile", { profileId: id }),
  listRunning: () => invoke<RunningSession[]>("list_running"),
  // (W24c) CDP websocket endpoint (ws://127.0.0.1:{port}/devtools/browser/…)
  // of a running session — for Playwright/Puppeteer connectOverCDP.
  getCdpWsUrl: (profileId: string) =>
    invoke<string>("get_cdp_ws_url", { profileId }),
  // (W23a) Stop every running session (full cleanup) then exit the app.
  stopAllAndQuit: () => invoke<void>("stop_all_and_quit"),

  // Binary
  ensureBinary: () => invoke<string>("ensure_binary"),

  // Settings & tags
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  setSetting: (key: string, value: string) =>
    invoke<void>("set_setting", { key, value }),
  listTags: () => invoke<TagInfo[]>("list_tags"),
  setProfileTags: (id: string, tags: string[]) =>
    invoke<void>("set_profile_tags", { profileId: id, tags }),

  // Folders & favorites
  listFolders: () => invoke<Folder[]>("list_folders"),
  createFolder: (name: string) => invoke<Folder>("create_folder", { name }),
  renameFolder: (id: string, name: string) =>
    invoke<Folder>("rename_folder", { id, name }),
  deleteFolder: (id: string) => invoke<boolean>("delete_folder", { id }),
  setFavorite: (id: string, favorite: boolean) =>
    invoke<void>("set_favorite", { id, favorite }),
  moveProfilesToFolder: (profileIds: string[], folderId: string | null) =>
    invoke<void>("move_profiles_to_folder", { profileIds, folderId }),

  // Trash (soft-delete)
  trashProfiles: (profileIds: string[]) =>
    invoke<void>("trash_profiles", { profileIds }),
  restoreProfiles: (profileIds: string[]) =>
    invoke<void>("restore_profiles", { profileIds }),
  purgeProfiles: (profileIds: string[]) =>
    invoke<void>("purge_profiles", { profileIds }),
  listTrash: () => invoke<Profile[]>("list_trash"),

  // Quick profile (stop prompt: Save as regular / Close & delete)
  convertQuickProfile: (profileId: string) =>
    invoke<Profile>("convert_quick_profile", { profileId }),
  deleteQuickProfile: (profileId: string) =>
    invoke<void>("delete_quick_profile", { profileId }),

  // Storage (size + safe cache cleanup)
  profileStorageSizes: (ids: string[]) =>
    invoke<ProfileStorageSize[]>("profile_storage_sizes", { ids }),
  clearProfileCache: (ids: string[]) =>
    invoke<ClearCacheResult[]>("clear_profile_cache", { ids }),

  // Export / import profile (W19a) — .bxprofile JSON; proxy password is never included
  exportProfile: (id: string) => invoke<string>("export_profile", { id }),
  importProfile: (json: string) => invoke<Profile>("import_profile", { json }),

  // Cookies (W24a) — CDP Storage.getCookies/setCookies; profiles that aren't
  // running are opened headlessly in the background, then closed softly.
  exportCookies: (profileId: string, format: CookieFormat) =>
    invoke<CookieExportResult>("export_cookies", { profileId, format }),
  importCookies: (profileId: string, data: string) =>
    invoke<number>("import_cookies", { profileId, data }),

  // CookieRobot (P3-4a) — sequential cookie-warming bot for ONE profile
  // (launches the profile if needed; progress on `cookierobot://progress`).
  startCookieRobot: (profileId: string, opts: CookieRobotOptions) =>
    invoke<void>("start_cookie_robot", {
      profileId,
      urls: opts.urls,
      dwellSecs: opts.dwellSecs ?? 0,
      randomOrder: opts.randomOrder ?? false,
      processConsent: opts.processConsent ?? false,
      closeWhenDone: opts.closeWhenDone ?? false,
    }),
  stopCookieRobot: (profileId: string) =>
    invoke<void>("stop_cookie_robot", { profileId }),

  // Bring to front (W20a) — CDP Page.bringToFront on a running session (macOS
  // falls back to OS-level window activation when CDP fails)
  bringToFront: (profileId: string) =>
    invoke<void>("bring_to_front", { profileId }),

  // Profile templates (W20b)
  listTemplates: () => invoke<ProfileTemplate[]>("list_templates"),
  saveAsTemplate: (name: string, config: ProfileInput) =>
    invoke<ProfileTemplate>("save_as_template", { name, config }),
  updateTemplate: (id: string, name: string, config?: ProfileInput | null) =>
    invoke<ProfileTemplate>("update_template", {
      id,
      name,
      config: config ?? null,
    }),
  deleteTemplate: (id: string) => invoke<boolean>("delete_template", { id }),
  createProfileFromTemplate: (templateId: string, name?: string | null) =>
    invoke<Profile>("create_profile_from_template", {
      templateId,
      name: name ?? null,
    }),
  /** (W29a) Bulk create N profiles from a template — one call, one transaction. */
  createProfilesFromTemplate: (
    templateId: string,
    count: number,
    namePrefix?: string | null,
  ) =>
    invoke<Profile[]>("create_profiles_from_template", {
      templateId,
      count,
      namePrefix: namePrefix ?? null,
    }),

  // Logs (W21b) — open ~/.browserx/logs in the OS file manager
  openLogsFolder: () => invoke<void>("open_logs_folder"),

  // Backup/Restore (W25a) — AES-256-GCM + Argon2id over the whole ~/.browserx.
  // Both refuse while sessions are running; progress on `backup://progress`.
  createBackup: (passphrase: string, destDir?: string | null) =>
    invoke<BackupResult>("create_backup", { passphrase, destDir: destDir ?? null }),
  restoreBackup: (backupPath: string, passphrase: string) =>
    invoke<RestoreResult>("restore_backup", { backupPath, passphrase }),
  /** Restart the app — required after restoreBackup to load the new data dir. */
  restartApp: () => invoke<void>("restart_app"),

  // Extensions (P3-1a) — central store + N-N assignment to profiles.
  listExtensions: () => invoke<Extension[]>("list_extensions"),
  /** Add a local unpacked extension folder (must contain manifest.json). */
  addExtensionFromFolder: (path: string) =>
    invoke<Extension>("add_extension_from_folder", { path }),
  /** Download from a Chrome Web Store URL, unpack into ~/.browserx/extensions/<id>/. */
  addExtensionFromStoreUrl: (url: string) =>
    invoke<Extension>("add_extension_from_store_url", { url }),
  removeExtension: (id: string) => invoke<void>("remove_extension", { id }),
  setExtensionEnabled: (id: string, enabled: boolean) =>
    invoke<void>("set_extension_enabled", { id, enabled }),
  /** Replace the profile's FULL assignment list with extIds. */
  assignExtensions: (profileId: string, extIds: string[]) =>
    invoke<void>("assign_extensions", { profileId, extIds }),
  getProfileExtensions: (profileId: string) =>
    invoke<Extension[]>("get_profile_extensions", { profileId }),

  // Audit log (W26a) — newest first, cursor pagination by id.
  listAudit: (query?: AuditQuery) =>
    invoke<AuditEntry[]>("list_audit", {
      actionPrefix: query?.actionPrefix ?? null,
      targetId: query?.targetId ?? null,
      beforeId: query?.beforeId ?? null,
      limit: query?.limit ?? 50,
    }),

  // Observability (W26b) — live sessions / RAM / launch p95 / error counters.
  getMetrics: () => invoke<SystemMetrics>("get_metrics"),
};

// --- Events ---

export function onProfileStatus(
  cb: (e: ProfileStatusEvent) => void,
): Promise<UnlistenFn> {
  return listen<ProfileStatusEvent>("profile://status", (ev) => cb(ev.payload));
}

export function onBinaryProgress(
  cb: (e: BinaryProgressEvent) => void,
): Promise<UnlistenFn> {
  return listen<BinaryProgressEvent>("binary://progress", (ev) =>
    cb(ev.payload),
  );
}

/** (W25a) Progress of create/restore backup. */
export function onBackupProgress(
  cb: (e: BackupProgressEvent) => void,
): Promise<UnlistenFn> {
  return listen<BackupProgressEvent>("backup://progress", (ev) =>
    cb(ev.payload),
  );
}

/** (P3-4a) Progress of a running CookieRobot. */
export function onCookieRobotProgress(
  cb: (e: CookieRobotProgressEvent) => void,
): Promise<UnlistenFn> {
  return listen<CookieRobotProgressEvent>("cookierobot://progress", (ev) =>
    cb(ev.payload),
  );
}

/** (W23a) Backend requests confirmation before quitting with running sessions. */
export function onExitRequested(
  cb: (e: ExitRequestedEvent) => void,
): Promise<UnlistenFn> {
  return listen<ExitRequestedEvent>("app://exit-requested", (ev) =>
    cb(ev.payload),
  );
}

/** True when running inside the Tauri WebView (invoke available). */
export function isTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}
