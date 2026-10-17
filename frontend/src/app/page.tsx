"use client";

import { useSearchParams } from "next/navigation";
import { Suspense } from "react";
import { SearchResults } from "@/components/search-results";
import { parseSearch } from "@/lib/search-url";
import { submitSearch, typeSearch } from "@/lib/search-history";
import { Home } from "@/components/home";

function SearchRoute() {
  // Null outside Next's router (tests): the home page.
  const params = useSearchParams();
  const state = parseSearch(new URLSearchParams(params?.toString()));

  if (state.q) {
    return <SearchResults state={state} onChange={submitSearch} onType={typeSearch} />;
  }
  return <Home onSearch={(q) => submitSearch({ ...state, q })} />;
}

export default function SearchPage() {
  // useSearchParams needs a Suspense boundary in a static export.
  return (
    <Suspense>
      <SearchRoute />
    </Suspense>
  );
}
