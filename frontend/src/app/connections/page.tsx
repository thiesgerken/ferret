import Link from "next/link";
import { Connections } from "@/components/connections";
import { BrandMark } from "@/components/brand-mark";

export default function ConnectionsPage() {
  return (
    <main className="mx-auto max-w-3xl space-y-8 px-4 py-6">
      <Link href="/" className="inline-flex items-baseline gap-1.5 text-2xl font-semibold tracking-tight text-brand">
        <BrandMark className="h-[0.8em] w-auto" />
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
