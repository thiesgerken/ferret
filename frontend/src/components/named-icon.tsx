"use client";

import { CircleDashed } from "lucide-react";
import { lazy, Suspense } from "react";

// Lazy, so lucide's map of every icon's import stays out of the pages' first load.
const NamedIconLoader = lazy(() => import("./named-icon-loader"));

/** A lucide icon stored by name (a connection's or API key's icon), each loaded on its own. */
export function NamedIcon({ name, className }: { name: string; className?: string }) {
  return (
    <Suspense fallback={<CircleDashed className={className} aria-hidden />}>
      <NamedIconLoader name={name} className={className} />
    </Suspense>
  );
}
