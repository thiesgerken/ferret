import { Brain, KeyRound, UserRound } from "lucide-react";
import { AccountSettings } from "@/components/account-settings";
import { ApiKeysSettings } from "@/components/api-keys-settings";
import { ModelsSettings } from "@/components/models-settings";
import { PageShell, Section } from "@/components/page-shell";

export default function SettingsPage() {
  return (
    <PageShell href="/settings" description="Your account, the keys your agents and scripts use, and the model your searches use.">
      <Section
        title="Account"
        icon={<UserRound />}
        description="A new password signs you out everywhere else. This browser stays signed in."
      >
        <AccountSettings />
      </Section>
      <Section
        title="API keys"
        icon={<KeyRound />}
        description={
          <>
            For agents, scripts and external ingesters. Send a key as{" "}
            <code className="font-mono text-xs">Authorization: Bearer fk_…</code>.
          </>
        }
      >
        <ApiKeysSettings />
      </Section>
      <Section
        title="Models"
        icon={<Brain />}
        description="The embedding model your searches use, in the UI and through your API keys. A model can be chosen before it has embedded everything: until then its semantic matches are partial."
      >
        <ModelsSettings />
      </Section>
    </PageShell>
  );
}
