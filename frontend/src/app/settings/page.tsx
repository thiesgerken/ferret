"use client";

import { Brain, KeyRound, Languages, UserRound } from "lucide-react";
import { useTranslations } from "next-intl";
import { AccountSettings } from "@/components/account-settings";
import { ApiKeysSettings } from "@/components/api-keys-settings";
import { LanguageSettings } from "@/components/language-settings";
import { ModelsSettings } from "@/components/models-settings";
import { PageShell, Section } from "@/components/page-shell";

export default function SettingsPage() {
  const t = useTranslations("settings");
  return (
    <PageShell href="/settings">
      <Section title={t("account.title")} icon={<UserRound />} description={t("account.description")}>
        <AccountSettings />
      </Section>
      <Section
        title={t("apiKeys.title")}
        icon={<KeyRound />}
        description={t.rich("apiKeys.description", {
          code: (chunks) => <code className="font-mono text-xs">{chunks}</code>,
        })}
      >
        <ApiKeysSettings />
      </Section>
      <Section title={t("models.title")} icon={<Brain />} description={t("models.description")}>
        <ModelsSettings />
      </Section>
      <Section title={t("language.title")} icon={<Languages />} description={t("language.description")}>
        <LanguageSettings />
      </Section>
    </PageShell>
  );
}
