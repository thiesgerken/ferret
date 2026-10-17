"use client";

import { useSearchParams } from "next/navigation";
import { Suspense } from "react";
import { SearchField } from "@/components/search-field";
import { SearchResults } from "@/components/search-results";
import { parseSearch, searchHref, type SearchState } from "@/lib/search-url";
import { BrandMark } from "@/components/brand-mark";

/** `history.pushState` keeps the page and updates `useSearchParams`, Next's router follows it. */
function navigate(state: SearchState) {
  window.history.pushState(null, "", searchHref(state));
}

function SearchRoute() {
  // Null outside Next's router (tests): the home page.
  const params = useSearchParams();
  const state = parseSearch(new URLSearchParams(params?.toString()));

  if (state.q) {
    return <SearchResults state={state} onChange={navigate} />;
  }
  return (
    <main className="flex min-h-dvh flex-col items-center px-4 pt-[22vh]">
      <h1 className="mb-8 flex items-baseline gap-3 text-6xl font-semibold tracking-tight text-brand select-none">
        <BrandMark className="h-[0.8em] w-auto" />
        ferret
      </h1>
      <SearchField onSearch={(q) => navigate({ ...state, q })} />
    </main>
  );
}

export default function SearchPage() {
  // useSearchParams needs a Suspense boundary in a static export.
  return (
    <Suspense>
      <SearchRoute />
    </Suspense>
  );
}
