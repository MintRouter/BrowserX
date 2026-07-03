import type {
  GeolocationMode,
  Platform,
  StartupBehavior,
  WebrtcMode,
} from "../../lib/api";

/** Local editing state for the profile form (superset of ProfileInput minus launch_args, which is edited as JSON text). */
export interface FormState {
  name: string;
  fingerprint_seed: string | null;
  platform: Platform;
  timezone: string | null;
  locale: string | null;
  screen_width: number;
  screen_height: number;
  gpu_vendor: string | null;
  gpu_renderer: string | null;
  hardware_concurrency: number | null;
  humanize: boolean;
  human_preset: string;
  headless: boolean;
  geoip: boolean;
  color_scheme: string | null;
  notes: string;
  proxy_id: string | null;
  tags: string[];
  folder_id: string | null;
  startup_behavior: StartupBehavior;
  startup_urls: string[];
  fp_noise: boolean;
  webrtc_mode: WebrtcMode;
  webrtc_ip: string | null;
  geolocation_mode: GeolocationMode;
  geo_latitude: string | null;
  geo_longitude: string | null;
  /** (W20b) Storage options — false = wiped from disk when the session stops. */
  store_history: boolean;
  store_passwords: boolean;
  store_sw_cache: boolean;
  /** (W24b) Local unpacked extension paths — passed as --load-extension on launch. */
  extensions: string[];
}

/** True when `raw` parses as an http(s) URL (startup URL validation). */
export function isValidStartupUrl(raw: string): boolean {
  try {
    const url = new URL(raw.trim());
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

export type SetField = <K extends keyof FormState>(
  key: K,
  value: FormState[K],
) => void;

export const RESOLUTION_PRESETS: ReadonlyArray<{
  label: string;
  width: number;
  height: number;
}> = [
  { label: "1920 × 1080", width: 1920, height: 1080 },
  { label: "1440 × 900", width: 1440, height: 900 },
  { label: "2560 × 1440", width: 2560, height: 1440 },
];

export const HARDWARE_CONCURRENCY_OPTIONS = [2, 4, 8, 12, 16] as const;
