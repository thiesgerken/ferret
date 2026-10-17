import { Suspense } from "react";
import { ActivityRoute } from "@/components/activity";
import { PageShell } from "@/components/page-shell";

export default function ActivityPage() {
  return (
    <PageShell href="/activity">
      <Suspense>
        <ActivityRoute />
      </Suspense>
    </PageShell>
  );
}
