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

const STORAGE_KEY = "browserx.theme";

function readStoredTheme(): Theme {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

function applyTheme(theme: Theme) {
  document.documentElement.classList.toggle("dark", theme === "dark");
}

interface ThemeContextValue {
  theme: Theme;
  setTheme: (theme: Theme) => void;
}

const ThemeContext = createContext<ThemeContextValue>({
  theme: "dark",
  setTheme: () => {},
});

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(readStoredTheme);

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  // Prefer the persisted backend setting when available.
  useEffect(() => {
    if (!isTauri()) return;
    api
      .getSettings()
      .then((s) => {
        if (s.theme === "light" || s.theme === "dark") setThemeState(s.theme);
      })
      .catch(() => {});
  }, []);

  const setTheme = useCallback((next: Theme) => {
    setThemeState(next);
    localStorage.setItem(STORAGE_KEY, next);
    if (isTauri()) api.setSetting("theme", next).catch(() => {});
  }, []);

  return (
    <ThemeContext.Provider value={{ theme, setTheme }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  return useContext(ThemeContext);
}
