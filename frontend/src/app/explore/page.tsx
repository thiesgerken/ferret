"use client";

import { useSearchParams } from "next/navigation";
import { useTranslations } from "next-intl";
import { Suspense } from "react";
import { Explore } from "@/components/explore";
import { exploreHref, parseExplore, type ExploreState } from "@/lib/explore-url";
import { HomeLink } from "@/components/brand-mark";

/** `history.pushState` keeps the page and updates `useSearchParams`, Next's router follows it. */
function navigate(state: ExploreState) {
  window.history.pushState(null, "", exploreHref(state));
}

function ExploreRoute() {
  const params = useSearchParams();
  return <Explore state={parseExplore(new URLSearchParams(params?.toString()))} onChange={navigate} />;
}

export default function ExplorePage() {
  const t = useTranslations("pages");
  return (
    <main className="mx-auto max-w-7xl space-y-6 px-4 py-6">
      <HomeLink />
      <h1 className="text-xl font-semibold">{t("explore.title")}</h1>
      {/* useSearchParams needs a Suspense boundary in a static export. */}
      <Suspense>
        <ExploreRoute />
      </Suspense>
    </main>
  );
}
