import { Connections } from "@/components/connections";
import { HomeLink } from "@/components/brand-mark";

export default function ConnectionsPage() {
  return (
    <main className="mx-auto max-w-3xl space-y-8 px-4 py-6">
      <HomeLink />
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
