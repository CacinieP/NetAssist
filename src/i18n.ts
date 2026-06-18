import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zh from "./locales/zh.json";
import en from "./locales/en.json";

export const SUPPORTED_LANGUAGES = ["zh-CN", "en-US"] as const;
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];

/**
 * Initialize i18next. The active language is controlled by
 * `changeLanguage()` calls from the Settings store — this default just boots
 * synchronously with Chinese; App.tsx re-syncs to the persisted setting on
 * load.
 */
void i18n.use(initReactI18next).init({
  resources: {
    "zh-CN": { translation: zh },
    "en-US": { translation: en },
  },
  lng: "zh-CN",
  fallbackLng: "zh-CN",
  interpolation: { escapeValue: false },
});

export default i18n;
