import type { Platform } from "./api";

/** Detect the host OS from the WebView user agent (no extra Tauri plugin needed). */
export function detectHostPlatform(): Platform {
  const ua = navigator.userAgent;
  if (/Windows/i.test(ua)) return "windows";
  if (/Mac OS X|Macintosh/i.test(ua)) return "macos";
  return "linux";
}
