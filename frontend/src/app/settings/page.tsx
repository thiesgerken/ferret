import Link from "next/link";
import { ApiKeysSettings } from "@/components/api-keys-settings";

export default function SettingsPage() {
  return (
    <main className="mx-auto max-w-3xl space-y-8 px-4 py-6">
      <Link href="/" className="text-2xl font-semibold tracking-tight text-brand">
        ferret
      </Link>
      <h1 className="text-xl font-semibold">Settings</h1>
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
    </main>
  );
}
