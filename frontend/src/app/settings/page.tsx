import Link from "next/link";
import { AccountSettings } from "@/components/account-settings";
import { ApiKeysSettings } from "@/components/api-keys-settings";
import { ModelsSettings } from "@/components/models-settings";

export default function SettingsPage() {
  return (
    <main className="mx-auto max-w-3xl space-y-8 px-4 py-6">
      <Link href="/" className="text-2xl font-semibold tracking-tight text-brand">
        ferret
      </Link>
      <h1 className="text-xl font-semibold">Settings</h1>
      <section className="space-y-4">
        <div>
          <h2 className="text-base font-medium">Account</h2>
          <p className="text-sm text-muted-foreground">
            A new password signs you out everywhere else. This browser stays signed in.
          </p>
        </div>
        <AccountSettings />
      </section>
      <section className="space-y-4">
        <div>
          <h2 className="text-base font-medium">API keys</h2>
          <p className="text-sm text-muted-foreground">
            For agents, scripts and external ingesters. Send a key as{" "}
            <code className="font-mono text-xs">Authorization: Bearer fk_…</code>.
          </p>
        </div>
        <ApiKeysSettings />
      </section>
      <section className="space-y-4">
        <div>
          <h2 className="text-base font-medium">Models</h2>
          <p className="text-sm text-muted-foreground">
            The embedding model your searches use, in the UI and through your API keys. A model
            can be chosen before it has embedded everything: until then its semantic matches are
            partial.
          </p>
        </div>
        <ModelsSettings />
      </section>
    </main>
  );
}
