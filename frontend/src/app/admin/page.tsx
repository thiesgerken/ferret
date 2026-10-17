import Link from "next/link";
import { Admin } from "@/components/admin";

export default function AdminPage() {
  return (
    <main className="mx-auto max-w-3xl space-y-8 px-4 py-6">
      <Link href="/" className="text-2xl font-semibold tracking-tight text-brand">
        ferret
      </Link>
      <div>
        <h1 className="text-xl font-semibold">Admin</h1>
        <p className="text-sm text-muted-foreground">Users, the embedding pipeline and the plugin workers.</p>
      </div>
      <Admin />
    </main>
  );
}
