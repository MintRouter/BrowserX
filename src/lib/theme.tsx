import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { api, isTauri } from "./api";

export type Theme = "light" | "dark";
/** User preference — "system" follows the OS via matchMedia. */
export type ThemeMode = Theme | "system";

const STORAGE_KEY = "browserx.theme"; // resolved theme, read by the FOUC script in index.html
const MODE_KEY = "browserx.themeMode"; // user preference, may be "system"

function systemTheme(): Theme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

function isMode(v: unknown): v is ThemeMode {
  return v === "light" || v === "dark" || v === "system";
}

function readStoredMode(): ThemeMode {
  const mode = localStorage.getItem(MODE_KEY);
  if (isMode(mode)) return mode;
  // Legacy key from before "system" existed.
  const legacy = localStorage.getItem(STORAGE_KEY);
  if (legacy === "light" || legacy === "dark") return legacy;
  return "system";
}

function applyTheme(theme: Theme) {
  document.documentElement.classList.toggle("dark", theme === "dark");
}

interface ThemeContextValue {
  /** Resolved theme actually applied ("system" already resolved). */
  theme: Theme;
  mode: ThemeMode;
  setMode: (mode: ThemeMode) => void;
  setTheme: (theme: Theme) => void;
}

const ThemeContext = createContext<ThemeContextValue>({
  theme: "dark",
  mode: "dark",
  setMode: () => {},
  setTheme: () => {},
});

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(readStoredMode);
  const [osTheme, setOsTheme] = useState<Theme>(systemTheme);
  const theme = mode === "system" ? osTheme : mode;

  useEffect(() => {
    applyTheme(theme);
    // Keep the resolved theme cached for the pre-mount FOUC script.
    localStorage.setItem(STORAGE_KEY, theme);
  }, [theme]);

  // Follow OS theme changes live while in "system" mode.
  useEffect(() => {
    if (mode !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => setOsTheme(mq.matches ? "dark" : "light");
    onChange();
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [mode]);

  // Prefer the persisted backend setting when available.
  useEffect(() => {
    if (!isTauri()) return;
    api
      .getSettings()
      .then((s) => {
        if (isMode(s.themeMode)) setModeState(s.themeMode);
        else if (s.theme === "light" || s.theme === "dark")
          setModeState(s.theme);
      })
      .catch(() => {});
  }, []);

  const setMode = useCallback((next: ThemeMode) => {
    setModeState(next);
    localStorage.setItem(MODE_KEY, next);
    if (isTauri()) {
      api.setSetting("themeMode", next).catch(() => {});
      const resolved = next === "system" ? systemTheme() : next;
      api.setSetting("theme", resolved).catch(() => {});
    }
  }, []);

  // Backward-compatible alias for callers that only know light/dark.
  const setTheme = useCallback((next: Theme) => setMode(next), [setMode]);

  return (
    <ThemeContext.Provider value={{ theme, mode, setMode, setTheme }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  return useContext(ThemeContext);
}
