import { Connections } from "@/components/connections";
import { PageShell } from "@/components/page-shell";

export default function ConnectionsPage() {
  return (
    <PageShell href="/connections" description="Plugins that keep a source in sync with an origin, on a schedule.">
      <Connections />
    </PageShell>
  );
}
