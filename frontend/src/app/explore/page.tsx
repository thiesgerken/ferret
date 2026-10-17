"use client";

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense } from "react";
import { Explore } from "@/components/explore";
import { exploreHref, parseExplore, type ExploreState } from "@/lib/explore-url";
import { BrandMark } from "@/components/brand-mark";

/** `history.pushState` keeps the page and updates `useSearchParams`, Next's router follows it. */
function navigate(state: ExploreState) {
  window.history.pushState(null, "", exploreHref(state));
}

function ExploreRoute() {
  const params = useSearchParams();
  return <Explore state={parseExplore(new URLSearchParams(params?.toString()))} onChange={navigate} />;
}

export default function ExplorePage() {
  return (
    <main className="mx-auto max-w-6xl space-y-6 px-4 py-6">
      <Link href="/" className="inline-flex items-baseline gap-1.5 text-2xl font-semibold tracking-tight text-brand">
        <BrandMark className="h-[0.8em] w-auto" />
        ferret
      </Link>
      <h1 className="text-xl font-semibold">Explore</h1>
      {/* useSearchParams needs a Suspense boundary in a static export. */}
      <Suspense>
        <ExploreRoute />
      </Suspense>
    </main>
  );
}
