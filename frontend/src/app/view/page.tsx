"use client";

import { useSearchParams } from "next/navigation";
import { Suspense, useMemo } from "react";
import { DocumentViewer } from "@/components/document-viewer";

// A query-param route, not /documents/[id]: the static export has no page for every id, so only
// this one survives a reload or a shared link.
function ViewRoute() {
  const params = useSearchParams();
  const id = params?.get("id");
  const start = Number(params?.get("start") ?? NaN);
  const end = Number(params?.get("end") ?? NaN);
  // Stable across renders: the viewer scrolls to the hit whenever it changes.
  const hit = useMemo(
    () => (Number.isInteger(start) && Number.isInteger(end) && end > start ? { start, end } : null),
    [start, end],
  );
  if (!id) {
    return <p className="p-8 text-center text-sm text-destructive">No document given.</p>;
  }
  return <DocumentViewer id={id} hit={hit} />;
}

export default function ViewPage() {
  return (
    <Suspense>
      <ViewRoute />
    </Suspense>
  );
}
