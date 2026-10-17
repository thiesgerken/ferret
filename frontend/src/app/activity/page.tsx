import { Activity } from "@/components/activity";
import { PageShell } from "@/components/page-shell";

export default function ActivityPage() {
  return (
    <PageShell href="/activity" description="What ferret ingested and embedded, live.">
      <Activity />
    </PageShell>
  );
}
