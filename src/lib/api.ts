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
  /**
   * Target folder. NOTE: the Rust ProfileInput/ProfileUpdate structs do not
   * (yet) read this field — serde ignores it. Callers must follow up with
   * `api.moveProfilesToFolder([id], folderId)` to persist the assignment.
   */
  folder_id?: string | null;
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
  username: string | null;
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
  searchProfiles: (query: string, tag?: string | null) =>
    invoke<Profile[]>("search_profiles", { query, tag: tag ?? null }),

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
  checkProxy: (input: ProxyCheckInput) =>
    invoke<ProxyCheckResult>("check_proxy", { input }),

  // Session
  launchProfile: (id: string) => invoke<LaunchResult>("launch_profile", { id }),
  stopProfile: (id: string) => invoke<void>("stop_profile", { id }),
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
  listTags: () => invoke<string[]>("list_tags"),
  setProfileTags: (id: string, tags: string[]) =>
    invoke<void>("set_profile_tags", { id, tags }),

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

  // Logs (W21b) — open ~/.browserx/logs in the OS file manager
  openLogsFolder: () => invoke<void>("open_logs_folder"),
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
