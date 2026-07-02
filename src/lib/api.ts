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
  proxy_id: string | null;
  tags: string[];
  created_at: string;
  updated_at: string;
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
  status: "starting" | "running" | "stopped" | "error";
  pid?: number;
  cdp_url?: string;
}

export interface BinaryProgressEvent {
  phase: string;
  pct: number;
}

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
  updateProxy: (id: string, input: ProxyInput) =>
    invoke<Proxy>("update_proxy", { id, input }),
  deleteProxy: (id: string) => invoke<void>("delete_proxy", { id }),
  assignProxy: (profileId: string, proxyId?: string | null) =>
    invoke<void>("assign_proxy", { profileId, proxyId: proxyId ?? null }),

  // Session
  launchProfile: (id: string) => invoke<LaunchResult>("launch_profile", { id }),
  stopProfile: (id: string) => invoke<void>("stop_profile", { id }),
  listRunning: () => invoke<RunningSession[]>("list_running"),

  // Binary
  ensureBinary: () => invoke<string>("ensure_binary"),

  // Settings & tags
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  setSetting: (key: string, value: string) =>
    invoke<void>("set_setting", { key, value }),
  listTags: () => invoke<string[]>("list_tags"),
  setProfileTags: (id: string, tags: string[]) =>
    invoke<void>("set_profile_tags", { id, tags }),
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

/** True when running inside the Tauri WebView (invoke available). */
export function isTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}
