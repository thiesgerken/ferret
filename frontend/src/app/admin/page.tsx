import { Admin } from "@/components/admin";
import { PageShell } from "@/components/page-shell";

export default function AdminPage() {
  return (
    <PageShell href="/admin">
      <Admin />
    </PageShell>
  );
}
