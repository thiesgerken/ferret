"use client";

import { NextIntlClientProvider } from "next-intl";
import { createContext, useContext, useEffect, useSyncExternalStore, type ReactNode } from "react";
import { browserLocale, loadLocaleSetting, MESSAGES, saveLocaleSetting, type Locale, type LocaleSetting } from "@/lib/i18n";

type LocaleContextValue = {
  setting: LocaleSetting;
  /** The browser's language, what "system" resolves to. */
  systemLocale: Locale;
  setSetting: (setting: LocaleSetting) => void;
};

const LocaleContext = createContext<LocaleContextValue | null>(null);

// The storage event only reaches other tabs, so this tab's own changes notify by hand.
const listeners = new Set<() => void>();

function subscribe(listener: () => void) {
  listeners.add(listener);
  window.addEventListener("storage", listener);
  window.addEventListener("languagechange", listener);
  return () => {
    listeners.delete(listener);
    window.removeEventListener("storage", listener);
    window.removeEventListener("languagechange", listener);
  };
}

function setSetting(setting: LocaleSetting) {
  saveLocaleSetting(setting);
  listeners.forEach((listener) => listener());
}

/** Picks the UI language: the user's setting, else the browser's languages. The static export
 * prerenders in English, the server snapshot, and switches once hydrated. */
export function LocaleProvider({ children }: { children: ReactNode }) {
  const setting = useSyncExternalStore(subscribe, loadLocaleSetting, () => "system" as const);
  const systemLocale = useSyncExternalStore(subscribe, () => browserLocale(navigator.languages), () => "en" as const);
  const locale = setting === "system" ? systemLocale : setting;

  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  return (
    <LocaleContext.Provider value={{ setting, systemLocale, setSetting }}>
      {/* Dates show in the browser's zone. */}
      <NextIntlClientProvider locale={locale} messages={MESSAGES[locale]} timeZone={Intl.DateTimeFormat().resolvedOptions().timeZone}>
        {children}
      </NextIntlClientProvider>
    </LocaleContext.Provider>
  );
}

export function useLocaleSetting(): LocaleContextValue {
  const value = useContext(LocaleContext);
  if (!value) throw new Error("useLocaleSetting outside LocaleProvider");
  return value;
}
