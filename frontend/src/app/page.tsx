"use client";

import { useSearchParams } from "next/navigation";
import { Suspense } from "react";
import { SearchField } from "@/components/search-field";
import { SearchResults } from "@/components/search-results";
import { parseSearch } from "@/lib/search-url";
import { submitSearch, typeSearch } from "@/lib/search-history";
import { BrandMark } from "@/components/brand-mark";

function SearchRoute() {
  // Null outside Next's router (tests): the home page.
  const params = useSearchParams();
  const state = parseSearch(new URLSearchParams(params?.toString()));

  if (state.q) {
    return <SearchResults state={state} onChange={submitSearch} onType={typeSearch} />;
  }
  return (
    <main className="flex min-h-dvh flex-col items-center px-4 pt-[22vh]">
      <h1 className="mb-8 flex items-baseline gap-3 text-6xl font-semibold tracking-tight text-brand select-none">
        <BrandMark className="h-[0.8em] w-auto" />
        ferret
      </h1>
      <SearchField onSearch={(q) => submitSearch({ ...state, q })} />
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
