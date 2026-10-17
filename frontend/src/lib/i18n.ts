import de from "../../messages/de.json";
import en from "../../messages/en.json";

export const LOCALES = ["en", "de"] as const;
export type Locale = (typeof LOCALES)[number];
/** What the user picked in Settings; "system" follows the browser's languages. */
export type LocaleSetting = Locale | "system";

export const MESSAGES: Record<Locale, typeof en> = { en, de };

/** The first of the browser's languages ferret speaks, English if none. */
export function browserLocale(languages: readonly string[]): Locale {
  for (const language of languages) {
    const base = language.toLowerCase().split("-")[0];
    if ((LOCALES as readonly string[]).includes(base)) return base as Locale;
  }
  return "en";
}

// The setting is per browser, like the theme: it needs no account round trip and applies on the
// sign-in page too.
const STORAGE_KEY = "ferret.locale";

export function loadLocaleSetting(): LocaleSetting {
  const stored = localStorage.getItem(STORAGE_KEY);
  return stored === "system" || (LOCALES as readonly string[]).includes(stored ?? "") ? (stored as LocaleSetting) : "system";
}

export function saveLocaleSetting(setting: LocaleSetting): void {
  localStorage.setItem(STORAGE_KEY, setting);
}
