import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { api, isTauri } from "../lib/api";
import en from "./en.json";
import vi from "./vi.json";

const STORAGE_KEY = "browserx.locale";
export const SUPPORTED_LOCALES = [
  { code: "vi", label: "Tiếng Việt" },
  { code: "en", label: "English" },
] as const;

function readStoredLocale(): string {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored && SUPPORTED_LOCALES.some((l) => l.code === stored)) return stored;
  return "vi";
}

i18n.use(initReactI18next).init({
  resources: {
    vi: { translation: vi },
    en: { translation: en },
  },
  lng: readStoredLocale(),
  fallbackLng: "en",
  supportedLngs: SUPPORTED_LOCALES.map((l) => l.code),
  interpolation: { escapeValue: false },
});

document.documentElement.lang = i18n.language;

// Prefer the persisted backend setting when available.
if (isTauri()) {
  api
    .getSettings()
    .then((s) => {
      if (s.locale && SUPPORTED_LOCALES.some((l) => l.code === s.locale)) {
        void i18n.changeLanguage(s.locale);
        document.documentElement.lang = s.locale;
      }
    })
    .catch(() => {});
}

export function setLocale(code: string) {
  void i18n.changeLanguage(code);
  document.documentElement.lang = code;
  localStorage.setItem(STORAGE_KEY, code);
  if (isTauri()) api.setSetting("locale", code).catch(() => {});
}

export default i18n;
