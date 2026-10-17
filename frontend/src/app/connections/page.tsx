import Link from "next/link";
import { Connections } from "@/components/connections";

export default function ConnectionsPage() {
  return (
    <main className="mx-auto max-w-3xl space-y-8 px-4 py-6">
      <Link href="/" className="text-2xl font-semibold tracking-tight text-brand">
        ferret
      </Link>
      <div>
        <h1 className="text-xl font-semibold">Connections</h1>
        <p className="text-sm text-muted-foreground">
          Plugins that keep a source in sync with an origin, on a schedule.
        </p>
      </div>
      <Connections />
    </main>
  );
}
