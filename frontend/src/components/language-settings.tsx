"use client";

import { useTranslations } from "next-intl";
import { useLocaleSetting } from "@/components/locale-provider";
import { LOCALES, type LocaleSetting } from "@/lib/i18n";
import { fieldClass } from "@/lib/utils";

export function LanguageSettings() {
  const t = useTranslations("settings.language");
  const { setting, systemLocale, setSetting } = useLocaleSetting();
  return (
    <label className="block max-w-sm space-y-1.5">
      <span className="text-xs font-medium text-muted-foreground">{t("label")}</span>
      <select value={setting} onChange={(event) => setSetting(event.target.value as LocaleSetting)} className={fieldClass}>
        <option value="system">{t("system", { language: t(systemLocale) })}</option>
        {LOCALES.map((locale) => (
          <option key={locale} value={locale}>
            {t(locale)}
          </option>
        ))}
      </select>
    </label>
  );
}
