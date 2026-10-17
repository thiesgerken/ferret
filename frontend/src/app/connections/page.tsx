import { Connections } from "@/components/connections";
import { PageShell } from "@/components/page-shell";

export default function ConnectionsPage() {
  return (
    <PageShell href="/connections">
      <Connections />
    </PageShell>
  );
}
